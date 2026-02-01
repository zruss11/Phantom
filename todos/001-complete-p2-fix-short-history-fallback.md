---
status: complete
priority: p2
issue_id: "001"
tags: [pr-comment, rust, git]
dependencies: []
---

# Problem Statement

Review Center uses `HEAD~10` as a fallback base ref when `main/master` are missing. In short histories, this produces a bad revision and hides commits.

# Findings

PR thread notes that `git log HEAD~10..HEAD` fails for repos with fewer than 10 commits and results in empty commit lists.

# Proposed Solutions

1. Use the first commit hash as the fallback base.
2. Use `git log -n 10` without a range when base ref is unavailable.

# Recommended Action

Implement a safe fallback that resolves to an existing commit (first commit) or uses `git log -n 10`.

# Acceptance Criteria

- Review Center shows commits for repos with < 10 commits when `main/master` are absent.
- No `bad revision` errors when computing history.

# Work Log

### 2026-02-01 - Created from PR review

**By:** Claude Code

**Actions:**
- Logged unresolved PR comment as a tracked todo.

**Learnings:**
- Short-history repos need a safe base ref fallback.

### 2026-02-01 - Implemented fallback

**By:** Claude Code

**Actions:**
- Updated merge-base fallback to use root commit or HEAD in `src-tauri/src/main.rs`.

**Learnings:**
- Root commit is a safe default for short histories.
