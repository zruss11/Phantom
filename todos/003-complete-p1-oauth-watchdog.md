---
status: complete
priority: p1
issue_id: "003"
tags: [oauth, process, safety]
dependencies: []
---

# Add OAuth Helper Watchdog Timeout

## Problem Statement
The Claude OAuth helper process can run indefinitely if the flow stalls, leaving orphaned processes.

## Findings
- `start_claude_oauth_internal` spawns the helper and stores the child without a timeout cleanup.
- Guidelines request a 5-minute timeout for OAuth flows.

## Proposed Solutions
1. **Spawn watchdog task (5 min)** (preferred)
   - Pros: automatic cleanup; minimal behavior change.
   - Cons: adds background task.
2. **Reuse polling completion to kill**
   - Pros: no timer.
   - Cons: still orphaned if user never completes flow.

## Recommended Action
After storing `oauth_state.url` and `oauth_state.child`, spawn a background task that waits 5 minutes and kills the child if still present.

## Acceptance Criteria
- Helper process is killed after 5 minutes if auth not completed.
- OAuth state is reset when watchdog fires.

## Work Log
### 2026-02-02 - Created
**By:** Claude Code
**Actions:**
- Logged review feedback and planned fix.

### 2026-02-02 - Resolved
**By:** Claude Code
**Actions:**
- Added a 5-minute watchdog that kills the OAuth helper and resets state in `src-tauri/src/main.rs`.
- Added a `watchdog_id` guard to avoid killing newer attempts.

**Learnings:**
- A simple monotonic id avoids stale watchdogs affecting newer OAuth runs.
