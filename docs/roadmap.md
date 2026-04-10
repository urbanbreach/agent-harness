# Agent Harness roadmap

We are building a pure-Rust agent harness that can stand next to tools like Opencode, Codex and pi-mono, while absorbing the strongest workflow ideas from projects like oh-my-openagent and oh-my-codex without copying them 1:1. The focus is user comfort, clarity, polish, and strong real-world verification, with Opencode parity as the main goal, both functionally and visually.

CHECK THE BOXES AFTER THE ITEM CAN BE MARKED AS DONE.

## Product guardrails

- Keep the full product surface pure Rust, including UI, orchestration, and extension points.
- Optimize for comfort and clarity first; avoid clever workflows that feel opaque in practice.
- Keep headless and TUI behavior aligned so the product does not split into two different mental models.
- Make parity claims provable with deterministic PTY/live evidence and live-provider validation.
- Never use "Opencode, Codex, Pi" etc. when naming anything. If anything in the code refers to those or some other harness it needs to be renamed.

## Current locked default path

Before widening the roadmap, the current shell contract should stay explicit:

- blessed provider: the shipped `default` `openai_compatible` provider in `configs/harness.example.jsonc`
- blessed transport path: local CLIProxy-compatible loopback endpoint at `http://127.0.0.1:8317/v1`
- blessed default model: `gpt-5.4-mini` with low reasoning

Primary agents for roadmap work:

- `plan` planner agent. Use Opencode's Plan agent as a close inspiration
- `build` agent. Use Opencode's Build agent as a close inpiration

Secondary/supporting agents:

- As of now subagent functionality is something that is planned but not yet implemented. Only start planning and implementing subagents when `plan` and `build` agents have been successfully and thoroughly tested and implemented.

- [x] Plan and Build agents have been successfully implemented, tested and are on par with the ones in Opencode.

## Core parity and verification

- Keep the canonical journey signoff map in `docs/testing.md` current, including explicit CLI/TUI expectations and any live-coverage gaps.
- Treat PTY PNG/snapshot evidence and live transcript/manifest artifacts as acceptance criteria for parity-critical changes.
- Reach tool parity with other harnesses named in this document and make sure tests verify actual tool functionality against live providers.
- Flesh out tests to run against live providers via CLIProxyAPI using `gpt-5.4-mini` with low reasoning.
- Expand live verification so parity-critical flows are tested through both CLI and TUI paths.
- Track provider-specific behavior differences in tests instead of assuming one provider's behavior generalizes cleanly.

## TUI and UX parity

- [ ] Build a clean, Opencode-like interface by mapping the best UI ideas from both pi-mono and Opencode, then combining them into one coherent harness UI/UX.
- [ ] Clean up the sidebar so it is 1:1 with Opencode, both visually and functionally.
- [ ] Show modified files in the sidebar like Opencode, show +/- and have the element be collapsible.
- [ ] Clean up the area under the text input box in chat view so it is 1:1 with Opencode. Move Context data there like Opencode has it.
- [ ] Make the chat layout look 1:1 with Opencode, need improvements/polish all over.
- [x] Make chat boxes and surrounding elements feel 1:1 with Opencode.
- [ ] Make thinking traces look exactly like Opencode.
- [x] Flesh out the commands menu.
- [ ] Polish the commands menu UI to match the level of fit and finish seen in Opencode.
- [x] Implement Opencode's theme/Color scheme.
- [x] Improve session recovery and reopen flow so returning to previous work feels obvious and low-friction. Implement it like it is done in Opencode.

## Models, providers, and config

- [ ] Support model selection in harness from config.
- [ ] Support reasoning/thinking preset selection from config.
- [ ] Support all major parameters that Opencode's opencode.json supports.
- [ ] Add provider capability detection so unsupported features degrade cleanly instead of failing late.
- [ ] Set up a first-boot CLI flow so users can get from install to a working config with minimal friction.

## Agents, orchestration, and prompt quality

- [ ] Add named agents with fleshed-out system prompts for main flows such as Build and Plan.
- [ ] Flesh out subagents with configs available in JSON.
- [ ] Improve main-agent prompt engineering with a Opencode/pi-like approach.
- [ ] Compaction (Check how it's done from other harnesses and inspiration/ folder. The decide on the best path.)
- [ ] Add orchestration functionality that is toggleable in the HUD and follows the general spirit of oh-my-openagent / oh-my-codex while staying understandable. (after main agents are implemented properly)
- [ ] Add swarms and Ralph loops inspired by oh-my-codex. (after main agents are implemented properly)
- [ ] Add approval and policy controls for orchestration/tool execution so multi-agent runs remain legible and predictable. (after main agents are implemented properly)
- [ ] Hooks (from oh-my-openagent and oh-my-codex)

## Commands, panes, and workflow surfaces

- [ ] Add `/` commands that users expect from tools like Opencode/oh-my-openagent. (after main agents are implemented properly)
- [ ] Add `$` commands inspired by oh-my-codex. (after main agents are implemented properly)
- [ ] Add tmux support with subagent panes, configurable from config. (after main agents are implemented properly)
- [ ] Make command discovery and execution feel fast and obvious from anywhere in the session flow.

## Skills, plugins, and extensibility

- [ ] Flesh out skills support.
- [ ] Add plugin support, with the option to disable plugin-backed features completely when users do not want them.
- [ ] Decide which advanced features should be core and which should be shipped as optional plugins.
- [ ] Add first-class Openclaw support similar in spirit to oh-my-codex and oh-my-openagent.
