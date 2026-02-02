---
status: complete
priority: p1
issue_id: "004"
tags: [privacy, oauth, config]
dependencies: []
---

# Gate Writes To ~/.claude Credentials Behind Opt-In

## Problem Statement
`ensure_claude_credentials_file` currently creates/overwrites `~/.claude/.credentials.json`, conflicting with guidelines to treat agent config as read-only unless explicitly consented.

## Findings
- The function writes credentials to support Docker runtime OAuth.
- Guidelines require explicit user consent or documented exception for writing to `~/.claude`.

## Proposed Solutions
1. **Add explicit opt-in setting** (preferred)
   - Pros: compliant with policy; transparent to user.
   - Cons: requires UI/setting plumbing.
2. **Read-only mode with warning**
   - Pros: no new setting.
   - Cons: Docker OAuth may not work without user action.

## Recommended Action
Add a setting/flag that explicitly allows managing Claude credentials. Only write `~/.claude/.credentials.json` when opt-in is true; otherwise, read existing data only.

## Acceptance Criteria
- No writes to `~/.claude` unless opt-in is enabled.
- Clear log/UX indicating when writes occur.

## Work Log
### 2026-02-02 - Created
**By:** Claude Code
**Actions:**
- Logged review feedback and planned fix.

### 2026-02-02 - Resolved
**By:** Claude Code
**Actions:**
- Added `claudeWriteCredentials` setting and opt-in gate in `src-tauri/src/main.rs`.
- Wired UI toggle in `gui/menu.html` and settings flow in `gui/js/application.js`.

**Learnings:**
- Explicit opt-in keeps `~/.claude` writes aligned with policy while supporting Docker OAuth.
