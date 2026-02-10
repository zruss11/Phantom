---
status: complete
priority: p2
issue_id: "010"
tags: [code-review, rust, claude, teammate-controller, api, security]
dependencies: []
---

# Harden Controller REST API Auth (Avoid Query Tokens, Clarify CORS/Origin Model)

## Problem Statement

The controller REST API accepts the bearer token via query string and performs an origin check without setting any CORS response headers. If this API is intended for browser/WebView consumption, it likely needs explicit CORS handling and OPTIONS preflight responses. Separately, query-string tokens are easy to leak via logs, shell history, proxies, or referrers.

## Findings

- Token accepted via query string `?token=`: `src-tauri/src/claude_controller_api.rs:181`.
- Token accepted via `Authorization: Bearer ...`: `src-tauri/src/claude_controller_api.rs:71`.
- Origin is verified only when `Origin` header exists: `src-tauri/src/claude_controller_api.rs:85`.
- Responses do not set `Access-Control-Allow-Origin`, and there is no explicit OPTIONS handling.

## Proposed Solutions

### Option 1: Remove Query Token Support (Recommended)

**Approach:**
- Require `Authorization: Bearer ...` only.
- Keep query token support behind a dev-only env flag if needed.

**Pros:**
- Reduces accidental token exposure.

**Cons:**
- Slightly less convenient for curl-style usage unless documented.

**Effort:** Small

**Risk:** Low

---

### Option 2: Implement CORS for Allowed Origins

**Approach:**
- If the WebView must call this API:
  - Add `Access-Control-Allow-Origin` for allowed origins
  - Handle `OPTIONS` preflight (methods + headers)

**Pros:**
- Browser-compatible and explicit.

**Cons:**
- Need to define a secure origin policy for Tauri + localhost.

**Effort:** Medium

**Risk:** Medium

## Recommended Action

Remove (or gate behind a dev-only flag) query-string token support and require `Authorization: Bearer ...` only. Then decide explicitly whether the API is intended for browser/WebView calls:
- If yes, implement `OPTIONS` + CORS response headers for a tightly-scoped allowlist of origins.
- If no, document it as localhost-only, non-browser API and keep the origin check as a best-effort defense.

## Technical Details

Affected files:
- `src-tauri/src/claude_controller_api.rs:71`
- `src-tauri/src/claude_controller_api.rs:176`

## Resources

- PR: #30 "[codex] Claude teammate controller integration"

## Acceptance Criteria

- [x] Tokens are not accepted via query string in production mode.
- [x] If browser usage is required, CORS preflight works for allowed origins only.
- [x] `cd src-tauri && cargo test` passes.

## Work Log

### 2026-02-09 - Approved for Work

**By:** Codex

**Actions:**
- Approved during triage (pending -> ready).
- Prioritized eliminating query-token leakage risk first, then clarifying browser compatibility.

### 2026-02-09 - Initial Discovery

**By:** Codex

**Actions:**
- Reviewed auth checks and origin gating in `claude_controller_api.rs`.

### 2026-02-09 - Completed

**By:** Codex

**Actions:**
- Gated query-string token support behind `PHANTOM_CLAUDE_CTRL_ALLOW_QUERY_TOKEN=1` (default requires `Authorization: Bearer ...`).
- Added OPTIONS preflight handling and ensured early-return responses include CORS headers for allowed origins.
- Ensured `/session/shutdown` triggers controller cleanup and `/session/init` shuts down any previous controller.
- Verified with `cd src-tauri && cargo test` (62 tests passing).
