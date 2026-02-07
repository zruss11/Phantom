# CLAUDE.md - Phantom Harness

## Project Overview

Phantom Harness is a Tauri-based desktop application serving as a unified orchestrator for AI code agents. It provides a desktop UI to interact with multiple agents (Claude Code, Codex, Factory Droid) using their native protocols.

## API Integration

When integrating with external APIs (Sentry, Stripe, etc.), ALWAYS read the official documentation first before attempting implementation. Never guess at API formats or parameters.

## Tech Stack

- **Backend**: Rust (Edition 2021) with Tauri v2, tokio, rusqlite, serde
- **Frontend**: Vanilla HTML/CSS/JavaScript with Bootstrap 5, jQuery
- **Database**: SQLite for task/session persistence (tables: tasks, sessions, messages, meeting_sessions, meeting_segments)
- **Audio**: cpal (mic capture), whisper-rs (local transcription), rubato (resampling)
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
  src/whisper_model.rs   # Whisper model download/management
  src/audio_capture.rs   # Mic + system audio capture (cpal, screencapturekit)
  src/meeting_notes.rs   # Meeting session manager + whisper inference

gui/                 # Frontend
  menu.html          # Main application UI
  js/tauri-bridge.js # Tauri IPC bridge
  js/meeting-notes.js # Meeting Notes tab frontend logic
```

## Key Configuration Files

- `backend/config/agents.toml` - Agent definitions (commands, env vars, models)
- `src-tauri/tauri.conf.json` - Tauri app config (allowlist, CSP, bundle)
- `backend/migrations/0001_init.sql` - Database schema
- `src-tauri/Info.plist` - macOS privacy permission strings (mic, screen capture)

## Storage Locations

**Config directory** (`~/.config/phantom-harness/` Linux, `~/Library/Application Support/phantom-harness/` macOS):
- `tasks.db` - SQLite database (tasks, sessions, messages, meeting transcriptions)
- `settings.json` - User preferences
- `attachments/` - Uploaded files
- `disabled-skills/<agent>/` - Skill disable markers
- `logs/` - Application logs
- `models/whisper/ggml-small.bin` - Whisper model (~466MB, downloaded on demand)

**User directory** (`~/phantom-harness/`):
- `workspaces/<repo>/<animal>` - Git worktrees for isolated branch work

**External agent directories** (not ours, read-only):
- `~/.codex/` - Codex auth & sessions
- `~/.claude/` - Claude auth & credentials

## Code Conventions

- **Tauri commands**: snake_case (`get_agent_models`, `start_task`)
- **Frontend events**: CamelCase (`StatusUpdate`, `SubmitTask`)
- **Session IDs**: `task-{timestamp_ms}-{uuid_prefix}` (agent tasks), `meeting-{timestamp_ms}-{uuid_prefix}` (meeting sessions)
- **Timestamps**: Unix seconds via `chrono::Utc::now().timestamp()`
- **Auth methods**: "api", "chatgpt" (Codex), "cli"/"oauth" (Claude)

## Key Tauri IPC Commands

```rust
// Agent tasks
get_agent_models(agent_id)    // Get available models for agent
create_agent_session(payload) // Create new agent session
start_task(task_id)           // Execute task prompt
codex_login() / claude_login() // OAuth authentication
check_codex_auth() / check_claude_auth() // Auth status
load_tasks() / delete_task()  // Task CRUD operations

// Whisper model management
check_whisper_model()         // Check if model is downloaded
download_whisper_model()      // Download with WhisperModelProgress events
delete_whisper_model()        // Remove downloaded model

// Meeting notes / transcription
meeting_start(title?, capture_system?)  // Start recording session
meeting_pause() / meeting_resume()      // Pause/resume recording
meeting_stop()                          // Stop and finalize session
meeting_state()                         // Get current session state
meeting_list_sessions()                 // List all past sessions
meeting_get_transcript(session_id)      // Get segments for a session
meeting_delete_session(session_id)      // Delete session + segments
meeting_export_transcript(session_id, format)  // Export as txt/md/json
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

## Tauri Events

| Event | Payload | Source |
|-------|---------|--------|
| `TranscriptionSegment` | `{ id, session_id, text, start_ms, end_ms }` | meeting_notes.rs |
| `TranscriptionStatus` | `{ state: "recording"\|"paused"\|"idle", session_id }` | meeting_notes.rs |
| `WhisperModelProgress` | `{ downloaded: u64, total: u64 }` | whisper_model.rs |

## Audio Pipeline Architecture

1. cpal callback → `Mutex<Vec<f32>>` shared buffer (non-blocking `try_lock`)
2. Combiner thread polls buffer every 100ms → drains `chunk_duration_secs` worth of samples
3. `resample_to_16k()` via rubato `SincFixedIn` → 16 kHz mono f32 PCM
4. `mpsc::Sender<AudioChunk>` → inference thread
5. Whisper `state.full(params, samples)` → segments emitted to frontend + persisted to SQLite

## Development Notes

- Agents use JSON-RPC over stdio for communication
- `AgentProcessClient` manages bidirectional message flow
- Claude Code runs with `--permission-mode bypassPermissions` (non-interactive)
- OAuth flows have 5-minute timeout
- Frontend updates are event-driven via Tauri emit
- whisper-rs 0.15 API: `full_n_segments()` returns `c_int`; use `state.get_segment(i)` for segment data
- System audio capture (macOS ScreenCaptureKit) is stubbed — mic-only for now

## Debugging

For debugging tasks, start by reading error messages and logs carefully, then trace the issue systematically rather than making speculative fixes.

## Language-Specific Guidelines

### Rust

When working with Rust code, prefer idiomatic error handling with Result types and use `cargo clippy` before committing changes.

## Philosophy

This codebase will outlive you. Every shortcut becomes someone else's burden. Every hack compounds into technical debt that slows the whole team down.

You are not just writing code. You are shaping the future of this project. The patterns you establish will be copied. The corners you cut will be cut again.

Fight entropy. Leave the codebase better than you found it.
