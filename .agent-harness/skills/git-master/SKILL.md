---
name: git-master
description: Git workflow guidance for clean history, safe diffs, and evidence-rich commits in the harness workspace.
---

# Git master

Use this skill when preparing commits, reviewing local changes, or reasoning about branch state.

## Workflow
- Inspect `git status --short` before editing or committing.
- Treat uncommitted changes you did not make as user-owned; work around them.
- Keep commits focused on one behavioral purpose.
- Prefer non-interactive git commands.
- Avoid destructive commands unless the user explicitly requested them.

## Commit notes
- Use the Lore commit style for this repository.
- The first line should explain why the change exists.
- Include useful trailers such as `Constraint:`, `Rejected:`, `Confidence:`, `Scope-risk:`, `Tested:`, and `Not-tested:`.
- Mention exact verification commands that were run.
