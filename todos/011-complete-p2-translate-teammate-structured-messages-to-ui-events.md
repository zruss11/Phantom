---
status: complete
priority: p2
issue_id: "011"
tags: [code-review, rust, claude, teammate-controller, ux, streaming]
dependencies: []
---

# Translate Teammate Structured Messages into UI/Streaming Events (Avoid Raw JSON in Chat)

## Problem Statement

The teammate controller path currently only special-cases `idle_notification` and `plain_text`. Any other structured message types (plan approval requests, permission requests, etc.) will be treated as raw JSON strings and emitted into the chat stream as user-visible text. That is noisy at best, and can break UX at worst.

## Findings

- Teammate streaming logic parses `msg.text` and only handles:
  - `idle_notification` (break)
  - `plain_text` (extract text)
  Otherwise it falls back to using the raw JSON string as chat content.
  - `src-tauri/src/main.rs:6332`
- Controller poller auto-approves plan/permission requests, but other structured events could still appear, and auto-approval responses could leak as JSON.

## Proposed Solutions

### Option 1: Filter/Suppress Non-Chat Messages (Recommended)

**Approach:**
- Parse `StructuredMessage` (already defined in `src-tauri/src/claude_controller/types.rs`).
- Only emit human-readable assistant text.
- Drop/suppress auto-approval messages entirely, or map them to a status line (not chat content).

**Pros:**
- Keeps chat log clean.

**Cons:**
- Might hide useful debug info unless separately logged.

**Effort:** Medium

**Risk:** Low

---

### Option 2: Map Structured Messages to Existing Phantom Events

**Approach:**
- Convert plan/permission structured messages into existing `StreamingUpdate::*` equivalents (similar to stream-json integration).

**Pros:**
- Parity with stream-json UX.

**Cons:**
- More work; need clear spec mapping.

**Effort:** Medium/Large

**Risk:** Medium

## Recommended Action

Parse teammate messages into `StructuredMessage` and only emit user-facing assistant text to the chat log. Suppress (or map to status UI) any non-chat structured messages so raw JSON never appears in chat. If needed, add a debug log channel for structured events.

## Technical Details

Affected files:
- `src-tauri/src/main.rs:6332`
- `src-tauri/src/claude_controller/types.rs:12`

## Resources

- PR: #30 "[codex] Claude teammate controller integration"

## Acceptance Criteria

- [x] Structured teammate messages never appear as raw JSON in the chat log.
- [x] Plan/permission flows have a defined UX (suppressed, status-only, or mapped to existing events).
- [x] `cd src-tauri && cargo test` passes.

## Work Log

### 2026-02-09 - Approved for Work

**By:** Codex

**Actions:**
- Approved during triage (pending -> ready).
- Set expectation that teammate-controller UX must match (or intentionally differ from) stream-json without leaking protocol JSON into chat.

### 2026-02-09 - Initial Discovery

**By:** Codex

**Actions:**
- Reviewed teammate streaming loop and found only two message types are handled.

### 2026-02-09 - Completed

**By:** Codex

**Actions:**
- Updated teammate streaming to parse `StructuredMessage` and only emit `PlainText` content to the chat stream.
- Suppressed other structured messages (including plan/permission request/response payloads) so protocol JSON never leaks into chat.
- Verified with `cd src-tauri && cargo test` (62 tests passing).
