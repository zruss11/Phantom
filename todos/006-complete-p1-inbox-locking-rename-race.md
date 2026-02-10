---
status: complete
priority: p1
issue_id: "006"
tags: [code-review, rust, claude, teammate-controller, concurrency, data-integrity]
dependencies: []
---

# Fix Inbox File Locking (Rename Breaks Lock Semantics)

## Problem Statement

The teammate-controller inbox implementation uses an advisory exclusive lock on the inbox JSON file, but then writes updates by renaming a temp file over the original. On Unix, a lock held on the old inode does not protect the newly-renamed file, so concurrent writers/readers can interleave and lose messages or corrupt the JSON. This undermines the core reliability goal of the teammate-controller path.

## Findings

- `src-tauri/src/claude_controller/inbox.rs:66` writes by `rename(tmp, path)` while holding a lock obtained on the original `path` file descriptor (`try_lock_exclusive`). This means the lock does not cover the replaced file.
- Both writer and reader paths do this while holding the lock:
  - `write_inbox` reads file, appends, then `atomic_write_json` rename. (`src-tauri/src/claude_controller/inbox.rs:82`)
  - `read_unread_and_mark_read` reads file, marks messages `read=true`, then `atomic_write_json` rename. (`src-tauri/src/claude_controller/inbox.rs:112`)
- Any agent writing to the controller inbox concurrently with the controller marking-read can race and lose updates.
- JSON parsing uses `unwrap_or_default()` (`src-tauri/src/claude_controller/inbox.rs:103`, `src-tauri/src/claude_controller/inbox.rs:134`) which can silently drop the entire inbox if the file is truncated/partial.

## Proposed Solutions

### Option 1: In-Place Rewrite While Holding Lock (Recommended)

**Approach:**
- After locking the file descriptor:
  - `file.rewind()` / `seek(SeekFrom::Start(0))`
  - `file.set_len(0)`
  - write the new JSON bytes to the *same* file handle
  - `file.sync_all()`

**Pros:**
- Lock covers the actual bytes being written.
- No inode replacement surprises.
- Simple mental model.

**Cons:**
- If process crashes mid-write, file can be left partially written (mitigate with WAL-like approach or backup file).

**Effort:** Medium

**Risk:** Medium

---

### Option 2: Lock a Separate Lock File, Keep Atomic Rename

**Approach:**
- Use a dedicated lock file (`{inbox}.lock`) and always lock that.
- Continue to write via temp + rename, but ensure all readers/writers lock the lock file first.

**Pros:**
- Keeps atomic rename (no partial writes).
- Lock semantics stay consistent.

**Cons:**
- Requires strict discipline everywhere.
- Still need to handle temp file collisions.

**Effort:** Medium

**Risk:** Medium

---

### Option 3: Switch Inbox Storage to SQLite (Append-Only Table)

**Approach:**
- Store inbox messages as rows, mark read via updates.

**Pros:**
- Concurrency-safe by construction.
- Better observability and tooling.

**Cons:**
- Bigger refactor.
- Requires schema + migration and careful perf work.

**Effort:** Large

**Risk:** Medium

## Recommended Action

Implement a lock-file-based critical section (e.g. lock `{inbox}.lock`) for all inbox readers/writers, and keep atomic temp+rename writes. While doing this, stop silently swallowing JSON parse failures (`unwrap_or_default`) to avoid quietly dropping messages; treat invalid JSON as an error and surface it (or implement a safe recovery path).

## Technical Details

Affected files:
- `src-tauri/src/claude_controller/inbox.rs:14` (locking + read/write)

## Resources

- PR: #30 "[codex] Claude teammate controller integration"

## Acceptance Criteria

- [x] Concurrent agent writes + controller read/mark-read cannot lose messages.
- [x] Corrupted/truncated JSON is treated as an error (or is recoverable without silent message loss).
- [x] `cd src-tauri && cargo test` passes.
- [x] Add a unit test that simulates concurrent read/mark-read + write and asserts all messages persist.

## Work Log

### 2026-02-09 - Approved for Work

**By:** Codex

**Actions:**
- Approved during triage (pending -> ready).
- Prioritized lock semantics correctness over partial-write risk by keeping atomic rename and moving the lock to a separate lock file.

### 2026-02-09 - Initial Discovery

**By:** Codex

**Actions:**
- Identified rename-under-lock issue in `src-tauri/src/claude_controller/inbox.rs:66`.
- Traced both write path and mark-read path to the same atomic rename behavior.

**Learnings:**
- Advisory locks are tied to the file/inode; replacing the file via rename invalidates the locking assumption.

### 2026-02-09 - Completed

**By:** Codex

**Actions:**
- Implemented lock-file-based critical section (`*.json.lock`) for all inbox read/write paths while keeping temp+rename writes.
- Added non-silent corruption recovery: invalid JSON is backed up to `*.json.corrupt-<timestamp>` and logged.
- Added concurrency unit tests (`test_concurrent_writes_do_not_lose_messages`, `test_mark_read_is_atomic_under_lock`).
- Verified with `cd src-tauri && cargo test` (59 tests passing).
