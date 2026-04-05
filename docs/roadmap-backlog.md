# Roadmap backlog

Tracked roadmap execution queue for the `claw-dev` line.

> Source-of-truth note: this backlog stays tracked in-repo so isolated issue lanes can work from a stable queue. The RB-03 canonical journey signoff matrix lives in `docs/testing.md`, with the shorter parity summary in `docs/parity-scorecard.md`.

## Recommended first implementation slice

**Start with RB-01 — Lock the core parity target and blessed default path (https://github.com/urbanbreach/agent-harness/issues/68).**

Why this first:
- `docs/parity-scorecard.md` says the first work package is to lock the target, the blessed default path, the ranked gap list, and the signoff plan.
- Later TUI, provider, orchestration, command, and plugin issues depend on one agreed shell contract.
- Landing RB-01 lets future isolated issue lanes move without re-planning the whole roadmap.

## Ordering principles

1. Treat the roadmap guardrails as constraints on every issue, not as standalone work items.
2. Finish the core parity package before multi-provider expansion, plugin/platform work, or advanced orchestration.
3. Drive UI work from the ranked parity gaps and signoff map, not from speculative redesigns.
4. Keep the shipped config, plan -> build workflow, and documented test lanes as the baseline contract.

## Milestone batches

## Batch 1 — Core parity target and verification baseline

Included queue items: RB-01, RB-02, RB-03, RB-04, RB-05
## Batch 2 — Transcript-first Opencode parity

Included queue items: RB-06, RB-07, RB-08, RB-09, RB-10, RB-11, RB-12, RB-13
## Batch 3 — Models, providers, and config ergonomics

Included queue items: RB-14, RB-15, RB-16, RB-17, RB-18
## Batch 4 — Agent, orchestration, and prompt quality foundations

Included queue items: RB-19, RB-20, RB-21, RB-22
## Batch 5 — Commands, panes, and workflow surfaces

Included queue items: RB-23, RB-24, RB-25, RB-26
## Batch 6 — Skills, plugins, and extensibility

Included queue items: RB-27, RB-28, RB-29, RB-30

## Ordered issue queue

| Queue | GitHub | Milestone batch | Depends on | Title |
| --- | --- | --- | --- | --- |
| RB-01 | [#68](https://github.com/urbanbreach/agent-harness/issues/68) | Batch 1 — Core parity target and verification baseline | — | Lock the core parity target and blessed default path |
| RB-02 | [#69](https://github.com/urbanbreach/agent-harness/issues/69) | Batch 1 — Core parity target and verification baseline | RB-01 | Rank current parity gaps against canonical evidence |
| RB-03 | [#70](https://github.com/urbanbreach/agent-harness/issues/70) | Batch 1 — Core parity target and verification baseline | RB-01, RB-02 | Map canonical journeys to deterministic PTY and live signoff |
| RB-04 | [#71](https://github.com/urbanbreach/agent-harness/issues/71) | Batch 1 — Core parity target and verification baseline | RB-03 | Expand live-provider parity coverage across CLI and TUI paths |
| RB-05 | [#72](https://github.com/urbanbreach/agent-harness/issues/72) | Batch 1 — Core parity target and verification baseline | RB-04 | Track provider-specific behavior differences in parity tests |
| RB-06 | [#73](https://github.com/urbanbreach/agent-harness/issues/73) | Batch 2 — Transcript-first Opencode parity | RB-02, RB-03 | Match the sidebar structure and modified-files visibility to Opencode |
| RB-07 | [#74](https://github.com/urbanbreach/agent-harness/issues/74) | Batch 2 — Transcript-first Opencode parity | RB-02, RB-03 | Match the composer footer and under-input controls to Opencode |
| RB-08 | [#75](https://github.com/urbanbreach/agent-harness/issues/75) | Batch 2 — Transcript-first Opencode parity | RB-02, RB-03 | Match transcript layout, chat boxes, and shell spacing to Opencode |
| RB-09 | [#76](https://github.com/urbanbreach/agent-harness/issues/76) | Batch 2 — Transcript-first Opencode parity | RB-08 | Match the thinking-trace presentation to Opencode |
| RB-10 | [#77](https://github.com/urbanbreach/agent-harness/issues/77) | Batch 2 — Transcript-first Opencode parity | RB-08 | Improve transcript and tool-row disclosure, metadata, and state visibility |
| RB-11 | [#78](https://github.com/urbanbreach/agent-harness/issues/78) | Batch 2 — Transcript-first Opencode parity | RB-02, RB-03 | Improve session recovery, reopen, replay, and child-session discoverability |
| RB-12 | [#79](https://github.com/urbanbreach/agent-harness/issues/79) | Batch 2 — Transcript-first Opencode parity | RB-06, RB-07, RB-08 | Flesh out and polish the commands menu |
| RB-13 | [#80](https://github.com/urbanbreach/agent-harness/issues/80) | Batch 2 — Transcript-first Opencode parity | RB-08, RB-10 | Add theme support and clearer HUD/status visibility |
| RB-14 | [#81](https://github.com/urbanbreach/agent-harness/issues/81) | Batch 3 — Models, providers, and config ergonomics | RB-01 | Support config-driven model selection |
| RB-15 | [#82](https://github.com/urbanbreach/agent-harness/issues/82) | Batch 3 — Models, providers, and config ergonomics | RB-14 | Support config-driven reasoning and thinking presets |
| RB-16 | [#83](https://github.com/urbanbreach/agent-harness/issues/83) | Batch 3 — Models, providers, and config ergonomics | RB-14, RB-15 | Implement provider capability detection and graceful degradation |
| RB-17 | [#84](https://github.com/urbanbreach/agent-harness/issues/84) | Batch 3 — Models, providers, and config ergonomics | RB-16 | Research and implement provider-aware prompt/token caching behavior |
| RB-18 | [#85](https://github.com/urbanbreach/agent-harness/issues/85) | Batch 3 — Models, providers, and config ergonomics | RB-14, RB-15, RB-16 | Add a first-boot CLI flow for working config setup |
| RB-19 | [#86](https://github.com/urbanbreach/agent-harness/issues/86) | Batch 4 — Agent, orchestration, and prompt quality foundations | RB-01 | Ship stronger Plan/Build agent profiles and main-agent prompts |
| RB-20 | [#87](https://github.com/urbanbreach/agent-harness/issues/87) | Batch 4 — Agent, orchestration, and prompt quality foundations | RB-19 | Add JSON-configured subagents and improve subagent prompts |
| RB-21 | [#88](https://github.com/urbanbreach/agent-harness/issues/88) | Batch 4 — Agent, orchestration, and prompt quality foundations | RB-13, RB-19, RB-20 | Add legible orchestration controls with approval and policy visibility |
| RB-22 | [#89](https://github.com/urbanbreach/agent-harness/issues/89) | Batch 4 — Agent, orchestration, and prompt quality foundations | RB-20, RB-21 | Add swarm and Ralph-style orchestration loops within the harness shell contract |
| RB-23 | [#90](https://github.com/urbanbreach/agent-harness/issues/90) | Batch 5 — Commands, panes, and workflow surfaces | RB-12, RB-19 | Add the expected slash-command surface |
| RB-24 | [#91](https://github.com/urbanbreach/agent-harness/issues/91) | Batch 5 — Commands, panes, and workflow surfaces | RB-19, RB-21, RB-23 | Add the dollar-command workflow surface |
| RB-25 | [#92](https://github.com/urbanbreach/agent-harness/issues/92) | Batch 5 — Commands, panes, and workflow surfaces | RB-18, RB-20, RB-21 | Add tmux-backed subagent panes configurable from config |
| RB-26 | [#93](https://github.com/urbanbreach/agent-harness/issues/93) | Batch 5 — Commands, panes, and workflow surfaces | RB-12, RB-23, RB-24, RB-25 | Make command discovery and execution obvious across the session flow |
| RB-27 | [#94](https://github.com/urbanbreach/agent-harness/issues/94) | Batch 6 — Skills, plugins, and extensibility | RB-18, RB-24 | Flesh out skills support |
| RB-28 | [#95](https://github.com/urbanbreach/agent-harness/issues/95) | Batch 6 — Skills, plugins, and extensibility | RB-01, RB-27 | Decide which advanced features are core versus optional plugins |
| RB-29 | [#96](https://github.com/urbanbreach/agent-harness/issues/96) | Batch 6 — Skills, plugins, and extensibility | RB-18, RB-28 | Add plugin support with a full disable path |
| RB-30 | [#97](https://github.com/urbanbreach/agent-harness/issues/97) | Batch 6 — Skills, plugins, and extensibility | RB-25, RB-27, RB-29 | Add first-class Openclaw support |

## Issue-ready breakdown

### RB-01 — Lock the core parity target and blessed default path
- GitHub issue: [#68](https://github.com/urbanbreach/agent-harness/issues/68)
- Milestone batch: Batch 1 — Core parity target and verification baseline
- Dependency order: None
- Scope:
  - Align the roadmap, README, plan-build workflow, and shipped example config around one clearly recommended default path.
  - Name which existing profiles are primary versus secondary so later parity work has one target shell contract.
  - Keep the scope tied to the current pure-Rust harness surface without adding new product promises.
- Acceptance notes:
  - The default provider/profile/model path is explicit in docs and config.
  - Primary vs secondary profiles are documented once and referenced consistently.
  - The resulting contract stays within the current roadmap and parity scorecard boundaries.
- Roadmap anchors:
  - docs/roadmap.md: product guardrails; core parity and verification
  - docs/parity-scorecard.md: P0 define and lock the target
  - docs/plan-build-workflow.md: canonical plan -> build split
### RB-02 — Rank current parity gaps against canonical evidence
- GitHub issue: [#69](https://github.com/urbanbreach/agent-harness/issues/69)
- Milestone batch: Batch 1 — Core parity target and verification baseline
- Dependency order: RB-01
- Scope:
  - Compare the current harness against the five canonical user journeys using the parity scorecard, the shipped PTY/live artifacts, and the Opencode parity audit.
  - Turn the findings into a ranked gap list tied to concrete screenshots, transcript evidence, or runtime behavior.
  - Use the ranked list to drive the order of follow-on TUI and verification work.
- Acceptance notes:
  - Every ranked gap cites the evidence that proves the gap exists today.
  - The list distinguishes blocker gaps from polish gaps.
  - The ranking matches the canonical journeys instead of ad hoc UI preferences.
- Roadmap anchors:
  - docs/roadmap.md: TUI and UX parity
  - docs/parity-scorecard.md: ranked gap list tied to concrete UX/runtime evidence
  - opencode-parity-audit.md
- Current evidence-driven follow-on order:
  1. **RB-03** — map each canonical journey to deterministic PTY/live signoff before more UI churn
  2. **RB-10** — close the transcript disclosure, metadata, timing, and tool-state blocker for journeys 2 and 5
  3. **RB-11** — close the recovery/reopen/artifact-discovery blocker for journey 4
  4. **RB-07** and **RB-13** — raise first-run, permission, and HUD/status clarity once the transcript and recovery blockers are ordered
  5. **RB-06**, **RB-08**, and **RB-09** — finish the remaining sidebar, shell-layout, and thinking-trace polish after the blocker/high items
### RB-03 — Map canonical journeys to deterministic PTY and live signoff
- GitHub issue: [#70](https://github.com/urbanbreach/agent-harness/issues/70)
- Milestone batch: Batch 1 — Core parity target and verification baseline
- Dependency order: RB-01, RB-02
- Scope:
  - Map each canonical journey to existing deterministic PTY coverage, existing live coverage, or an explicit missing-coverage gap.
  - Make CLI and TUI signoff expectations explicit for parity-critical flows.
  - Treat visual artifacts and transcript evidence as acceptance criteria, not just debugging output.
- Acceptance notes:
  - Each canonical journey has a documented signoff path.
  - Missing coverage is called out before implementation lanes start.
  - The signoff map references the existing harness-testkit lanes rather than inventing new verification categories.
  - PTY PNG/snapshot evidence and live transcript/manifest artifacts are named as acceptance criteria, not optional debugging output.
- Roadmap anchors:
  - docs/roadmap.md: core parity and verification
  - docs/parity-scorecard.md: top journeys have matching signoff plans
  - docs/testing.md
### RB-04 — Expand live-provider parity coverage across CLI and TUI paths
- GitHub issue: [#71](https://github.com/urbanbreach/agent-harness/issues/71)
- Milestone batch: Batch 1 — Core parity target and verification baseline
- Dependency order: RB-03
- Scope:
  - Extend the live-provider lanes so the highest-priority parity journeys run through both prompt/CLI and TUI flows.
  - Prefer composed signoff wrappers around the shipped harness-testkit live lanes so Batch 1 closeout stays tied to the #70 signoff map instead of inventing new verification categories.
  - Use CLIProxyAPI with gpt-5.4-mini at low reasoning where the roadmap calls for live-provider parity verification.
  - Keep the live coverage aligned with the deterministic PTY contract instead of diverging into a second UX story.
- Acceptance notes:
  - Parity-critical journeys have live coverage through both CLI and TUI entrypoints where appropriate.
  - The added coverage is framed as signoff, not speculative provider exploration.
  - The live lanes remain actionable when they fail (clear evidence, not opaque flakes).
- Roadmap anchors:
  - docs/roadmap.md: Flesh out tests to run against live providers via CLIProxyAPI using gpt-5.4-mini with low reasoning; expand live verification through both CLI and TUI paths
  - docs/testing.md: live visual and chat-control signoff lanes
### RB-05 — Track provider-specific behavior differences in parity tests
- GitHub issue: [#72](https://github.com/urbanbreach/agent-harness/issues/72)
- Milestone batch: Batch 1 — Core parity target and verification baseline
- Dependency order: RB-04
- Scope:
  - Teach parity-critical tests to record provider-specific behavior differences instead of assuming one provider generalizes cleanly.
  - Keep the differences visible in fixtures or expectations so later provider work builds on evidence.
  - Avoid widening into multi-provider feature expansion beyond what parity signoff needs.
- Acceptance notes:
  - Parity tests can encode provider-specific expectations where behavior differs.
  - Differences are documented as observed behavior, not papered over with loose assertions.
  - The scope stays limited to parity verification behavior.
- Roadmap anchors:
  - docs/roadmap.md: Track provider-specific behavior differences in tests
  - docs/testing.md
### RB-06 — Match the sidebar structure and modified-files visibility to Opencode
- GitHub issue: [#73](https://github.com/urbanbreach/agent-harness/issues/73)
- Milestone batch: Batch 2 — Transcript-first Opencode parity
- Dependency order: RB-02, RB-03
- Scope:
  - Close the highest-value sidebar parity gaps identified in the ranked gap list.
  - Show modified files in the sidebar in a way that matches the roadmap target.
  - Keep child-session and artifact discoverability aligned with the broader recovery flow.
- Acceptance notes:
  - Sidebar structure is closer to the target contract defined in Batch 1.
  - Modified files are visible from the sidebar.
  - Changes land with PTY evidence instead of screenshots-only claims.
- Roadmap anchors:
  - docs/roadmap.md: Clean up the sidebar so it is 1:1 with Opencode; show modified files in the sidebar like Opencode
### RB-07 — Match the composer footer and under-input controls to Opencode
- GitHub issue: [#74](https://github.com/urbanbreach/agent-harness/issues/74)
- Milestone batch: Batch 2 — Transcript-first Opencode parity
- Dependency order: RB-02, RB-03
- Scope:
  - Bring the area under the text input box in chat view up to the target parity contract.
  - Keep the control strip legible so users can understand run state, commands, and next actions without hidden state.
  - Integrate the change with the current Ratatui layout contracts rather than scattering layout logic.
- Acceptance notes:
  - The under-input area matches the chosen parity target more closely.
  - The control area remains legible in deterministic PTY evidence.
  - No new layout rules are introduced outside the established TUI boundaries.
- Roadmap anchors:
  - docs/roadmap.md: Clean up the area under the text input box in chat view so it is 1:1 with Opencode
### RB-08 — Match transcript layout, chat boxes, and shell spacing to Opencode
- GitHub issue: [#75](https://github.com/urbanbreach/agent-harness/issues/75)
- Milestone batch: Batch 2 — Transcript-first Opencode parity
- Dependency order: RB-02, RB-03
- Scope:
  - Bring the main chat layout, message framing, and surrounding spacing toward the 1:1 parity target.
  - Use the transcript-first shell as the main reference surface instead of isolated widgets.
  - Preserve headless/TUI alignment while improving the visible shell contract.
- Acceptance notes:
  - The main transcript shell reads closer to the intended Opencode-class layout.
  - Chat boxes and surrounding elements feel coherent as one shell, not a set of local tweaks.
  - Deterministic PTY evidence proves the new baseline.
- Roadmap anchors:
  - docs/roadmap.md: Make the chat layout look 1:1 with Opencode; make chat boxes and surrounding elements feel 1:1 with Opencode
### RB-09 — Match the thinking-trace presentation to Opencode
- GitHub issue: [#76](https://github.com/urbanbreach/agent-harness/issues/76)
- Milestone batch: Batch 2 — Transcript-first Opencode parity
- Dependency order: RB-08
- Scope:
  - Bring thinking traces up to the roadmap parity target within the transcript-first shell.
  - Keep trace disclosure compatible with the ranked gap list and signoff map from Batch 1.
  - Avoid adding a second parallel trace UI that splits the mental model.
- Acceptance notes:
  - Thinking traces match the intended presentation direction more closely.
  - Trace disclosure remains legible in dense transcript sessions.
  - Parity evidence covers the resulting trace presentation.
- Roadmap anchors:
  - docs/roadmap.md: Make thinking traces look exactly like Opencode
### RB-10 — Improve transcript and tool-row disclosure, metadata, and state visibility
- GitHub issue: [#77](https://github.com/urbanbreach/agent-harness/issues/77)
- Milestone batch: Batch 2 — Transcript-first Opencode parity
- Dependency order: RB-08
- Scope:
  - Improve per-item disclosure depth, inline metadata, timing, duration, failure visibility, and active/running row clarity in the transcript.
  - Keep shell, edit, search, and generic tool calls readable in tool-heavy runs.
  - Use the ranked parity gaps as the source of truth for what matters first.
- Acceptance notes:
  - Dense transcript sessions remain legible without collapsing into noise.
  - Active, pending, completed, and failed states are visible without log-diving.
  - The result improves the core parity journeys instead of adding sidecar UI.
- Roadmap anchors:
  - docs/roadmap.md: Reach tool parity and make sure tests verify actual tool functionality against live providers; add clearer HUD/status visibility
  - docs/parity-scorecard.md: transcript-first live session; tool-heavy run inspection
### RB-11 — Improve session recovery, reopen, replay, and child-session discoverability
- GitHub issue: [#78](https://github.com/urbanbreach/agent-harness/issues/78)
- Milestone batch: Batch 2 — Transcript-first Opencode parity
- Dependency order: RB-02, RB-03
- Scope:
  - Make session list, reopen, continue, replay, child-session relationships, and artifact discovery low-friction for returning users.
  - Keep the recovery flow aligned between the live TUI and replay surfaces.
  - Treat recovery as a primary parity journey rather than a secondary admin view.
- Acceptance notes:
  - Returning to prior work is obvious from the shipped shell.
  - Child-session and artifact relationships are legible.
  - The recovery flow has explicit signoff coverage.
- Roadmap anchors:
  - docs/roadmap.md: Improve session recovery and reopen flow so returning to previous work feels obvious and low-friction
  - docs/parity-scorecard.md: continue-session and recovery flow
### RB-12 — Flesh out and polish the commands menu
- GitHub issue: [#79](https://github.com/urbanbreach/agent-harness/issues/79)
- Milestone batch: Batch 2 — Transcript-first Opencode parity
- Dependency order: RB-06, RB-07, RB-08
- Scope:
  - Fill out the commands menu with the expected surface area from the current roadmap.
  - Polish the menu UI so it matches the rest of the shell instead of feeling bolted on.
  - Keep the menu aligned with the later / and $ command backlog rather than inventing separate command systems.
- Acceptance notes:
  - The commands menu exposes the intended shell actions clearly.
  - The menu looks integrated with the surrounding TUI.
  - The design leaves a clean path for later / and $ command issues.
- Roadmap anchors:
  - docs/roadmap.md: Flesh out the commands menu; polish the commands menu UI to match Opencode fit and finish
### RB-13 — Add theme support and clearer HUD/status visibility
- GitHub issue: [#80](https://github.com/urbanbreach/agent-harness/issues/80)
- Milestone batch: Batch 2 — Transcript-first Opencode parity
- Dependency order: RB-08, RB-10
- Scope:
  - Add theme support for the harness shell.
  - Make model, profile, tool, and run state visible enough that the shell always explains itself.
  - Keep visual status cues aligned with the transcript-first shell instead of splitting status into hidden panels.
- Acceptance notes:
  - Theme selection exists without destabilizing the parity baseline.
  - Users can always see the current model/profile/tool/run state.
  - The new status cues remain consistent across the main shell surfaces.
- Roadmap anchors:
  - docs/roadmap.md: Add theme support; add clearer HUD/status visibility for model, profile, tool, and run state
### RB-14 — Support config-driven model selection
- GitHub issue: [#81](https://github.com/urbanbreach/agent-harness/issues/81)
- Milestone batch: Batch 3 — Models, providers, and config ergonomics
- Dependency order: RB-01
- Scope:
  - Allow harness users to choose models from config on the public configuration surface.
  - Keep the configured model path aligned with the blessed default path established in Batch 1.
  - Avoid expanding into unrelated provider abstractions while landing the model-selection surface.
- Acceptance notes:
  - Users can select models from config without patching code.
  - The example config and docs explain the supported path.
  - Validation catches broken model references early.
- Roadmap anchors:
  - docs/roadmap.md: Support model selection in harness from config
  - docs/config.md
### RB-15 — Support config-driven reasoning and thinking presets
- GitHub issue: [#82](https://github.com/urbanbreach/agent-harness/issues/82)
- Milestone batch: Batch 3 — Models, providers, and config ergonomics
- Dependency order: RB-14
- Scope:
  - Expose reasoning/thinking preset selection on the public config surface.
  - Make the selected preset visible in the shell where status visibility matters.
  - Keep the surface provider-aware so unsupported combinations do not fail late.
- Acceptance notes:
  - Profiles can select supported reasoning/thinking presets from config.
  - Docs and example config show the supported path.
  - Unsupported combinations fail fast or degrade cleanly.
- Roadmap anchors:
  - docs/roadmap.md: Support reasoning/thinking preset selection from config
### RB-16 — Implement provider capability detection and graceful degradation
- GitHub issue: [#83](https://github.com/urbanbreach/agent-harness/issues/83)
- Milestone batch: Batch 3 — Models, providers, and config ergonomics
- Dependency order: RB-14, RB-15
- Scope:
  - Detect which provider capabilities are actually available for the configured model/profile surface.
  - Degrade unsupported features cleanly instead of failing late.
  - Use the capability model to inform later caching and orchestration work.
- Acceptance notes:
  - Unsupported features are surfaced before a run relies on them.
  - Capability differences are visible in config/runtime behavior.
  - The implementation stays aligned with the public config contract.
- Roadmap anchors:
  - docs/roadmap.md: Add provider capability detection so unsupported features degrade cleanly instead of failing late
### RB-17 — Research and implement provider-aware prompt/token caching behavior
- GitHub issue: [#84](https://github.com/urbanbreach/agent-harness/issues/84)
- Milestone batch: Batch 3 — Models, providers, and config ergonomics
- Dependency order: RB-16
- Scope:
  - Check the available provider documentation for OpenAI, Google, Anthropic, Qwen, Kimi, GLM, and Minimax to understand prompt/token caching behavior.
  - Implement provider-aware handling based on documented behavior instead of one-size-fits-all assumptions.
  - Keep the result bounded to documented caching behavior and the harness config/runtime surface.
- Acceptance notes:
  - The supported providers have documented caching behavior notes recorded in-repo.
  - Runtime behavior follows provider-aware handling where the docs support it.
  - Unknown provider behavior is treated explicitly instead of guessed.
- Roadmap anchors:
  - docs/roadmap.md: Figure out actual prompt and token caching behavior ... then implement provider-aware handling
### RB-18 — Add a first-boot CLI flow for working config setup
- GitHub issue: [#85](https://github.com/urbanbreach/agent-harness/issues/85)
- Milestone batch: Batch 3 — Models, providers, and config ergonomics
- Dependency order: RB-14, RB-15, RB-16
- Scope:
  - Get users from install to a working config with minimal friction using a first-boot CLI flow.
  - Keep the setup flow aligned with the blessed default path and supported provider/model/preset surfaces.
  - Use the shipped example config as a base rather than introducing a second configuration story.
- Acceptance notes:
  - A new user can reach a working config from the CLI without repo-specific guesswork.
  - The flow stays aligned with the documented default path.
  - The resulting config validates through the public schema path.
- Roadmap anchors:
  - docs/roadmap.md: Set up a first-boot CLI flow so users can get from install to a working config with minimal friction
  - docs/config.md; docs/plan-build-workflow.md
### RB-19 — Ship stronger Plan/Build agent profiles and main-agent prompts
- GitHub issue: [#86](https://github.com/urbanbreach/agent-harness/issues/86)
- Milestone batch: Batch 4 — Agent, orchestration, and prompt quality foundations
- Dependency order: RB-01
- Scope:
  - Add fleshed-out agent profiles for the main flows such as Plan and Build.
  - Improve the main-agent prompt engineering in the pi-like direction called out by the roadmap.
  - Keep the profiles legible and aligned with the existing plan -> build workflow.
- Acceptance notes:
  - Plan and Build profiles have explicit prompt/behavior contracts.
  - The prompt work improves the shipped main flow instead of adding detached experiments.
  - Docs/config stay aligned with the resulting profile surface.
- Roadmap anchors:
  - docs/roadmap.md: Add agent profiles with fleshed-out system prompts for main flows such as Build and Plan; improve main-agent prompt engineering with a pi-like approach
  - docs/plan-build-workflow.md
### RB-20 — Add JSON-configured subagents and improve subagent prompts
- GitHub issue: [#87](https://github.com/urbanbreach/agent-harness/issues/87)
- Milestone batch: Batch 4 — Agent, orchestration, and prompt quality foundations
- Dependency order: RB-19
- Scope:
  - Expose subagent configuration through JSON/config.
  - Improve subagent prompt engineering so delegated work remains legible and bounded.
  - Keep subagent behavior inside the current coordinator and policy boundaries.
- Acceptance notes:
  - Subagent configuration exists on the supported config surface.
  - Subagent prompts are good enough for the intended delegated flows.
  - The implementation does not bypass existing policy boundaries.
- Roadmap anchors:
  - docs/roadmap.md: Flesh out subagents with configs available in JSON; improve subagent prompt engineering
### RB-21 — Add legible orchestration controls with approval and policy visibility
- GitHub issue: [#88](https://github.com/urbanbreach/agent-harness/issues/88)
- Milestone batch: Batch 4 — Agent, orchestration, and prompt quality foundations
- Dependency order: RB-13, RB-19, RB-20
- Scope:
  - Add orchestration functionality that can be toggled from the HUD while staying understandable.
  - Surface approval and policy controls for orchestration/tool execution so multi-agent runs remain predictable.
  - Keep the control surface inside the current shell contract instead of inventing a second orchestration UI.
- Acceptance notes:
  - Users can see and control whether orchestration is active.
  - Approval/policy state is visible during orchestrated runs.
  - The orchestration controls remain legible in the main shell.
- Roadmap anchors:
  - docs/roadmap.md: Add orchestration functionality that is toggleable in the HUD; add approval and policy controls for orchestration/tool execution
### RB-22 — Add swarm and Ralph-style orchestration loops within the harness shell contract
- GitHub issue: [#89](https://github.com/urbanbreach/agent-harness/issues/89)
- Milestone batch: Batch 4 — Agent, orchestration, and prompt quality foundations
- Dependency order: RB-20, RB-21
- Scope:
  - Add swarms and Ralph loops inspired by oh-my-codex.
  - Keep the implementation aligned with the harness shell contract and the legibility requirements established earlier.
  - Avoid exceeding the current shell contract with speculative workflow layers.
- Acceptance notes:
  - Swarm/Ralph-style loops exist as part of the harness orchestration surface.
  - They respect the approval/policy controls from the prior issue.
  - The result stays understandable in the main shell.
- Roadmap anchors:
  - docs/roadmap.md: Add swarms and Ralph loops inspired by oh-my-codex
### RB-23 — Add the expected slash-command surface
- GitHub issue: [#90](https://github.com/urbanbreach/agent-harness/issues/90)
- Milestone batch: Batch 5 — Commands, panes, and workflow surfaces
- Dependency order: RB-12, RB-19
- Scope:
  - Add the / commands users expect from Opencode-like tools.
  - Keep the command contract aligned with the Plan/Build flow and the commands menu.
  - Use one coherent command system rather than scattered entrypoints.
- Acceptance notes:
  - The shipped / command surface covers the intended baseline workflows.
  - The commands are discoverable from the menu and shell.
  - The implementation fits the current workflow contract.
- Roadmap anchors:
  - docs/roadmap.md: Add / commands that users expect from tools like Opencode
### RB-24 — Add the dollar-command workflow surface
- GitHub issue: [#91](https://github.com/urbanbreach/agent-harness/issues/91)
- Milestone batch: Batch 5 — Commands, panes, and workflow surfaces
- Dependency order: RB-19, RB-21, RB-23
- Scope:
  - Add $ workflow commands inspired by oh-my-codex.
  - Keep the command surface understandable and consistent with the orchestration controls.
  - Treat the $ commands as part of the main shell contract, not a separate expert-only side channel.
- Acceptance notes:
  - The shipped $ commands cover the intended workflow hooks.
  - The shell explains what the commands do and when they apply.
  - The surface stays aligned with the existing command system.
- Roadmap anchors:
  - docs/roadmap.md: Add $ commands inspired by oh-my-codex
### RB-25 — Add tmux-backed subagent panes configurable from config
- GitHub issue: [#92](https://github.com/urbanbreach/agent-harness/issues/92)
- Milestone batch: Batch 5 — Commands, panes, and workflow surfaces
- Dependency order: RB-18, RB-20, RB-21
- Scope:
  - Add tmux support with subagent panes that can be configured from config.
  - Keep pane behavior aligned with the orchestration controls and policy model.
  - Avoid hidden pane behavior that splits the user mental model.
- Acceptance notes:
  - Tmux-backed subagent panes can be configured from the supported config surface.
  - Pane behavior remains legible from the main shell.
  - The pane system respects orchestration and policy settings.
- Roadmap anchors:
  - docs/roadmap.md: Add tmux support with subagent panes, configurable from config
### RB-26 — Make command discovery and execution obvious across the session flow
- GitHub issue: [#93](https://github.com/urbanbreach/agent-harness/issues/93)
- Milestone batch: Batch 5 — Commands, panes, and workflow surfaces
- Dependency order: RB-12, RB-23, RB-24, RB-25
- Scope:
  - Tighten command discovery and execution so users can find and run the right command from anywhere in the session flow.
  - Unify menu, HUD, slash-command, dollar-command, and pane entrypoints into one understandable interaction model.
  - Treat discoverability as a top-level workflow concern rather than a polish-only follow-up.
- Acceptance notes:
  - Users can find the relevant command surface without prior repo knowledge.
  - The discovery path feels fast and obvious in the main shell.
  - The result ties together the command/pane surfaces instead of multiplying them.
- Roadmap anchors:
  - docs/roadmap.md: Make command discovery and execution feel fast and obvious from anywhere in the session flow
### RB-27 — Flesh out skills support
- GitHub issue: [#94](https://github.com/urbanbreach/agent-harness/issues/94)
- Milestone batch: Batch 6 — Skills, plugins, and extensibility
- Dependency order: RB-18, RB-24
- Scope:
  - Expand the harness skills surface beyond the current starter pack.
  - Keep skills aligned with the command and configuration surfaces rather than as ad hoc hidden behavior.
  - Use the existing skill roots and permission model as the base contract.
- Acceptance notes:
  - Skills support is meaningfully more complete than the starter baseline.
  - The resulting behavior is documented and discoverable.
  - Skill execution remains consistent with the permission model.
- Roadmap anchors:
  - docs/roadmap.md: Flesh out skills support
  - docs/starter-skills.md; docs/config.md
### RB-28 — Decide which advanced features are core versus optional plugins
- GitHub issue: [#95](https://github.com/urbanbreach/agent-harness/issues/95)
- Milestone batch: Batch 6 — Skills, plugins, and extensibility
- Dependency order: RB-01, RB-27
- Scope:
  - Decide which advanced features belong in core and which should ship as optional plugins.
  - Ground the boundary in the roadmap and the core parity work rather than speculative platform design.
  - Use the decision to constrain the plugin surface before implementation expands.
- Acceptance notes:
  - The core-vs-plugin boundary is documented in-repo.
  - The decision does not add new feature scope beyond the roadmap.
  - Later plugin issues have a clear contract to implement against.
- Roadmap anchors:
  - docs/roadmap.md: Decide which advanced features should be core and which should be shipped as optional plugins
### RB-29 — Add plugin support with a full disable path
- GitHub issue: [#96](https://github.com/urbanbreach/agent-harness/issues/96)
- Milestone batch: Batch 6 — Skills, plugins, and extensibility
- Dependency order: RB-18, RB-28
- Scope:
  - Add plugin support to the harness runtime.
  - Provide a supported way to disable plugin-backed features completely.
  - Keep the implementation bounded by the core-vs-plugin decision and current pure-Rust guardrails.
- Acceptance notes:
  - Plugin-backed features can be enabled or disabled through the supported surface.
  - The disable path fully removes plugin-backed behavior where promised.
  - The implementation follows the in-repo boundary decision.
- Roadmap anchors:
  - docs/roadmap.md: Add plugin support, with the option to disable plugin-backed features completely
### RB-30 — Add first-class Openclaw support
- GitHub issue: [#97](https://github.com/urbanbreach/agent-harness/issues/97)
- Milestone batch: Batch 6 — Skills, plugins, and extensibility
- Dependency order: RB-25, RB-27, RB-29
- Scope:
  - Add first-class Openclaw support in the spirit of oh-my-codex and oh-my-openagent.
  - Build it on top of the settled skills/plugin/pane foundations rather than as a one-off integration.
  - Keep the scope within the roadmap without widening into unrelated platform work.
- Acceptance notes:
  - Openclaw support exists as a first-class supported surface.
  - The feature respects the surrounding config, command, and plugin contracts.
  - The implementation stays grounded in the roadmap-defined scope.
- Roadmap anchors:
  - docs/roadmap.md: Add first-class Openclaw support similar in spirit to oh-my-codex and oh-my-openagent
