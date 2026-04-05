# Agent Harness parity scorecard

## Purpose

This document defines the **first work package** for Agent Harness.

Before adding more opinionated workflow layers, the harness should have a clear, testable answer to: **what does “Opencode-class core parity” actually mean here?**

This scorecard is meant to drive implementation, signoff, and prioritization.

## The first thing to start with

Start with a **core parity pass** built around three outputs:

1. a canonical set of user journeys
2. a blessed default path in config and docs
3. a ranked gap list tied to concrete UX/runtime evidence

Do **not** start with multi-provider expansion or a generalized opinionated-pack system.

## Canonical user journeys

These are the workflows that should feel complete before the harness claims strong core parity.

### 1. First successful prompt run

User can install/configure the harness, launch it, run a prompt, and understand what happened without prior repo knowledge.

Success means:

- the default config path is obvious
- the default profile/provider/model choice is obvious
- startup and first-run UX are legible
- tool activity, permissions, and completion state are understandable

### 2. Transcript-first live session

User can follow an active run without feeling lost.

Success means:

- active and completed tool rows are visually distinct
- inline metadata is useful
- disclosure controls are good enough for dense sessions
- failures and pending states are visible without log-diving

### 3. Permission-handling flow

User can understand why a permission was requested, what was allowed or denied, and what effect that had.

Success means:

- allow / deny / ask behavior is visible in both TUI and headless flows
- blocked actions produce understandable evidence
- permission resolution does not feel like hidden state

### 4. Continue-session and recovery flow

User can come back to work and continue confidently.

Success means:

- session list / reopen / continue are easy to navigate
- artifacts and prior tool results are discoverable
- child-session relationships are understandable
- replay is legible enough to recover context quickly

### 5. Tool-heavy run inspection

User can inspect shell, edit, search, and generic tool calls without the transcript collapsing into noise.

Success means:

- inline tool summaries are informative
- expanded views are available where needed
- diff presentation is useful
- generic-tool fallback remains readable and not lossy

## Blessed default path

The product story should revolve around one clearly recommended path:

- documented default provider: `default` from `configs/harness.example.jsonc`
- documented provider transport: local CLIProxy-compatible loopback endpoint at `http://127.0.0.1:8317/v1`
- documented default profile: `plan`
- documented handoff profile: `build`
- documented default model/variant pairing: `gpt-5.4-mini` for the primary plan/build flow, with `gpt-5.4-mini` + `deterministic` reserved for the secondary `tool_audit` signoff lane
- documented first-run command path: `cargo run -p harness -- --config configs/harness.example.jsonc`

Profile classification should stay explicit:

- primary profiles: `plan`, `build`
- secondary profiles: `tool_audit`, `deep_compat`

Secondary or migration-oriented paths should stay visibly secondary:

- audit-oriented profiles (`tool_audit`)
- secondary compat/regression profiles (`deep_compat`)
- experimentation-oriented optional flows

## Ranked parity backlog

This is the order I would use for the first implementation slice.

### P0 — define and lock the target

- turn the five canonical journeys above into explicit signoff targets
- align `README.md`, `docs/roadmap.md`, and `configs/harness.example.jsonc` around one blessed default path
- decide which current profiles are primary versus secondary

### P1 — close the most user-visible TUI/session gaps

Using the shipped parity docs and current native PTY/live evidence as the source of truth, prioritize:

- per-item transcript disclosure depth
- active/running tool-row polish
- richer inline title grammar and metadata
- diff controls where transcript diffs matter
- clearer timing, duration, and state visibility
- artifact and child-session discoverability

#### Current evidence baseline by journey

1. **First successful prompt run** — `startup_shell`, `startup_command_palette`, and `live_shell` are already part of the shipped PTY evidence contract (`crates/harness-testkit/tests/support/visual_contracts.rs:9-87`), and `crates/harness-testkit/tests/snapshots/pty_e2e__pty_interactive_type_first_startup.snap:5-12` proves the current startup shell can show a draft plus the `Ctrl+p open` affordance. The remaining gap is not basic startup existence; it is the richer first-run composer / command / sidebar contract still called out in `docs/roadmap.md:42-49,74-77`.
2. **Transcript-first live session** — `transcript_shell` is a first-class offline evidence family (`crates/harness-testkit/tests/support/visual_contracts.rs:100-117`), and the dense/task snapshots prove inline timestamps, durations, tool output, and child-call metadata today (`crates/harness-testkit/tests/snapshots/pty_e2e__native_tool_parity_dense.snap:6-16`; `crates/harness-testkit/tests/snapshots/pty_e2e__native_tool_parity_task_row.snap:5-14`). The biggest remaining gap is disclosure/state richness, not transcript existence.
3. **Permission-handling flow** — the shipped PTY parity snapshot already proves a visible blocked-state contract with preserved draft text (`crates/harness-testkit/tests/snapshots/pty_e2e__pty_permission_overlay_parity.snap:5-14`). This journey still needs live/headless signoff mapping later, but it does not currently outrank the transcript and recovery gaps.
4. **Continue-session and recovery flow** — the offline evidence contract already includes `continue_session`, `replay_shell`, `replay`, and `operator_sidebar` families (`crates/harness-testkit/tests/support/visual_contracts.rs:40-135`). The current snapshots prove rejection and read-only states (`crates/harness-testkit/tests/snapshots/pty_e2e__pty_continue_rejected_active.snap:5-12`; `crates/harness-testkit/tests/snapshots/pty_e2e__pty_replay_read_only.snap:5-14`), but discoverability is still behind the roadmap target.
5. **Tool-heavy run inspection** — the dense transcript parity snapshots already show inline attachments, tool names, command text, timing, and child-session summaries (`crates/harness-testkit/tests/snapshots/pty_e2e__native_tool_parity_dense.snap:6-16`; `crates/harness-testkit/tests/snapshots/pty_e2e__native_tool_parity_task_row.snap:5-14`). The remaining mismatch is that inspection still lacks some Opencode-class disclosure, diff, and presentation affordances.

