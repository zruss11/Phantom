---
status: complete
priority: p2
issue_id: "001"
tags: [summaries, parsing]
dependencies: []
---

# Fix JSON Extraction To Prefer Final Output

## Problem Statement
The JSON extraction in `src-tauri/src/namegen.rs` can return the first JSON object from echoed prompt examples instead of the model's final response, producing incorrect titles/branch names.

## Findings
- CodeRabbit notes smaller/echo-prone models may include example JSON before the real answer.
- The prior behavior that used the last `}` captured the final JSON object more reliably in this case.

## Proposed Solutions
1. **Return last complete JSON object** (preferred)
   - Pros: matches prior behavior, robust against echoed examples.
   - Cons: could skip earlier object if only one intended (rare).
2. **Keep first object with stronger filtering**
   - Pros: aligns with "first object" semantics.
   - Cons: requires heuristics to detect prompt examples; more brittle.

## Recommended Action
Update `extract_json_from_text` to return the last complete JSON object, preserving robustness against echoed prompt examples.

## Acceptance Criteria
- Title/branch generation ignores prompt example JSON and uses final response JSON.
- Unit test for multiple JSON objects expects the last object.

## Work Log
### 2026-02-02 - Created
**By:** Claude Code
**Actions:**
- Logged review feedback and planned fix.

### 2026-02-02 - Resolved
**By:** Claude Code
**Actions:**
- Updated JSON extraction to return the last valid object in `src-tauri/src/namegen.rs`.
- Expanded the unit test to cover echoed prompt examples.

**Learnings:**
- Validating JSON candidates prevents prompt examples from overriding the final output.
