---
status: complete
priority: p1
issue_id: "006"
tags: [pr-comment, frontend, lint]
dependencies: []
---

# Problem Statement

`gui/js/tauri-bridge.js` redeclares `payload` multiple times in the invoke router and uses 4-space indentation, violating lint and style rules.

# Findings

CodeRabbit reports `noRedeclare` lint warnings and notes the repo uses 2-space indentation in `gui/**/*.js`.

# Proposed Solutions

1. Rename each payload to a unique variable and normalize indentation.
2. Refactor to a helper function that extracts payload per channel.

# Recommended Action

Rename payload variables per block and reindent to 2 spaces.

# Acceptance Criteria

- No `payload` redeclaration in the invoke router.
- Indentation matches 2-space style in `gui/js/tauri-bridge.js`.

# Work Log

### 2026-02-01 - Created from PR review

**By:** Claude Code

**Actions:**
- Logged unresolved PR comment as a tracked todo.

**Learnings:**
- Keep JS indentation consistent and avoid redeclarations.

### 2026-02-01 - Implemented payload rename

**By:** Claude Code

**Actions:**
- Renamed per-block payload variables and fixed indentation in `gui/js/tauri-bridge.js`.

**Learnings:**
- Distinct variable names avoid function-scope `var` collisions.
