# CLAUDE.md - Phantom Harness

## Project Overview

Phantom Harness is a Tauri-based desktop application serving as a unified orchestrator for AI code agents. It provides a desktop UI to interact with multiple agents (Claude Code, Codex, Factory Droid) using their native protocols.

## API Integration

When integrating with external APIs (Sentry, Stripe, etc.), ALWAYS read the official documentation first before attempting implementation. Never guess at API formats or parameters.

## Tech Stack

- **Backend**: Rust (Edition 2021) with Tauri v2, tokio, rusqlite, serde
- **Frontend**: Vanilla HTML/CSS/JavaScript with Bootstrap 5, jQuery
- **Database**: SQLite for task/session persistence
- **Build**: Cargo + Tauri build system

## Build & Run Commands

```bash
# Development mode (hot reload)
cargo tauri dev

# Production build
cargo build --release -p phantom_harness

# Build native installers
cargo tauri build
```

## Directory Structure

```
backend/              # Shared Rust library for agent protocols
  src/cli.rs         # Agent process client (JSON-RPC over stdio)
  src/models.rs      # Model selection logic
  config/agents.toml # Agent registry configuration
  migrations/        # SQLite schema migrations

src-tauri/           # Tauri desktop app
  src/main.rs        # Main app: commands, auth, session management
  src/db.rs          # SQLite task persistence

gui/                 # Frontend
  menu.html          # Main application UI
  js/tauri-bridge.js # Tauri IPC bridge
```

## Key Configuration Files

- `backend/config/agents.toml` - Agent definitions (commands, env vars, models)
- `src-tauri/tauri.conf.json` - Tauri app config (allowlist, CSP, bundle)
- `backend/migrations/0001_init.sql` - Database schema

## Storage Locations

**Config directory** (`~/.config/phantom-harness/` Linux, `~/Library/Application Support/phantom-harness/` macOS):
- `tasks.db` - SQLite database (tasks, sessions, messages)
- `settings.json` - User preferences
- `attachments/` - Uploaded files
- `disabled-skills/<agent>/` - Skill disable markers
- `logs/` - Application logs

**User directory** (`~/phantom-harness/`):
- `workspaces/<repo>/<animal>` - Git worktrees for isolated branch work

**External agent directories** (not ours, read-only):
- `~/.codex/` - Codex auth & sessions
- `~/.claude/` - Claude auth & credentials

## Code Conventions

- **Tauri commands**: snake_case (`get_agent_models`, `start_task`)
- **Frontend events**: CamelCase (`StatusUpdate`, `SubmitTask`)
- **Session IDs**: `task-{timestamp_ms}-{uuid_prefix}`
- **Timestamps**: Unix seconds via `chrono::Utc::now().timestamp()`
- **Auth methods**: "api", "chatgpt" (Codex), "cli"/"oauth" (Claude)

## Key Tauri IPC Commands

```rust
get_agent_models(agent_id)    // Get available models for agent
create_agent_session(payload) // Create new agent session
start_task(task_id)           // Execute task prompt
codex_login() / claude_login() // OAuth authentication
check_codex_auth() / check_claude_auth() // Auth status
load_tasks() / delete_task()  // Task CRUD operations
```

## Agent Protocol Flow

1. `create_session` → spawn agent subprocess
2. `session/new` → initialize session (or `thread/start` for Codex)
3. `session/prompt` → send user prompt (array format: `[{type, text}]`)
4. Stream results via JSON-RPC notifications
5. Persist state to SQLite for crash recovery

## Model Configuration

Models are configured per-agent in `backend/config/agents.toml`:

| Agent | `model_source` | How Models are Loaded |
|-------|----------------|----------------------|
| Claude Code | `config` | Static aliases: `default`, `sonnet`, `opus`, `haiku`, `opusplan` |
| Codex | `app-server` | Dynamic via `model/list` API |
| Factory Droid | `config` | Static from config |

## Development Notes

- Agents use JSON-RPC over stdio for communication
- `AgentProcessClient` manages bidirectional message flow
- Claude Code runs with `--permission-mode bypassPermissions` (non-interactive)
- OAuth flows have 5-minute timeout
- Frontend updates are event-driven via Tauri emit

## Debugging

For debugging tasks, start by reading error messages and logs carefully, then trace the issue systematically rather than making speculative fixes.

## Language-Specific Guidelines

### Rust

When working with Rust code, prefer idiomatic error handling with Result types and use `cargo clippy` before committing changes.

## Philosophy

This codebase will outlive you. Every shortcut becomes someone else's burden. Every hack compounds into technical debt that slows the whole team down.

You are not just writing code. You are shaping the future of this project. The patterns you establish will be copied. The corners you cut will be cut again.

Fight entropy. Leave the codebase better than you found it.
