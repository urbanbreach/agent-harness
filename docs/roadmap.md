# Agent Harness roadmap

We are building a pure-Rust agent harness with a polished, self-contained product identity. The focus is user comfort, clarity, polish, and strong real-world verification, with a high bar for functionality and fit-and-finish.

CHECK THE BOXES AFTER THE ITEM CAN BE MARKED AS DONE.

## Product guardrails

- Keep the full product surface pure Rust, including UI, orchestration, and extension points.
- Optimize for comfort and clarity first; avoid clever workflows that feel opaque in practice.
- Keep headless and TUI behavior aligned so the product does not split into two different mental models.
- Make parity claims provable with deterministic PTY/live evidence and live-provider validation.
- Keep harness-owned names self-contained. If anything in the code or docs points at another harness identity, rename it.

## Current locked default path

Before widening the roadmap, the current shell contract should stay explicit:

- blessed provider: the shipped `default` `openai_compatible` provider in `configs/harness.example.jsonc`
- blessed transport path: local CLIProxy-compatible loopback endpoint at `http://127.0.0.1:8317/v1`
- blessed default model: `gpt-5.4`, with `gpt-5.4-mini` high reasoning for interactive work

Primary agents for roadmap work:

- `plan` planner agent.
- `build` agent.

Secondary/supporting agents:

- As of now subagent functionality is something that is planned but not yet implemented. Only start planning and implementing subagents when `plan` and `build` agents have been successfully and thoroughly tested and implemented.

- [ ] Plan and Build agents have been successfully implemented, tested, and meet the harness quality bar.

## Core parity and verification

- Keep the canonical journey signoff map in `docs/testing.md` current, including explicit CLI/TUI expectations and any live-coverage gaps.
- Treat PTY PNG/snapshot evidence and live transcript/manifest artifacts as acceptance criteria for parity-critical changes.
- Reach tool parity with other harnesses named in this document and make sure tests verify actual tool functionality against live providers.
- Flesh out tests to run against live providers via CLIProxyAPI using the documented default model plus the `gpt-5.4-mini` signoff variants.
- Expand live verification so parity-critical flows are tested through both CLI and TUI paths.
- Track provider-specific behavior differences in tests instead of assuming one provider's behavior generalizes cleanly.

## TUI and UX parity

- [ ] Build a clean, cohesive interface that feels unmistakably like this harness.
- [ ] Clean up the sidebar so it feels polished, cohesive, and fully native to this harness.
- [ ] Show modified files in the sidebar with +/- indicators and a collapsible presentation.
- [ ] Clean up the area under the text input box in chat view so Context data lives there cleanly and predictably.
- [ ] Make the chat layout feel cohesive and polished from edge to edge.
- [ ] Make chat boxes and surrounding elements feel deliberate and refined.
- [ ] Make thinking traces feel polished, readable, and well integrated into the shell.
- [ ] Flesh out the commands menu.
- [ ] Polish the commands menu UI so it matches the overall shell quality bar.
- [ ] Implement the harness theme and color system.
- [ ] Improve session recovery and reopen flow so returning to previous work feels obvious and low-friction.

## Models, providers, and config

- [ ] Support model selection in harness from config.
- [ ] Support reasoning/thinking preset selection from config.
- [ ] Support the major configuration parameters required by the harness runtime.
- [ ] Add provider capability detection so unsupported features degrade cleanly instead of failing late.
- [ ] Set up a first-boot CLI flow so users can get from install to a working config with minimal friction.

## Agents, orchestration, and prompt quality

- [ ] Add named agents with fleshed-out system prompts for main flows such as Build and Plan.
- [ ] Flesh out subagents with configs available in JSON.
- [ ] Improve main-agent prompt engineering with a high-signal, low-noise approach.
- [ ] Compaction (Check how it's done from other harnesses and inspiration/ folder. The decide on the best path.)
- [ ] Add orchestration functionality that is toggleable in the HUD while staying legible and understandable. (after main agents are implemented properly)
- [ ] Add swarm and persistent completion-loop functionality. (after main agents are implemented properly)
- [ ] Add approval and policy controls for orchestration/tool execution so multi-agent runs remain legible and predictable. (after main agents are implemented properly)
- [ ] Hooks and automation triggers.

## Commands, panes, and workflow surfaces

- [ ] Add `/` commands that users expect from a modern agent shell. (after main agents are implemented properly)
- [ ] Add `$` commands for concise workflow shortcuts. (after main agents are implemented properly)
- [ ] Add tmux support with subagent panes, configurable from config. (after main agents are implemented properly)
- [ ] Make command discovery and execution feel fast and obvious from anywhere in the session flow.

## Skills, plugins, and extensibility

- [ ] Flesh out skills support.
- [ ] Add plugin support, with the option to disable plugin-backed features completely when users do not want them.
- [ ] Decide which advanced features should be core and which should be shipped as optional plugins.
- [ ] Add first-class Openclaw support with the same level of polish as the rest of the harness.
