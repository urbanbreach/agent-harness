---
name: dev-browser
description: Iterative development-browser workflow guidance for persistent browser state and visual/debug evidence.
tools: [webfetch, bash, read]
commands:
  - npx playwright --version
environment:
  allow:
    - PLAYWRIGHT_*
    - DEV_BROWSER_*
permissions:
  webfetch: ask
  bash: ask
---

# Dev browser

Use this skill for iterative web or app debugging where a persistent browser profile/state is useful.

## Operating notes
- Keep browser state scoped to the current run or explicitly configured workspace cache.
- Capture before/after evidence for UI fixes: URL, viewport, screenshot/trace path, and observed issue.
- Prefer deterministic checks first; use live browser sessions only when the task requires rendered behavior.
