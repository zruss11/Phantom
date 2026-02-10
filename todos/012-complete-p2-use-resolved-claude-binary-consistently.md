---
status: complete
priority: p2
issue_id: "012"
tags: [code-review, rust, claude, teammate-controller, reliability]
dependencies: []
---

# Use Resolved `claude` Binary Consistently (Reconnect Path)

## Problem Statement

The teammate controller init path uses `resolve_claude_command()` to find the installed Claude CLI, but the reconnect path in `start_task_internal` hard-codes `"claude"`. This can break teammate mode for users whose Claude binary is not on PATH or is at a custom location.

## Findings

- Create-session path uses `resolve_claude_command()` before `ClaudeTeamsController::init`. `src-tauri/src/main.rs:4663`.
- Reconnect path uses `"claude".to_string()` for init. `src-tauri/src/main.rs:5607`.

## Proposed Solutions

### Option 1: Reuse `resolve_claude_command()` (Recommended)

**Approach:**
- Replace hard-coded `"claude"` with `resolve_claude_command()` in the reconnect/init path.

**Pros:**
- Consistent behavior.
- Fewer environment-specific failures.

**Cons:**
- None meaningful.

**Effort:** Small

**Risk:** Low

## Recommended Action

Use `resolve_claude_command()` in the reconnect path when initializing `ClaudeTeamsController` (replace the hard-coded `"claude".to_string()`) so teammate mode works for non-PATH installs and custom locations.

## Technical Details

Affected files:
- `src-tauri/src/main.rs:5607`

## Resources

- PR: #30 "[codex] Claude teammate controller integration"

## Acceptance Criteria

- [x] Reconnect path uses the same Claude binary resolution as create-session path.
- [x] `cd src-tauri && cargo test` passes.

## Work Log

### 2026-02-09 - Approved for Work

**By:** Codex

**Actions:**
- Approved during triage (pending -> ready).
- Marked as a small consistency fix with low risk.

### 2026-02-09 - Initial Discovery

**By:** Codex

**Actions:**
- Compared init paths for teammate controller in create-session vs reconnect.

### 2026-02-09 - Completed

**By:** Codex

**Actions:**
- Updated the reconnect/init path to use `resolve_claude_command()` instead of hard-coding `"claude"`.
- Verified with `cd src-tauri && cargo test` (62 tests passing).
