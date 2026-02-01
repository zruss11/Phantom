---
status: complete
priority: p2
issue_id: "002"
tags: [pr-comment, frontend, review-center]
dependencies: []
---

# Problem Statement

Mock tasks use `ID` (uppercase), so code referencing `task.id` breaks in Review Center.

# Findings

PR comment suggests normalizing task IDs with a helper to keep browser and mock modes consistent.

# Proposed Solutions

1. Add `getTaskId(task)` helper and use it across selection/lookup.
2. Normalize tasks on load by mapping to a common `id` field.

# Recommended Action

Add a small helper `getTaskId` and use it wherever task IDs are read.

# Acceptance Criteria

- Mock tasks are selectable and render in Review Center.
- No `undefined` task IDs when building dropdown items or lookups.

# Work Log

### 2026-02-01 - Created from PR review

**By:** Claude Code

**Actions:**
- Logged unresolved PR comment as a tracked todo.

**Learnings:**
- Normalize task IDs across mock and browser modes.

### 2026-02-01 - Implemented normalization

**By:** Claude Code

**Actions:**
- Added `getTaskId` helper and used it across task selection/lookup in `gui/js/review.js`.

**Learnings:**
- Normalizing IDs avoids mock/browser mismatches.
