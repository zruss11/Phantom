# Claude Teammate Controller (Opt-In) Manual Verification

This checklist validates Phantom's opt-in Claude Code teammate-controller integration:

- `claude` is spawned in teammate mode via PTY.
- Messages are delivered via `~/.claude/teams/{team}/inboxes/*.json`.
- Plans/permissions are auto-approved (v1).
- Agent names are derived from task data and persisted in SQLite.
- Local REST API is reachable with bearer token auth.

## Setup

1. Ensure `claude` is installed and on PATH (or detected by Phantom).
2. Start Phantom in dev:
   - `cd src-tauri && cargo tauri dev`

## Enable Teammate Mode

1. Open settings and set `claudeIntegrationMode` to `teammate_controller`.
2. Optional env toggles:
   - `PHANTOM_CLAUDE_TEAMS=1` forces teammate mode on.
   - `PHANTOM_CLAUDE_TEAMS_NO_FALLBACK=1` disables fallback to stream-json if teammate init fails.

## Create Task + Verify Agent Naming

1. Create a new task with agent `claude-code`.
2. Verify the task record has:
   - `claudeTeamName` (default: `phantom-harness`)
   - `claudeAgentName` (derived from prompt/title + task id suffix)
3. Verify on disk:
   - `~/.claude/teams/phantom-harness/config.json` contains the member entry.
   - `~/.claude/teams/phantom-harness/inboxes/{claudeAgentName}.json` exists and is valid JSON.

## Run Task + Verify Chat Output

1. Click Start.
2. Verify:
   - Prompt is written to the agent inbox.
   - Assistant responses appear in the chat log (message-granularity streaming).
   - The task reaches `Completed` (or `Ready` if soft-stopped).

## Stop / Kill Behavior

1. Use Stop (hard stop).
2. Verify:
   - Agent process is killed.
   - Member is removed from `config.json`.
   - Task status becomes `Stopped`.

## REST API Verification

1. Ensure controller API is running (auto-starts when teammate mode enabled):
   - Default port: `43779`
   - Override: `PHANTOM_CLAUDE_CTRL_PORT`
2. Read the token from settings (`claudeControllerToken`).
3. Verify endpoints (examples use bearer token):
   - `GET /health`
   - `POST /session/init`
   - `GET /agents`
   - `POST /agents` (spawn)
   - `POST /agents/:name/messages` (send)
   - `POST /agents/:name/kill`

