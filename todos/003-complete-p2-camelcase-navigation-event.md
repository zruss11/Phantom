---
status: complete
priority: p2
issue_id: "003"
tags: [pr-comment, frontend, events]
dependencies: []
---

# Problem Statement

Frontend event name `phantom:navigate` does not follow CamelCase conventions.

# Findings

PR comment requests renaming the event and its emitter to CamelCase to match project standards.

# Proposed Solutions

1. Rename to `PhantomNavigate` and update dispatchers/listeners.
2. Rename to `NavigatePhantom` and update dispatchers/listeners.

# Recommended Action

Rename to `PhantomNavigate` and update all dispatch sites.

# Acceptance Criteria

- Event name is CamelCase in listener and emitter.
- Navigation still triggers Review Center initialization.

# Work Log

### 2026-02-01 - Created from PR review

**By:** Claude Code

**Actions:**
- Logged unresolved PR comment as a tracked todo.

**Learnings:**
- Frontend event names must be CamelCase.

### 2026-02-01 - Renamed event

**By:** Claude Code

**Actions:**
- Renamed navigation event to `PhantomNavigate` in `gui/js/gui.js` and `gui/js/review.js`.

**Learnings:**
- Keep event names CamelCase for consistency.
