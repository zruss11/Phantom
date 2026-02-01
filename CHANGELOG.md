# Changelog

## v1.0.2 - 2026-02-01

### Bugfixes
- Fixed database initialization crash on PRAGMA journal_mode returning results.

## v1.0.1 - 2026-01-29

### Features
- Codex: load prompt commands for slash autocomplete.
- Codex: show a helpful message when auth expires.
- Claude Code: auto-refresh OAuth tokens before expiry.
- Claude Code: display plan content from ExitPlanMode.
- Claude Code: load plugin skills for the Skills page and Skill Tree.
- UI: redesign chat header for a sleeker aesthetic.
- UI: add plan delegation button to send plans to agents.
- UI: add usage warning badge for near-limit agents.
- UI: move task pill into the status dropdown.
- UI: add Create PR button to the chat log header.
- UI: add AI code review dropdown in the chat log.
- UI: add session cost display to the chat status bar.
- UI: add floating progress pill and unify message widths.
- UI: revise PR prompt and add beta tags.
- Tasks: show git diff stats in chat and tasks list.
- Tasks: warn before deleting tasks with uncommitted worktree changes.
- Tasks: implement soft stop for task generation.
- Discord: require project allowlist for actions.

### Fixes
- UI: reduce progress pill gap for tighter spacing.
- UI: improve progress pill task tracking and positioning.
- Plan mode: respond to user input without JSON-RPC header.
- Plan mode: accept tool/requestUserInput server requests.
- Plan mode: persist request IDs for plan input.
- Plan mode: resolve parsing and Discord import issues.
- Plan mode: wire plan mode and Discord input UI.
- Discord: log and relax request IDs for user input responses.
- Discord: clear plan input status after button responses.

### Refactors
- UI: simplify permission card and input option styling.
- UI: rename Command Center to Analytics Center.
- Sessions: use Arc<Mutex<>> for shared session handles.

### Chores
- Logging: trace user input response flow.

### Notes
- Reverted the diff stats + DIFFS column change due to regressions.
