# Docker Container Isolation for Phantom Harness

## Executive Summary

This document outlines a design to enhance Phantom Harness with Docker container isolation for AI code agents. The goal is to provide defense-in-depth security where agents operate in sandboxed environments that can only communicate back to the app through secure, controlled channels—preventing potential breakouts that could cause havoc on the host system.

## Current Architecture

### How Agents Run Today

```
┌─────────────────────────────────────────────────────────────┐
│                     Phantom Harness (Tauri)                  │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                   AppState                           │    │
│  │  • Sessions map (task_id → SessionHandle)           │    │
│  │  • SQLite DB connection                             │    │
│  │  • Settings (API keys, preferences)                 │    │
│  └─────────────────────────────────────────────────────┘    │
│                            │                                 │
│                   spawn subprocess                          │
│                            ▼                                │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              AgentProcessClient                      │    │
│  │  • JSON-RPC over stdio (Codex, ACP)                 │    │
│  │  • NDJSON streaming (Claude, Factory Droid)         │    │
│  │  • Full host filesystem access                      │    │
│  │  • Full network access                              │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

**Current Isolation: Git Worktrees**
- Each task gets an isolated git worktree at `~/phantom-harness/workspaces/<repo>/<animal>/`
- Provides branch isolation but NOT process isolation
- Agent can access ANY file the Tauri process can access
- No resource limits (CPU, memory, disk, network)

### Security Gaps

1. **No Process Isolation**: Agents run as direct subprocesses with full host access
2. **Credential Exposure**: API keys passed via environment variables to agents
3. **No Resource Limits**: A runaway agent can consume unlimited CPU/memory
4. **Network Access**: Agents can make arbitrary network requests
5. **File Access**: Agents can read/write anywhere the user can

## Proposed Architecture

### Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Phantom Harness (Host)                           │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │                          AppState                                │    │
│  │  • ContainerManager (Docker API via bollard)                    │    │
│  │  • Sessions map (task_id → ContainerHandle)                     │    │
│  │  • SQLite DB (host-side persistence)                            │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                                │                                         │
│              Docker API (unix socket / named pipe)                      │
│                                ▼                                        │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                    Docker Container (gVisor/runc)                 │   │
│  │  ┌────────────────────────────────────────────────────────┐      │   │
│  │  │  Agent Process (claude/codex/droid)                     │      │   │
│  │  │  • Read-write: /workspace (mounted worktree)            │      │   │
│  │  │  • Read-only: /credentials (auth tokens)                │      │   │
│  │  │  • No host filesystem access                            │      │   │
│  │  │  • Filtered network (egress whitelist)                  │      │   │
│  │  └────────────────────────────────────────────────────────┘      │   │
│  │                           │                                       │   │
│  │           stdio (stdin/stdout/stderr)                            │   │
│  │                           ▼                                      │   │
│  │  ┌────────────────────────────────────────────────────────┐      │   │
│  │  │              Bridge Process (harness-bridge)            │      │   │
│  │  │  • JSON-RPC multiplexer                                 │      │   │
│  │  │  • Credential injection (never exposes raw keys)        │      │   │
│  │  │  • Request validation & rate limiting                   │      │   │
│  │  │  • Communicates via Unix socket to host                │      │   │
│  │  └────────────────────────────────────────────────────────┘      │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                │                                         │
│                    Unix Socket: /tmp/phantom-{task_id}.sock             │
│                                │                                         │
└────────────────────────────────┼────────────────────────────────────────┘
                                 │
                    Bidirectional JSON-RPC
```

### Security Layers

#### Layer 1: Container Runtime Isolation

**Options (in order of security strength):**

| Runtime | Isolation Level | Performance | Compatibility |
|---------|-----------------|-------------|---------------|
| **gVisor (runsc)** | Kernel-level sandbox | ~5-15% overhead | Good (some syscall limitations) |
| **Kata Containers** | Micro-VM | ~10-20% overhead | Excellent |
| **runc + user namespaces** | Namespace isolation | Minimal | Excellent |

**Recommendation**: Start with **runc + rootless mode + user namespaces** for broad compatibility, with optional **gVisor** for high-security workloads.

#### Layer 2: Capability Restrictions

