---
status: complete
priority: p3
issue_id: "013"
tags: [code-review, rust, claude, teammate-controller, cleanup]
dependencies: []
---

# Clean Up Teammate Controller Warnings / Dead Code

## Problem Statement

The PR introduces a handful of unused fields/methods that generate warnings in `cargo test`. This is not blocking, but cleaning it up will keep the build output signal high.

## Findings

- `SessionBackend::ClaudeTeams` stores `team_name` and `pid`, but they are never read (warning in `cargo test` output).
- `ProcessManager::pid` and `kill_all` are unused.
- `ClaudeCtrlConfig.port` is unused.
- `db::update_task_claude_team_agent` is unused.

## Proposed Solutions

### Option 1: Remove Unused Fields/Methods (Recommended)

**Approach:**
- Remove unused fields from structs/enums or start using them intentionally (e.g., expose pid in debug UI, use port for introspection).

**Pros:**
- Clean build output.
- Less confusing API surface.

**Cons:**
- Minor churn.

**Effort:** Small

**Risk:** Low

---

### Option 2: Add Minimal Uses (Telemetry/Debug)

**Approach:**
- If these are meant for future work, add a minimal read path (e.g., include `pid` in a debug endpoint).

**Pros:**
- Preserves intended API.

**Cons:**
- Risk of half-baked UX/API surface.

**Effort:** Small/Medium

**Risk:** Low

## Recommended Action

Decide whether unused fields/methods are intentional future hooks. If not, remove them to keep `cargo test` output clean. If yes, add a minimal, explicit use (e.g. include PID/port in a debug-only endpoint) and document why they exist.

## Technical Details

Affected files:
- `src-tauri/src/main.rs`
- `src-tauri/src/claude_controller/process.rs`
- `src-tauri/src/claude_controller_api.rs`
- `src-tauri/src/db.rs`

## Resources

- PR: #30 "[codex] Claude teammate controller integration"

## Acceptance Criteria

- [x] `cd src-tauri && cargo test` emits no new warnings from teammate-controller code paths.

## Work Log

### 2026-02-09 - Approved for Work

**By:** Codex

**Actions:**
- Approved during triage (pending -> ready).
- Deferred exact approach (remove vs minimal use) to the implementation step.

### 2026-02-09 - Initial Discovery

**By:** Codex

**Actions:**
- Ran `cd src-tauri && cargo test` and recorded warnings introduced by PR #30.

### 2026-02-09 - Completed

**By:** Codex

**Actions:**
- Removed or accounted for unused teammate-controller fields/methods (e.g., removed unused process PID accessor; ensured controller config fields are used where appropriate).
- Silenced intentionally-unused DB helper with `#[allow(dead_code)]` (consistent with other task helper accessors).
- Verified `cd src-tauri && cargo test` output no longer includes teammate-controller dead-code warnings (remaining `objc` duplicate runtime notes are unrelated).
