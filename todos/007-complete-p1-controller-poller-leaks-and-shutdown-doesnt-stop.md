---
status: complete
priority: p1
issue_id: "007"
tags: [code-review, rust, claude, teammate-controller, reliability, lifecycle]
dependencies: []
---

# Stop Controller Poller and Clean Up Processes on Shutdown

## Problem Statement

The teammate controller starts a background poller task that runs forever and has no cancellation path. Dropping the controller (or calling the REST API `/session/shutdown`) does not stop polling, does not stop auto-approving, and does not clean up agent processes. This can leak resources and keep unexpected background behavior running after the user believes the controller is stopped.

## Findings

- Poller is a `tokio::spawn` with an infinite loop and no exit condition. `src-tauri/src/claude_controller/controller.rs:57`.
- Poller is started during controller init and is only gated by `poller_started`; there is no `shutdown()` method. `src-tauri/src/claude_controller/controller.rs:53`.
- REST API `/session/shutdown` drops the controller by setting the slot to `None`, but does not stop poller tasks already spawned nor kill agents. `src-tauri/src/claude_controller_api.rs:228`.
- `ProcessManager` has `kill_all()` but it is unused. `src-tauri/src/claude_controller/process.rs:156`.

## Proposed Solutions

### Option 1: Add CancellationToken + JoinHandle Tracking (Recommended)

**Approach:**
- Add a `CancellationToken` (or `watch::Receiver<bool>`) stored in `ClaudeTeamsController`.
- Store the poller `JoinHandle`.
- Implement `ClaudeTeamsController::shutdown_all()`:
  - cancel token
  - abort/join poller
  - `kill_all()` agents
  - remove members from team config (best-effort)
- Have API `/session/shutdown` call `shutdown_all()` before clearing the slot.

**Pros:**
- Correct lifecycle semantics.
- Predictable cleanup.

**Cons:**
- Requires plumbing shutdown through both Tauri usage and API.

**Effort:** Medium

**Risk:** Low

---

### Option 2: Make Poller Owned by Controller Slot (No Detached Task)

**Approach:**
- Avoid detached `tokio::spawn`; run poll loop in a task owned by the API/service and stop it when controller slot is cleared.

**Pros:**
- No hidden background task.

**Cons:**
- Requires more structural change across the API wiring.

**Effort:** Medium

**Risk:** Medium

## Recommended Action

Add an explicit shutdown path for the teammate controller:
- Track the poller `JoinHandle` and a cancellation signal (e.g. `CancellationToken`).
- Implement `ClaudeTeamsController::shutdown_all()` that cancels the poller, waits/aborts it, and kills all tracked agent processes (and best-effort removes members from `config.json`).
- Call that shutdown method from the REST API `/session/shutdown` before dropping the controller slot.

## Technical Details

Affected files:
- `src-tauri/src/claude_controller/controller.rs:57`
- `src-tauri/src/claude_controller_api.rs:212`
- `src-tauri/src/claude_controller/process.rs:35`

## Resources

- PR: #30 "[codex] Claude teammate controller integration"

## Acceptance Criteria

- [x] `/session/shutdown` stops polling within 1s and no longer auto-approves anything.
- [x] All controller-spawned agent processes are terminated (or explicitly left running with clear docs).
- [x] Team config members are cleaned up (or explicitly left with clear docs).
- [x] `cd src-tauri && cargo test` passes.

## Work Log

### 2026-02-09 - Approved for Work

**By:** Codex

**Actions:**
- Approved during triage (pending -> ready).
- Chose an explicit lifecycle API (`shutdown_all`) to prevent detached background tasks and process leaks.

### 2026-02-09 - Initial Discovery

**By:** Codex

**Actions:**
- Found poller is a detached infinite loop (`src-tauri/src/claude_controller/controller.rs:67`).
- Verified API shutdown path does not call any cleanup (`src-tauri/src/claude_controller_api.rs:228`).

### 2026-02-09 - Completed

**By:** Codex

**Actions:**
- Implemented `ClaudeTeamsController::shutdown_all()` that cancels the poller, aborts the join handle, kills all tracked agent processes, and best-effort removes members from `config.json`.
- Updated the REST API `/session/shutdown` and `/session/init` to call shutdown before dropping/replacing the controller.
- Verified with `cd src-tauri && cargo test` (59 tests passing).
