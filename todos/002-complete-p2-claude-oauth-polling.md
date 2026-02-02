---
status: complete
priority: p2
issue_id: "002"
tags: [ui, oauth, polling]
dependencies: []
---

# Prevent Overlapping Claude OAuth Polling Calls

## Problem Statement
OAuth polling uses `setInterval` with async calls that can overlap, leading to redundant requests or race conditions in the UI flow.

## Findings
- `checkClaudeAuth` is async; if it takes longer than 1s, multiple invocations can overlap.
- CodeRabbit suggests sequential polling using `setTimeout` and awaiting each call.

## Proposed Solutions
1. **Sequential polling with setTimeout** (preferred)
   - Pros: prevents overlap, simpler to reason about.
   - Cons: slightly more code.
2. **Guard with in-flight flag**
   - Pros: minimal changes.
   - Cons: still uses interval; easier to get wrong.

## Recommended Action
Replace interval-based polling with a self-scheduling async function that awaits `checkClaudeAuth` before scheduling the next tick.

## Acceptance Criteria
- No concurrent `checkClaudeAuth` calls while polling.
- OAuth success still closes modal and enables usage.

## Work Log
### 2026-02-02 - Created
**By:** Claude Code
**Actions:**
- Logged review feedback and planned fix.

### 2026-02-02 - Resolved
**By:** Claude Code
**Actions:**
- Replaced interval polling with sequential timeout polling in `gui/js/application.js`.
- Added `startClaudeOauthPolling` to ensure no overlapping `checkClaudeAuth` calls.

**Learnings:**
- Sequential polling avoids concurrent network calls during OAuth flows.
