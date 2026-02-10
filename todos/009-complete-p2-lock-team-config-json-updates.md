---
status: complete
priority: p2
issue_id: "009"
tags: [code-review, rust, claude, teammate-controller, concurrency]
dependencies: ["006"]
---

# Prevent Team `config.json` Update Races (Add/Remove Member)

## Problem Statement

Team membership updates (`add_member`/`remove_member`) read-modify-write `~/.claude/teams/{team}/config.json` with no file locking. Concurrent updates can clobber each other (lost members, stale values). This is plausible if multiple controller instances exist or if external tooling edits the file.

## Findings

- `add_member` reads config, mutates in memory, then writes the entire file. `src-tauri/src/claude_controller/team.rs:87`.
- `remove_member` does the same. `src-tauri/src/claude_controller/team.rs:94`.
- Writes are atomic rename, but without a lock this is last-writer-wins.

## Proposed Solutions

### Option 1: Add a Dedicated Lock File (Recommended)

**Approach:**
- Use `fs2` to lock `config.json.lock` for the entire read-modify-write cycle.

**Pros:**
- Minimal change; fixes lost update risk.

**Cons:**
- Requires everyone touching config.json to respect the lock.

**Effort:** Small

**Risk:** Low

---

### Option 2: Store Membership in Per-Member Files

**Approach:**
- Move `members` to a directory of files, one per agent, and rebuild in memory.

**Pros:**
- Avoids whole-file rewrite.

**Cons:**
- Protocol compatibility risk (Claude expects config.json format).

**Effort:** Medium

**Risk:** Medium

## Recommended Action

Add file locking around the read-modify-write of `config.json` (prefer a dedicated `config.json.lock` file using `fs2`) so concurrent `add_member`/`remove_member` operations cannot lose updates. Keep the existing atomic temp+rename write once locking is in place.

## Technical Details

Affected files:
- `src-tauri/src/claude_controller/team.rs:87`

## Resources

- PR: #30 "[codex] Claude teammate controller integration"

## Acceptance Criteria

- [x] Concurrent add/remove operations cannot lose updates.
- [x] `cd src-tauri && cargo test` passes.

## Work Log

### 2026-02-09 - Approved for Work

**By:** Codex

**Actions:**
- Approved during triage (pending -> ready).
- Kept dependency on `006` so the project adopts a consistent lock-file strategy for both inbox + config writes.

### 2026-02-09 - Initial Discovery

**By:** Codex

**Actions:**
- Identified read-modify-write without locking for team config updates.

### 2026-02-09 - Completed

**By:** Codex

**Actions:**
- Added `config.json.lock`-based locking around team config read-modify-write updates (`add_member`/`remove_member`) to prevent lost updates under concurrency.
- Verified with `cd src-tauri && cargo test` (62 tests passing).
