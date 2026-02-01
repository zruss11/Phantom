---
status: complete
priority: p2
issue_id: "005"
tags: [pr-comment, rust, git]
dependencies: []
---

# Problem Statement

Commit subjects can include `|`, breaking `git log` parsing that splits on `|`.

# Findings

PR comment recommends using a non-ambiguous separator like `%x1f` and splitting on unit separator.

# Proposed Solutions

1. Change git log format to use `%x1f` and split on `\x1f`.
2. Use `--pretty=format:%x1f` with NUL-like separation and parse accordingly.

# Recommended Action

Use `%x1f` in git log format and split lines on `\x1f` when constructing `ReviewCommit`.

# Acceptance Criteria

- Commit parsing is robust even when subject contains `|`.
- Both base-ref and fallback log commands use the same separator.

# Work Log

### 2026-02-01 - Created from PR review

**By:** Claude Code

**Actions:**
- Logged unresolved PR comment as a tracked todo.

**Learnings:**
- Use an unambiguous separator for git log parsing.

### 2026-02-01 - Implemented separator change

**By:** Claude Code

**Actions:**
- Updated git log format to use `%x1f` and split on `\\x1f` in `src-tauri/src/main.rs`.

**Learnings:**
- Unit separator avoids subject parsing issues.
