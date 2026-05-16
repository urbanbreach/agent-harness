---
name: review-work
description: Focused review protocol for finding correctness, safety, regression, and test coverage issues before closeout.
---

# Review work

Use this skill when auditing a patch, plan, or completed implementation.

## Review stance
- Lead with concrete findings, ordered by severity.
- Anchor findings to file paths, line numbers, commands, or artifacts.
- Separate evidence from inference.
- Check for missing tests around behavior changes, public contracts, replay semantics, permissions, and user-visible flows.

## Closeout
- If no issues are found, say that directly and name the residual risk.
- Summaries are secondary to findings.
- Do not claim completion from passing tests alone; map the evidence back to the requested behavior.
