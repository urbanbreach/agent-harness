---
name: agent-browser
description: Browser task guidance for using the optional agent-browser CLI with dependency diagnostics and persisted evidence.
tools: [bash, read]
commands:
  - agent-browser --help
environment:
  allow:
    - AGENT_BROWSER_*
permissions:
  bash: ask
---

# Agent browser

Use this skill when the optional `agent-browser` CLI is the requested browser automation surface.

## Operating notes
- Check whether `agent-browser` is installed before relying on it.
- If missing, return a concise dependency diagnostic and continue with non-browser evidence when possible.
- Store generated screenshots, traces, and summaries under session artifacts rather than in transient terminal output only.
