---
status: complete
priority: p2
issue_id: "001"
tags: [summarize, status]
dependencies: []
---

# Avoid labeling any PR link as opened

Summaries should not report that a PR was opened just because a PR URL appears in the text.

## Problem Statement

The status summarizer currently assumes any GitHub PR URL means the PR was opened, which can misrepresent work summaries when the text references existing PRs (reviewed, merged, investigated, etc.).

## Findings

- `extract_pr_summary` formats any PR URL as "PR #<n> opened".
- `generate_status` short-circuits on the first match, so any PR URL becomes the summarized outcome.
- This is a regression versus the prior LLM summary behavior.

## Proposed Solutions

### Option 1: Distinguish PR URLs from "opened" intent

**Approach:** Treat PR URLs as neutral references unless the text explicitly signals creation/opening.

**Pros:**
- Prevents misleading summaries.
- Keeps intent in the LLM summary when present.

**Cons:**
- Requires intent detection logic or additional heuristics.

**Effort:** 1-2 hours

**Risk:** Low

---

### Option 2: Only short-circuit when verb indicates opening

**Approach:** Keep PR URL detection but only promote it if verbs like "opened/created" are present.

**Pros:**
- Minimal change to existing flow.

**Cons:**
- Still heuristic; might miss nuanced phrasing.

**Effort:** 1 hour

**Risk:** Low

## Recommended Action

Update PR URL handling so summaries only state "PR #<n> opened" when the text explicitly indicates creation/opening, otherwise treat PR links as neutral references and allow the LLM summary to stand.

## Technical Details

**Affected files:**
- `src-tauri/src/summarize.rs`

## Resources

- **PR comment:** https://github.com/zruss11/Phantom/pull/10#discussion_r2746833203

## Acceptance Criteria

- [x] Summaries no longer claim a PR was opened solely due to a PR URL reference.
- [x] Explicit "opened/created" language still yields "PR #<n> opened".
- [x] Tests updated for the new behavior.

## Work Log

### 2026-01-30 - Initial Review

**By:** Codex

**Actions:**
- Captured PR feedback and documented options.

**Learnings:**
- Current logic short-circuits PR links into "opened" status.

---

### 2026-01-30 - Resolution

**By:** Codex

**Actions:**
- Updated `src-tauri/src/summarize.rs` to require opened/created context before returning PR summary.
- Added tests to cover neutral PR references and explicit opened intent.

**Learnings:**
- Simple token-window checks prevent false positives without losing explicit open signals.