#### Current ranked gaps against canonical evidence

1. **Blocker — deepen transcript disclosure, diff controls, and run-state visibility before further shell polish.**
   - **Journeys:** 2. Transcript-first live session; 5. Tool-heavy run inspection
   - **Why ranked here:** this is the highest-frequency surface in the current shell, and it is the gap most directly called out by both the scorecard and the Opencode audit.
   - **Proof today:**
     - the scorecard says these journeys succeed only when active/completed rows are distinct, metadata is useful, disclosure works in dense sessions, and diff presentation is usable (`docs/parity-scorecard.md:36-77`)
     - the audit still calls out missing disclosure depth, active-row polish, diff-wrap controls, and plainer thinking treatment (`opencode-parity-audit.md:45-120,190-197,241-245`)
     - the shipped PTY snapshots prove the current baseline has inline timing and child-call metadata, but they also show the proof stops at that simpler contract today (`crates/harness-testkit/tests/snapshots/pty_e2e__native_tool_parity_dense.snap:6-16`; `crates/harness-testkit/tests/snapshots/pty_e2e__native_tool_parity_task_row.snap:5-14`)
   - **Follow-on lanes:** RB-10 first, then RB-09 and RB-08.

2. **Blocker — make recovery, replay, artifacts, and child-session navigation low-friction instead of merely present.**
   - **Journeys:** 4. Continue-session and recovery flow; 5. Tool-heavy run inspection
   - **Why ranked here:** the repo already proves recovery states exist, so the remaining gap is now discoverability and navigation quality — exactly the difference users feel when returning to prior work.
   - **Proof today:**
     - the scorecard requires easy reopen/continue flows, discoverable artifacts/tool results, understandable child-session relationships, and legible replay (`docs/parity-scorecard.md:57-77`)
     - the shipped PTY evidence contract already covers continue, replay, child-navigation, and operator-sidebar families (`crates/harness-testkit/tests/support/visual_contracts.rs:40-135`)
     - the current snapshots only prove the baseline states like `tasks are still in flight` and `Replay is read-only`, while the roadmap still leaves sidebar parity, modified-files visibility, and low-friction recovery explicitly open (`crates/harness-testkit/tests/snapshots/pty_e2e__pty_continue_rejected_active.snap:5-12`; `crates/harness-testkit/tests/snapshots/pty_e2e__pty_replay_read_only.snap:5-14`; `docs/roadmap.md:42-52`)
     - the audit says child-session metadata now exists inline, but richer affordance and discoverability headroom remains (`opencode-parity-audit.md:165-173,203-211`)
   - **Follow-on lanes:** RB-11 first, then RB-06.

3. **Blocker — broaden live-provider signoff beyond the narrow chat-control and file-edit lanes.**
   - **Journeys:** 1-5
   - **Why ranked here:** parity claims are supposed to be provable with both deterministic PTY evidence and live-provider validation, but the shipped live guidance is still narrower than the five canonical journeys.
   - **Proof today:**
     - the roadmap still calls for live-provider tool verification, CLIProxy-backed live tests, CLI+TUI parity coverage, and provider-difference tracking (`docs/roadmap.md:32-37`)
     - the testing guide documents a broad offline PTY evidence set, additive live visual/chat-control signoff lanes, and the still-explicit live gaps in the journey matrix (`docs/testing.md`, sections `Agent-visible visual artifacts`, `Additive live visual signoff`, `Live chat-control signoff`, and `Canonical journey signoff expectations`)
     - the shipped live-proxy guide centers on `live_proxy_prompt_chat_tool_flow`, `live_proxy_e2e_tui_tool_flow`, and `live_proxy_e2e_visual_verifier`; it does not yet map the five canonical journeys to live signoff artifacts (`crates/harness-testkit/tests/README.live-proxy.md:38-73,116-126,148-159`)
   - **Follow-on lanes:** RB-03, then RB-04 and RB-05.

