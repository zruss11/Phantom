# Phantom Harness Rust Backend Outline

## Role
Tauri backend acts as the CLI client and orchestrator. It spawns local agent
subprocesses, manages worktrees, enforces command safety, and persists durable
session state for crash recovery.

## High-Level Components
- **AppState**: Shared state container (SQLite handle, task registry, config).
- **AgentManager**: Spawns and supervises agent subprocesses (max 5).
- **CliClient**: NDJSON-over-stdio adapter for CLI session IO.
- **WorktreeManager**: Creates and maps worktrees to sessions.
- **SafetyGuard**: Allow/block list enforcement for shell and git commands.
- **Store**: SQLite persistence for sessions, logs, and metadata.
- **LogWriter**: Append-only stream logs for each session.
- **ResumeManager**: Restores incomplete sessions on app start.
- **ModelResolver**: Loads available models per agent (dynamic via app-server or static from config).

## Process Model
```
Tauri (Rust)
  ├─ spawns Agent subprocess (CLI stdio)
  ├─ CliClient reads/writes stream-json events
  ├─ Store writes all events and checkpoints
  └─ UI receives streaming updates via events
```

## Session Lifecycle
- `create_session`:
  - Validate concurrency limit (max 5).
  - Generate agent name (tiny model via configured provider).
  - Create git worktree and branch.
  - Write `PLAN.md` when plan mode is enabled.
  - Persist session row + initial status.
- `start_session`:
  - Spawn CLI agent subprocess (cwd set to worktree path).
  - Stream output -> Store + LogWriter -> UI events.
- `pause_session`:
  - Send CLI interrupt/cancel (or SIGTERM if no protocol support).
- `resume_session`:
  - Resume loop from last checkpoint (Store + Log).
- `stop_session`:
  - Kill subprocess, mark session aborted.
- `archive_session`:
  - Confirm user intent (UI modal).
  - Delete worktree + logs + SQLite row.

## SQLite Schema (Sketch)
- `sessions`: id, name, agent_type, status, created_at, updated_at, worktree_path
- `messages`: id, session_id, role, content, ts
- `tool_calls`: id, session_id, name, input_json, output_json, ts
- `checkpoints`: id, session_id, iteration, state_json, ts
- `plan_files`: id, session_id, path, sha256, ts

## CLI Integration
- Implement CLI client interface (stream-json over stdio).
- Route incoming messages into Store and UI streams.
- For external agents, use per-agent launch templates (configurable).
- Model sources vary by agent:
  - **Claude Code**: Static aliases from config (`sonnet`, `opus`, `haiku`, etc.)
  - **Codex**: Dynamic via `app-server` model/list API
  - **Others**: Static from `agents.toml`
- When the user selects a model, pass the selection on the next CLI invocation.

## Command Safety
- Implement SafetyGuard in the command execution layer.
- Enforce allow/block rules from `claude-code-safety-net`.
- Deny by default on ambiguous destructive patterns.

## Tauri Commands (API Surface)
- `create_session(config)`
- `start_session(session_id)`
- `pause_session(session_id)`
- `resume_session(session_id)`
- `stop_session(session_id)`
- `archive_session(session_id)`
- `list_sessions()`
- `get_session_detail(session_id)`
- `subscribe_session(session_id)` (event stream)

## Event Stream to UI
- `session.created`
- `session.updated`
- `session.log`
- `session.completed`
- `session.error`

## Configuration
- `agents.toml` or `config.json` in app data:
  - CLI command templates per agent
  - Default model / plan model per agent
  - Environment variables (API keys)
  - Safety policy toggles
  - Max concurrency (default 5)

## Error Handling & Recovery
- Persist every tool call and output.
- Checkpoint after each tool call and message chunk.
- On app start, ResumeManager scans for `running` sessions and auto-resumes.

## Suggested File Layout
```
src-tauri/
  src/
    main.rs
    state.rs
    cli/
      client.rs
      protocol.rs
    agents/
      manager.rs
      launch.rs
      safety_guard.rs
    worktrees/
      manager.rs
    store/
      sqlite.rs
      models.rs
    events/
      bus.rs
```
