---
status: complete
priority: p1
issue_id: "008"
tags: [code-review, rust, claude, teammate-controller, security, filesystem]
dependencies: ["006"]
---

# Validate/Sanitize `from` When Routing Inbox Events (Prevent Path Traversal + Misrouting)

## Problem Statement

The controller poller uses `InboxMessage.from` from the controller inbox as both:
- the key for per-agent broadcast channels
- the destination inbox file name when auto-approving plan/permission requests

If `from` is not exactly the expected safe agent name, responses can be misrouted or the code can write outside the intended inbox directory via path traversal (because `PathBuf::join` accepts `..` and path separators inside the joined component). This is especially risky because `from` is external input (it comes from files written by other processes).

## Findings

- `from` is taken directly from inbox events: `src-tauri/src/claude_controller/controller.rs:80`.
- Auto-approval writes to inbox path derived from `from` without validation: `src-tauri/src/claude_controller/controller.rs:101` and `src-tauri/src/claude_controller/controller.rs:135`.
- Inbox path builder joins `agent_name` directly into the filesystem path: `src-tauri/src/claude_controller/paths.rs:23`.
- The REST API validates names (`src-tauri/src/claude_controller_api.rs:146`), but the controller poller does not.
- If Claude emits `from` as an agent id like `name@team` (unknown), the code may write approvals to a non-existent/incorrect inbox file.

## Proposed Solutions

### Option 1: Strict Validate `from` and Drop Unknown (Recommended)

**Approach:**
- Introduce a shared `validate_agent_name()` function (same rules as API).
- In the poller:
  - accept only safe names
  - ignore or log-and-skip otherwise
- Additionally, sanitize path segments for `team_name`/`agent_name` in `paths.rs` (defense in depth).

**Pros:**
- Prevents path traversal and misrouting.
- Simple and consistent.

**Cons:**
- If upstream uses a different identifier format, this will block until mapping is implemented.

**Effort:** Small

**Risk:** Low

---

### Option 2: Parse `from` into `{name, team}` and Map to Inbox File

**Approach:**
- If `from` is `name@team`, extract `name` and validate `name`.
- Confirm expected teammate protocol field semantics and adjust accordingly.

**Pros:**
- Works even if protocol uses agent ids.

**Cons:**
- Must be grounded in the actual Claude teammate inbox protocol.

**Effort:** Small/Medium

**Risk:** Medium

## Recommended Action

Treat `InboxMessage.from` as untrusted input:
- Validate/sanitize `from` with the same constraints as the REST API name validation (or parse `name@team` and validate `name`).
- Reject/log-and-skip events with invalid `from` values.
- Add defense-in-depth sanitization in `paths::inbox_path` to ensure `team_name` and `agent_name` cannot escape `~/.claude/teams/{team}/inboxes/`.

## Technical Details

Affected files:
- `src-tauri/src/claude_controller/controller.rs:79`
- `src-tauri/src/claude_controller/paths.rs:11`

## Resources

- PR: #30 "[codex] Claude teammate controller integration"

## Acceptance Criteria

- [x] `from` values containing `/`, `\\`, `..`, or other invalid characters are rejected and cannot influence filesystem paths.
- [x] Auto-approval responses always land in the intended agent inbox file.
- [x] Add unit tests for `inbox_path()` sanitization and `from` parsing/validation.

## Work Log

### 2026-02-09 - Approved for Work

**By:** Codex

**Actions:**
- Approved during triage (pending -> ready).
- Marked as dependent on fixing inbox locking first (`006`) since validation should be applied consistently across all inbox read/write paths.

### 2026-02-09 - Initial Discovery

**By:** Codex

**Actions:**
- Traced `from` usage to filesystem writes in auto-approval path.
- Confirmed `paths::inbox_path` joins unsanitized `agent_name`.

### 2026-02-09 - Completed

**By:** Codex

**Actions:**
- Added defense-in-depth name validation in `src-tauri/src/claude_controller/paths.rs` (rejects invalid team/agent names so paths cannot escape the `~/.claude/teams/...` layout).
- Treated `InboxMessage.from` as untrusted: normalized `name@team` -> `name` when team matches and skipped/logged invalid senders.
- Added unit tests covering name validation for `team_dir()` and `inbox_path()`.
- Verified with `cd src-tauri && cargo test` (59 tests passing).
