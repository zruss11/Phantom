# Branch Selector: Open PRs First

## Goal
- In the Base Branch dropdown, show branches with open PRs at the top.
- Show an open PR number next to the branch name as a clean pill badge.

## Manual Test Checklist
1. Pick a GitHub-backed repo with at least 1 open PR.
2. Open the Create Tasks UI and focus `Base Branch`.
3. Verify branches that have open PRs appear at the top of the list.
4. Verify each open-PR branch shows a `#123` style pill on the right.
5. Hover an open-PR branch and confirm the tooltip includes the PR number and title.
6. Type in the search box:
   - Search by branch name and verify filtering works.
   - Search by PR number (e.g. `123`) and verify the matching branch appears.
7. Switch project paths:
   - Verify the list refreshes and still prioritizes open-PR branches when available.

## Notes
- If `gh` is unavailable or not authenticated, the dropdown still loads branches, but no PR pills are shown and no reordering occurs.