```yaml
# Container security profile
security_opt:
  - no-new-privileges:true
  - seccomp:phantom-seccomp.json
cap_drop:
  - ALL
cap_add:
  - NET_BIND_SERVICE  # Only if needed for agent auth
read_only: true
tmpfs:
  - /tmp:size=100M,mode=1777
```

#### Layer 3: Resource Limits

```yaml
# Per-container limits (configurable per agent)
resources:
  limits:
    cpus: "2"           # Max 2 CPU cores
    memory: "4g"        # Max 4GB RAM
    pids: 256           # Max processes
  reservations:
    cpus: "0.5"
    memory: "512m"
```

#### Layer 4: Network Isolation

```yaml
networks:
  phantom-agents:
    driver: bridge
    internal: false  # Allow egress
    driver_opts:
      # Egress filtering via iptables rules
      com.docker.network.bridge.enable_ip_masquerade: "true"
```

**Network Policy (per-agent whitelist):**
```
# Claude Code needs:
- api.anthropic.com:443
- github.com:443
- *.githubusercontent.com:443

# Codex needs:
- api.openai.com:443
- github.com:443

# All agents:
- DNS resolution (53/udp to host resolver)
```

#### Layer 5: Filesystem Isolation

| Mount | Mode | Purpose |
|-------|------|---------|
| `/workspace` | read-write | Git worktree (only this task's files) |
| `/credentials` | read-only | OAuth tokens, session files |
| `/tmp` | tmpfs | Temporary files (memory-backed, size-limited) |
| `/home/agent` | read-only | Agent CLI binaries and config |

**Explicitly NOT Mounted:**
- Host home directory
- Docker socket
- Host network namespace
- Any path outside the worktree

### Container Images

#### Base Image Strategy

```dockerfile
# phantom-agent-base:latest
FROM debian:bookworm-slim

# Minimal runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    git \
    curl \
    jq \
    && rm -rf /var/lib/apt/lists/*

# Non-root user
RUN useradd -m -s /bin/bash -u 1000 agent
USER agent
WORKDIR /workspace

# Bridge process for secure communication
COPY --chown=agent:agent harness-bridge /usr/local/bin/
ENTRYPOINT ["/usr/local/bin/harness-bridge"]
```

#### Agent-Specific Images

```dockerfile
# phantom-agent-claude:latest
FROM phantom-agent-base:latest

# Install Claude CLI
RUN curl -fsSL https://claude.ai/install.sh | sh
ENV PATH="/home/agent/.claude/bin:$PATH"

# Pre-configured for stream-json output
ENV CLAUDE_OUTPUT_FORMAT=stream-json
```

```dockerfile
# phantom-agent-codex:latest
FROM phantom-agent-base:latest

# Install Codex CLI
RUN npm install -g @openai/codex
ENV PATH="/home/agent/.npm-global/bin:$PATH"
```

### Secure Communication Protocol

#### Bridge Process Architecture

The `harness-bridge` is a minimal Rust binary that runs inside the container and mediates all communication:

```rust
// harness-bridge/src/main.rs (conceptual)
pub struct Bridge {
    /// Unix socket connection to host
    host_socket: UnixStream,
    /// Subprocess handle for the agent
    agent_process: Child,
    /// Request ID counter
    next_id: AtomicU64,
}

impl Bridge {
    /// Main loop: proxy messages between agent and host
    async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                // Agent → Host: validate and forward
                line = self.agent_stdout.next_line() => {
                    if let Some(msg) = line? {
                        self.validate_and_forward_to_host(msg).await?;
                    }
                }
                // Host → Agent: inject credentials and forward
                msg = self.read_from_host() => {
                    let enriched = self.inject_credentials(msg?)?;
                    self.agent_stdin.write_all(enriched.as_bytes()).await?;
                }
            }
        }
    }

    /// Validate outbound messages (prevent data exfiltration)
    fn validate_and_forward_to_host(&self, msg: String) -> Result<()> {
        let parsed: Value = serde_json::from_str(&msg)?;

        // Block attempts to send raw credentials
        if self.contains_credential_pattern(&parsed) {
            return Err(anyhow!("Blocked: message contains credential pattern"));
        }

        // Forward validated message
        self.host_socket.write_all(msg.as_bytes()).await?;
        Ok(())
    }
}
```

#### Communication Flow

```
1. Host creates Unix socket: /tmp/phantom-{task_id}.sock
2. Host starts container with socket bind-mounted
3. Bridge connects to socket on startup
4. Host sends: {"jsonrpc":"2.0","method":"session/new","params":{...},"id":1}
5. Bridge spawns agent subprocess
6. Bridge forwards request to agent stdin
7. Agent responds via stdout
8. Bridge validates response, forwards to host via socket
9. Host processes response, updates UI
```

#### Credential Handling

**Never expose raw API keys to containers.** Instead:

1. **OAuth tokens**: Mount as read-only files at `/credentials/`
2. **API keys**: Bridge injects into requests on-the-fly
3. **Session tokens**: Stored on host, referenced by ID

```rust
// On host side
fn prepare_container_credentials(settings: &Settings, agent_id: &str) -> ContainerCredentials {
    match agent_id {
        "claude-code" => {
            // Copy OAuth session to temp location
            let session_path = dirs::home_dir()
                .unwrap()
                .join(".claude")
                .join("credentials.json");
            ContainerCredentials::OAuthFile(session_path)
        }
        "codex" => {
            // Codex uses ChatGPT OAuth, also file-based
            let session_path = dirs::home_dir()
                .unwrap()
                .join(".codex")
                .join("auth.json");
            ContainerCredentials::OAuthFile(session_path)
        }
        _ => {
            // API key injection via bridge
            ContainerCredentials::ApiKey(settings.anthropic_api_key.clone())
        }
    }
}
```

### Container Lifecycle Management

#### State Machine

```
                    ┌─────────────┐
                    │   Created   │
                    └──────┬──────┘
                           │ start()
                           ▼
┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│   Stopped   │◄───│   Running   │───►│   Paused    │
└──────┬──────┘    └──────┬──────┘    └─────────────┘
       │                  │
       │ remove()         │ timeout/error
       ▼                  ▼
┌─────────────┐    ┌─────────────┐
│   Removed   │    │   Failed    │
└─────────────┘    └─────────────┘
```

#### ContainerManager API

```rust
pub struct ContainerManager {
    docker: Docker,  // bollard client
    containers: HashMap<String, ContainerHandle>,
    config: ContainerConfig,
}

impl ContainerManager {
    /// Create and start a container for an agent task
    pub async fn spawn_agent(
        &mut self,
        task_id: &str,
        agent_id: &str,
        worktree_path: &Path,
        credentials: &ContainerCredentials,
    ) -> Result<ContainerHandle> {
        let image = self.resolve_image(agent_id)?;
        let socket_path = format!("/tmp/phantom-{}.sock", task_id);

        // Create Unix socket for communication
        let listener = UnixListener::bind(&socket_path)?;

        let config = ContainerConfigBuilder::new()
            .image(&image)
            .hostname(&format!("phantom-{}", &task_id[..8]))
            .user("agent")
            .working_dir("/workspace")
            // Security
            .security_opt(vec!["no-new-privileges:true"])
            .cap_drop(vec!["ALL"])
            .read_only(true)
            // Resources
            .memory(self.config.memory_limit)
            .cpu_quota(self.config.cpu_quota)
            .pids_limit(self.config.max_pids)
            // Mounts
            .bind(worktree_path, "/workspace", "rw")
            .bind(&socket_path, "/run/phantom.sock", "rw")
            .tmpfs("/tmp", "size=100M,mode=1777")
            // Network
            .network_mode(&self.config.network)
            .build()?;

        let container = self.docker.create_container(
            Some(CreateContainerOptions { name: task_id }),
            config,
        ).await?;

        self.docker.start_container::<String>(&container.id, None).await?;

        // Wait for bridge to connect
        let stream = listener.accept().await?;

        Ok(ContainerHandle {
            id: container.id,
            task_id: task_id.to_string(),
            socket: stream,
            status: ContainerStatus::Running,
        })
    }

    /// Stop a container gracefully
    pub async fn stop(&mut self, task_id: &str, timeout_secs: u64) -> Result<()> {
        if let Some(handle) = self.containers.get(task_id) {
            self.docker.stop_container(
                &handle.id,
                Some(StopContainerOptions { t: timeout_secs as i64 }),
            ).await?;
        }
        Ok(())
    }

    /// Force kill a container
    pub async fn kill(&mut self, task_id: &str) -> Result<()> {
        if let Some(handle) = self.containers.get(task_id) {
            self.docker.kill_container::<String>(&handle.id, None).await?;
        }
        Ok(())
    }

    /// Remove container and cleanup resources
    pub async fn remove(&mut self, task_id: &str) -> Result<()> {
        if let Some(handle) = self.containers.remove(task_id) {
            self.docker.remove_container(
                &handle.id,
                Some(RemoveContainerOptions { force: true, ..Default::default() }),
            ).await?;

            // Cleanup socket
            let socket_path = format!("/tmp/phantom-{}.sock", task_id);
            let _ = std::fs::remove_file(&socket_path);
        }
        Ok(())
    }
}
```

### Configuration Schema

Add to `agents.toml`:

```toml
[container]
enabled = true
runtime = "runc"  # or "runsc" for gVisor

[container.resources]
memory = "4g"
cpus = 2
max_pids = 256
timeout_secs = 3600  # 1 hour max runtime

[container.network]
mode = "bridge"
egress_whitelist = [
    "api.anthropic.com:443",
    "api.openai.com:443",
    "github.com:443",
    "*.githubusercontent.com:443",
]

[[agents]]
id = "claude-code"
container_image = "ghcr.io/phantom-harness/agent-claude:latest"
container_override = { memory = "8g", cpus = 4 }  # Claude needs more resources
# ... rest of agent config
```

### Fallback Mode

For environments without Docker (or user preference), maintain the current subprocess mode:

```rust
pub enum AgentRuntime {
    /// Direct subprocess (current behavior)
    Subprocess(AgentProcessClient),
    /// Docker container with security isolation
    Container(ContainerHandle),
}

impl AgentRuntime {
    pub async fn send_message(&mut self, msg: &str) -> Result<()> {
        match self {
            Self::Subprocess(client) => client.send(msg).await,
            Self::Container(handle) => handle.socket.write_all(msg.as_bytes()).await?,
        }
        Ok(())
    }
}
```

Selection logic:
```rust
fn select_runtime(settings: &Settings, agent: &AgentConfig) -> RuntimeMode {
    if !settings.container_isolation_enabled {
        return RuntimeMode::Subprocess;
    }
    if !is_docker_available() {
        log::warn!("Docker not available, falling back to subprocess mode");
        return RuntimeMode::Subprocess;
    }
    if agent.container_image.is_none() {
        return RuntimeMode::Subprocess;
    }
    RuntimeMode::Container
}
```

## Threat Model

### In Scope

| Threat | Mitigation |
|--------|------------|
| Agent escapes to host filesystem | Bind mounts only worktree, read-only credentials |
| Agent exfiltrates credentials | Bridge validates outbound messages, no raw key access |
| Agent consumes excessive resources | cgroups limits on CPU, memory, PIDs |
| Agent makes unauthorized network requests | Egress whitelist via iptables |
| Agent privilege escalation | Rootless mode, dropped capabilities, seccomp |
| Malicious code execution | User namespaces, gVisor kernel sandboxing |

### Out of Scope

| Threat | Reason |
|--------|--------|
| Host kernel exploits | Requires gVisor/Kata for full mitigation |
| Docker daemon compromise | Trust boundary—daemon runs as root |
| Side-channel attacks | Not addressed by containerization |
| Supply chain attacks on agent CLIs | Separate concern (image scanning) |

## Implementation Phases

### Phase 1: Foundation (Week 1-2)
- [ ] Add `bollard` dependency for Docker API
- [ ] Create `ContainerManager` with basic create/start/stop/remove
- [ ] Build `phantom-agent-base` Docker image
- [ ] Implement Unix socket communication layer

### Phase 2: Agent Images (Week 2-3)
- [ ] Build agent-specific images (claude, codex, droid)
- [ ] Create `harness-bridge` binary
- [ ] Implement credential mounting (OAuth files)
- [ ] Test basic agent operations in containers

### Phase 3: Security Hardening (Week 3-4)
- [ ] Add seccomp profile
- [ ] Implement resource limits
- [ ] Add network egress filtering
- [ ] Test with rootless Docker
- [ ] Optional: gVisor runtime support

### Phase 4: Integration (Week 4-5)
- [ ] Add container settings to UI
- [ ] Implement fallback mode (container → subprocess)
- [ ] Add container status monitoring
- [ ] Handle container recovery on app restart
- [ ] Documentation and testing

### Phase 5: Production Hardening (Week 5-6)
- [ ] Container image CI/CD pipeline
- [ ] Image vulnerability scanning
- [ ] Performance benchmarking
- [ ] Chaos testing (kill containers, network partition)
- [ ] User documentation

## Open Questions

1. **Image Distribution**: Build locally vs pull from registry (ghcr.io)?
2. **Rootless Docker**: Require rootless mode or make it optional?
3. **gVisor**: Include as default or optional security enhancement?
4. **Windows Support**: Use Windows containers or WSL2 Docker?
5. **Container Reuse**: One container per task or pool of containers?
6. **DMR Integration Priority**: Implement container isolation first, or DMR support first?
7. **DMR Model Selection**: Let users pick models in UI, or auto-detect from `docker model ls`?
8. **Hybrid Mode**: Allow some agents to use cloud APIs while others use DMR?

---

## Alternative: Docker Model Runner Integration

Docker Model Runner (DMR) is Docker's native solution for running LLMs locally. It provides an OpenAI-compatible API at `http://localhost:12434/v1`, enabling fully local inference without cloud API costs.

### How DMR Differs from Container Isolation

| Aspect | Container Isolation (above) | Docker Model Runner |
|--------|----------------------------|---------------------|
| **Purpose** | Sandbox agent CLIs | Run local LLMs |
| **What runs** | claude/codex/droid CLIs | llama.cpp inference engine |
| **API calls** | Still go to cloud APIs | Stay completely local |
| **Cost** | Per-token API pricing | $0 after model download |
| **Privacy** | Better (sandboxed agent) | Best (no external calls) |

### DMR Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Docker Desktop                            │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │              Docker Model Runner                         │    │
│  │  • llama.cpp inference engine (native host process)     │    │
│  │  • Models cached as OCI artifacts                       │    │
│  │  • OpenAI-compatible API: localhost:12434/v1            │    │
│  └─────────────────────────────────────────────────────────┘    │
│                              ▲                                   │
│                    OpenAI API format                            │
│                              │                                   │
└──────────────────────────────┼──────────────────────────────────┘
                               │
┌──────────────────────────────┼──────────────────────────────────┐
│                    Phantom Harness                               │
│                              │                                   │
│  ┌───────────────────────────┴───────────────────────────┐      │
│  │               Agent (OpenCode, Claude, etc.)           │      │
│  │  Configured to use: http://localhost:12434/v1         │      │
│  │  Instead of: https://api.openai.com/v1                │      │
│  └────────────────────────────────────────────────────────┘     │
└─────────────────────────────────────────────────────────────────┘
```

### Supported Inference Engines

| Engine | Platform | Use Case | Model Format |
|--------|----------|----------|--------------|
| **llama.cpp** | All (default) | Local dev, resource efficiency | GGUF quantized |
| **vLLM** | Linux + NVIDIA | Production, high throughput | Safetensors |
| **Diffusers** | Linux + NVIDIA | Image generation | Various |

### Compatible Agents

These agents support custom OpenAI-compatible endpoints:

| Agent | Configuration |
|-------|---------------|
| **OpenCode** | `opencode.json` → point to `localhost:12434` |
| **Claude Code** | `ANTHROPIC_BASE_URL` or custom provider config |
| **Aider** | `--openai-api-base http://localhost:12434/v1` |
| **Continue** | Provider config in `config.json` |

### Combined Architecture: Container Isolation + DMR

For maximum security AND privacy, combine both:

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Phantom Harness (Host)                           │
│                                                                          │
│  ┌────────────────────────────────────────────────────────────────┐     │
│  │                   Docker Model Runner                           │     │
│  │           localhost:12434/v1 (OpenAI-compatible)               │     │
│  └────────────────────────────────────────────────────────────────┘     │
│                                ▲                                         │
│                                │ API calls (local only)                 │
│                                │                                         │
│  ┌─────────────────────────────┴──────────────────────────────────┐     │
│  │                    Docker Container (sandboxed)                 │     │
│  │  ┌──────────────────────────────────────────────────────────┐  │     │
│  │  │  Agent CLI (opencode/aider/etc.)                          │  │     │
│  │  │  • Configured to use localhost:12434                      │  │     │
│  │  │  • NO external network access                             │  │     │
│  │  │  • Worktree mounted at /workspace                         │  │     │
│  │  └──────────────────────────────────────────────────────────┘  │     │
│  └────────────────────────────────────────────────────────────────┘     │
│                                │                                         │
│                    Unix Socket to host                                  │
└────────────────────────────────┼────────────────────────────────────────┘
```

**Benefits of combined approach:**
- **Zero cloud costs**: All inference runs locally via DMR
- **Zero network egress**: Container has no external network access
- **Full sandbox**: Agent can't escape to host filesystem
- **Data sovereignty**: Code never leaves your machine

### DMR Configuration for Phantom Harness

Add to `agents.toml`:

```toml
[model_runner]
enabled = true
endpoint = "http://localhost:12434/v1"
# Models available via: docker model ls
default_model = "ai/qwen2.5-coder:14b"

# Per-agent DMR configuration
[[agents]]
id = "opencode"
# ... existing config ...
model_runner_enabled = true
model_runner_models = [
    "ai/qwen2.5-coder:14b",
    "ai/qwen2.5-coder:32b",
    "ai/deepseek-coder-v2:16b",
    "ai/codellama:34b",
]
```

### Runtime Selection Matrix

```rust
pub enum AgentRuntime {
    /// Direct subprocess with worktree (current default)
    Subprocess(AgentProcessClient),
    /// Docker container isolation (cloud APIs)
    Container(ContainerHandle),
    /// Docker container + Model Runner (fully local)
    ContainerWithDMR(ContainerHandle),
}

fn select_runtime(settings: &Settings, agent: &AgentConfig) -> RuntimeMode {
    match (settings.container_enabled, settings.model_runner_enabled) {
        (false, false) => RuntimeMode::Subprocess,           // Current behavior
        (true, false) => RuntimeMode::Container,             // Sandboxed + cloud APIs
        (false, true) => RuntimeMode::SubprocessWithDMR,     // Local LLM, no sandbox
        (true, true) => RuntimeMode::ContainerWithDMR,       // Maximum isolation
    }
}
```

### DMR Network Configuration

When using DMR with containerized agents, the container needs access to the host's DMR endpoint:

```yaml
# Container config for DMR access
extra_hosts:
  - "host.docker.internal:host-gateway"
environment:
  - OPENAI_API_BASE=http://host.docker.internal:12434/v1
  - OPENAI_API_KEY=not-needed  # DMR doesn't require auth
```

### Hardware Acceleration

DMR automatically detects and uses available acceleration:

| Hardware | Backend | Notes |
|----------|---------|-------|
| Apple Silicon | Metal API | Automatic, no config needed |
| NVIDIA GPU | CUDA | Requires nvidia-container-runtime |
| AMD/Intel GPU | Vulkan | Supported since Oct 2025 |
| CPU only | llama.cpp | Works everywhere, slower |

### Implementation Notes

1. **DMR is optional**: Not all users have Docker Desktop or sufficient hardware
2. **Model quality varies**: Local models may not match Claude/GPT quality
3. **Memory requirements**: 14B models need ~16GB RAM, 32B need ~32GB
4. **First pull is slow**: Models are large (8-20GB), cached after download

---

## References

- [Docker Security Best Practices](https://docs.docker.com/engine/security/)
- [gVisor Documentation](https://gvisor.dev/docs/)
- [Bollard - Rust Docker SDK](https://github.com/fussybeaver/bollard)
- [OWASP Docker Security Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Docker_Security_Cheat_Sheet.html)
- [Container Security Best Practices 2025](https://cloudnativenow.com/topics/cloudnativedevelopment/docker/docker-security-in-2025-best-practices-to-protect-your-containers-from-cyberthreats/)
- [Docker Model Runner Documentation](https://docs.docker.com/ai/model-runner/)
- [Docker Model Runner Product Page](https://www.docker.com/products/model-runner/)
- [Claude Code with Docker Model Runner](https://www.docker.com/blog/run-claude-code-locally-docker-model-runner/)
- [OpenCode + Docker Model Runner](https://www.docker.com/blog/opencode-docker-model-runner-private-ai-coding/)
