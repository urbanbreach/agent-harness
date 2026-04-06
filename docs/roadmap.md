# Agent Harness roadmap

Build a pure-Rust agent harness that can stand next to tools like Opencode and pi-mono, while absorbing the strongest workflow ideas from projects like oh-my-openagent and oh-my-codex without copying them 1:1. The focus is user comfort, clarity, polish, and strong real-world verification.

## Product guardrails

- [ ] Keep the full product surface pure Rust, including UI, orchestration, and extension points.
- [ ] Optimize for comfort and clarity first; avoid clever workflows that feel opaque in practice.
- [ ] Keep headless and TUI behavior aligned so the product does not split into two different mental models.
- [ ] Make parity claims provable with deterministic PTY/live evidence and live-provider validation.

## Current locked default path

Before widening the roadmap, the current shell contract should stay explicit:

- blessed provider: the shipped `default` `openai_compatible` provider in `configs/harness.example.jsonc`
- blessed transport path: local CLIProxy-compatible loopback endpoint at `http://127.0.0.1:8317/v1`
- blessed first-run profile: `plan`
- blessed handoff profile: `build`
- blessed default model: `gpt-5.4-mini`

Primary profiles for roadmap work:

- `plan`
- `build`

Secondary/supporting profiles:

- `tool_audit`
- `deep_compat`

## Core parity and verification

- [ ] Keep the canonical journey signoff map in `docs/testing.md` current, including explicit CLI/TUI expectations and any live-coverage gaps.
- [ ] Treat PTY PNG/snapshot evidence and live transcript/manifest artifacts as acceptance criteria for parity-critical changes.
- [ ] Reach tool parity and make sure tests verify actual tool functionality against live providers.
- [ ] Flesh out tests to run against live providers via CLIProxyAPI using `gpt-5.4-mini` with low reasoning.
- [ ] Expand live verification so parity-critical flows are tested through both CLI and TUI paths.
- [ ] Track provider-specific behavior differences in tests instead of assuming one provider's behavior generalizes cleanly.

## TUI and UX parity

- [ ] Build a clean, pi-like interface by mapping the best UI ideas from both pi-mono and Opencode, then combining them into one coherent harness UX.
- [ ] Clean up the sidebar so it is 1:1 with Opencode.
  - [ ] Show modified files in the sidebar like Opencode.
- [ ] Clean up the area under the text input box in chat view so it is 1:1 with Opencode.
- [ ] Make the chat layout look 1:1 with Opencode.
- [ ] Make chat boxes and surrounding elements feel 1:1 with Opencode.
- [ ] Make thinking traces look exactly like Opencode.
- [ ] Flesh out the commands menu.
- [ ] Polish the commands menu UI to match the level of fit and finish seen in Opencode.
- [x] Add theme support.
- [x] Add clearer HUD/status visibility for model, profile, tool, and run state so users always know what the harness is doing.
- [ ] Improve session recovery and reopen flow so returning to previous work feels obvious and low-friction.

## Models, providers, and config

- [x] Support model selection in harness from config.
- [ ] Support reasoning/thinking preset selection from config.
- [ ] Figure out actual prompt and token caching behavior for OpenAI, Google, Anthropic, Qwen, Kimi, GLM, and Minimax from their available documentation, then implement provider-aware handling.
- [ ] Add provider capability detection so unsupported features degrade cleanly instead of failing late.
- [ ] Set up a first-boot CLI flow so users can get from install to a working config with minimal friction.

## Agents, orchestration, and prompt quality

- [ ] Add agent profiles with fleshed-out system prompts for main flows such as Build and Plan.
- [ ] Flesh out subagents with configs available in JSON.
- [ ] Improve subagent prompt engineering.
- [ ] Improve main-agent prompt engineering with a pi-like approach.
- [ ] Add orchestration functionality that is toggleable in the HUD and follows the general spirit of oh-my-openagent / oh-my-codex while staying understandable.
- [ ] Add swarms and Ralph loops inspired by oh-my-codex.
- [ ] Add approval and policy controls for orchestration/tool execution so multi-agent runs remain legible and predictable.

## Commands, panes, and workflow surfaces

- [ ] Add `/` commands that users expect from tools like Opencode.
- [ ] Add `$` commands inspired by oh-my-codex.
- [ ] Add tmux support with subagent panes, configurable from config.
- [ ] Make command discovery and execution feel fast and obvious from anywhere in the session flow.

## Skills, plugins, and extensibility

- [ ] Flesh out skills support.
- [ ] Add plugin support, with the option to disable plugin-backed features completely when users do not want them.
- [ ] Decide which advanced features should be core and which should be shipped as optional plugins.
- [ ] Add first-class Openclaw support similar in spirit to oh-my-codex and oh-my-openagent.
