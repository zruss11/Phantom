# Agent Registry (CLI Launch Templates)

This file documents the CLI agent launch templates used by the backend and how
the config is interpreted at runtime.

## Config Location
- `backend/config/agents.toml`

## Template Variables
These variables are replaced at spawn time:
- `{worktree}`: absolute path to the session worktree
- `{session_id}`: session UUID
- `{agent_name}`: auto-generated display name

## Fields
- `id`: stable identifier stored in SQLite.
- `display_name`: user-facing name for the UI.
- `command`: executable to spawn.
- `args`: argv array; may include template variables. The backend appends
  `--output-format stream-json` if not already provided.
- `required_env`: env var names required to start the agent.
- `cwd_mode`: `"process"` uses the spawn cwd as the worktree path.
- `supports_plan`: if false, plan mode will be disabled.
- `default_plan_model`: per-agent default planner model.
- `default_exec_model`: per-agent default execution model.
- `model_source`: how models are loaded for this agent:
  - `config`: use static list from `models` field (Claude Code, Factory Droid)
  - `app-server`: fetch dynamically via agent's model/list API (Codex)
- `models`: list of model values for dropdowns (used when `model_source = "config"`).

## Validation Rules (backend)
- If `required_env` is missing, session creation fails with actionable error.
- If `command` is not found, session creation fails with a fix hint.
- If `supports_plan` is false, disable plan mode in UI for that agent.
- If `model_source` is `app-server`, request model list dynamically; if `config`, use the `models` array.
- Max parallel sessions enforced via `max_parallel`.

## Notes
- Users can override command/args in Settings; overrides are stored separately.
- `cwd_mode = "process"` means spawn the subprocess with its cwd set to the
  worktree path. Some agents also accept `--cwd` in args.
- If the CLI exposes model config options at session setup, prefer those
  for populating the plan/execution model dropdowns.
