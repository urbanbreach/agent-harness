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

### P2 — tie parity to verification

- map each canonical journey to existing deterministic PTY/live signoff where possible
- add missing signoff coverage for the top parity-critical journeys
- make visual and transcript evidence part of the acceptance criteria

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
