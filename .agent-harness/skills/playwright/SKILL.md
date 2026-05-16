---
name: playwright
description: Browser automation guidance for Playwright-backed MCP sessions, screenshots, and interaction evidence.
tools: [webfetch, read, bash]
commands:
  - npx playwright --version
  - npx @playwright/mcp --help
environment:
  allow:
    - PLAYWRIGHT_*
    - BROWSER_*
mcp:
  playwright:
    command: npx @playwright/mcp
permissions:
  webfetch: ask
  bash: ask
---

# Playwright browser automation

Use this skill when a task needs browser inspection, interaction, screenshots, accessibility checks, or console/network evidence.

## Operating notes
- Prefer a skill-scoped Playwright MCP server when available; do not assume it is globally running.
- Treat browser dependencies as optional: report missing `npx`, Playwright, or browser binaries as actionable setup diagnostics.
- Persist screenshots, traces, console summaries, and network findings as task evidence when the lane is enabled.
- Keep live/browser work environment-gated and avoid credentials unless explicitly provided for the task.
