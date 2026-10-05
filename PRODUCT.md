# Harness

Harness is a coding agent for terminal users. Its Rust CLI and Ratatui interface
let users ask agents to read and edit projects, run commands, call configured MCP
tools, and delegate work to named subagents.

One coordinator owns permissions, execution, cancellation, and durable events.
Users can inspect or replay saved sessions without repeating tools or contacting
providers. Replay and readiness remain read-only and offline.

The interface keeps the conversation readable, work status visible, and the next
action accessible by keyboard. Eval adds persistent code and tool composition
inside that existing workflow. [DESIGN.md](DESIGN.md) owns terminal presentation.

These product facts come from [README.md](README.md), [AGENTS.md](AGENTS.md), and
the established terminal design. No new audience or product direction is implied.
