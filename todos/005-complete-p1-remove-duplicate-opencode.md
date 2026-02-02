---
status: complete
priority: p1
issue_id: "005"
tags: [summaries, build]
dependencies: []
---

# Ensure Only One call_opencode_cli Definition

## Problem Statement
Duplicate definitions of `call_opencode_cli` in `src-tauri/src/summarize.rs` would cause compile failure.

## Findings
- Review comment flags a duplicate function in the PR diff.
- Current branch appears to have only one definition, but PR thread remains unresolved.

## Proposed Solutions
1. **Remove duplicate definition** (preferred)
   - Pros: resolves compile error and thread.
   - Cons: none.
2. **Rename one function**
   - Pros: preserves both behaviors if intended.
   - Cons: needs call site updates; likely unnecessary.

## Recommended Action
Confirm only a single `call_opencode_cli` exists and remove any duplicate in the diff.

## Acceptance Criteria
- Only one `call_opencode_cli` function in `summarize.rs`.
- `cargo check` for `src-tauri` passes.

## Work Log
### 2026-02-02 - Created
**By:** Claude Code
**Actions:**
- Logged review feedback and planned fix.

### 2026-02-02 - Resolved
**By:** Claude Code
**Actions:**
- Removed the duplicate `call_opencode_cli` definition in `src-tauri/src/summarize.rs`.

**Learnings:**
- Keeping a single helper avoids compile-time duplicate symbol errors.