4. **Blocker — upgrade first-run discovery from a working startup shell to an obvious Opencode-class command/composer/sidebar contract.**
   - **Journeys:** 1. First successful prompt run; 2. Transcript-first live session
   - **Why ranked here:** the first-run shell exists, but the remaining misses are still in the entry surfaces that determine whether a new user understands how to act without repo context.
   - **Proof today:**
     - the scorecard says first-run success requires an obvious default path plus legible startup, tool activity, permissions, and completion state (`docs/parity-scorecard.md:25-35`)
     - the startup PTY snapshot proves only the current draft-first shell and `Ctrl+p open` affordance (`crates/harness-testkit/tests/snapshots/pty_e2e__pty_interactive_type_first_startup.snap:5-12`)
     - the roadmap still leaves the sidebar, under-input area, commands menu, and command discovery unchecked (`docs/roadmap.md:42-49,74-77`)
     - the testing guide lists startup and command-palette checkpoints and now spells out the first-run signoff expectation, while still calling out the missing end-to-end live oracle for the whole journey (`docs/testing.md`, sections `Agent-visible visual artifacts` and `Canonical journey signoff expectations`)
   - **Follow-on lanes:** RB-07 first, then RB-12 and RB-06.

5. **Polish — keep improving transcript layout richness and tool/title semantics, not the already-landed transcript-first structure.**
   - **Journeys:** 2. Transcript-first live session; 5. Tool-heavy run inspection
   - **Why ranked here:** the canonical screenshots now prove the transcript-first structure is basically right, so the remaining work is about richness and polish rather than rebuilding the shell.
   - **Proof today:**
     - the audit says dense transcript-first screenshots are no longer the primary problem; the remaining delta is block richness, tool-specific affordances, and title/icon polish (`opencode-parity-audit.md:146-153,215-245`)
     - the dense/task snapshots already prove inline density, timing, and child-call summaries (`crates/harness-testkit/tests/snapshots/pty_e2e__native_tool_parity_dense.snap:6-16`; `crates/harness-testkit/tests/snapshots/pty_e2e__native_tool_parity_task_row.snap:5-14`)
   - **Follow-on lanes:** RB-08.

6. **Polish — make thinking traces read like a first-class transcript surface.**
   - **Journeys:** 2. Transcript-first live session
   - **Why ranked here:** this is a visible Opencode delta, but it is narrower than the disclosure/recovery/signoff gaps above it.
   - **Proof today:**
     - the roadmap still has an explicit thinking-trace parity checkbox (`docs/roadmap.md:47`)
     - the audit says harness thinking is still structurally plainer even though the global toggle now exists (`opencode-parity-audit.md:190-197`)
   - **Follow-on lanes:** RB-09.

### P2 — tie parity to verification

- map each canonical journey to existing deterministic PTY/live signoff where possible
- add missing signoff coverage for the top parity-critical journeys
- make visual and transcript evidence part of the acceptance criteria

#### Canonical journey signoff map

`docs/testing.md` owns the detailed CLI/TUI signoff matrix. The current journey-level map is:

| Journey | Deterministic PTY baseline | Existing live signoff | Explicit gap |
| --- | --- | --- | --- |
| 1. First successful prompt run | `startup_shell`, `startup_command_palette`, `live_shell` | CLI `live_proxy_prompt_responses_smoke`; TUI `live_proxy_preflight`, `live_proxy_e2e_tui_prompt_responses_smoke`, `live_proxy_e2e_visual_verifier` | No live lane yet proves first-run command palette/sidebar/under-input discoverability. |
| 2. Transcript-first live session | `transcript_shell` | CLI `live_proxy_prompt_chat_tool_flow`; TUI `live_proxy_preflight`, `live_proxy_e2e_tui_tool_flow`, `live_proxy_e2e_visual_verifier` | No dedicated live dense-transcript oracle yet matches the PTY disclosure/state/diff contract. |
| 3. Permission-handling flow | `permission` | No current live CLI or TUI permission ask/allow/deny lane | Missing live CLI and live TUI permission-request coverage. |
| 4. Continue-session and recovery flow | `startup_session_history`, `continue_session`, `replay_shell`, `replay`, `operator_sidebar` | No current live CLI or TUI continue/replay/reopen lane | Recovery, replay, artifact discovery, and child-session navigation are still PTY-only for parity signoff. |
| 5. Tool-heavy run inspection | `transcript_shell`, `replay_shell`, `operator_sidebar` | CLI `live_proxy_prompt_chat_tool_flow`; TUI `live_proxy_preflight`, `live_proxy_e2e_tui_tool_flow`, `live_proxy_e2e_visual_verifier` | No dedicated live replay/inspection lane yet covers dense tool disclosure, diff presentation, or child-session/tool-artifact inspection. |

For parity-critical changes, PTY PNG/snapshot evidence and live transcript/manifest artifacts are
acceptance criteria, not debugging extras.

## What should wait

Until the scorecard above is in good shape, defer:

- multi-provider work
- generalized pack abstractions
- large memory systems
- broad delegation frameworks
- highly custom orchestration surfaces that exceed the current shell contract

## Exit condition for the first work package

The first work package is done when:

- the core parity target is written down clearly
- the default path is unambiguous
- the highest-value parity gaps are ranked
- the top journeys have matching signoff plans

At that point, the next sensible move is to execute the parity backlog — not to jump early into optional opinionated packs.
