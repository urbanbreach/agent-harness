# Harness / Grok Build Parity PRD, Binding Specification, and Execution Plan

> **Status:** Binding implementation contract. This supersedes prior completion claims.
> **Scope:** The entire Harness product surface required for parity: TUI, CLI handoff, coordinator, sessions, workspaces, providers, authentication, tools, permissions, persistence, integrations, and parity evidence.
> **Done when:** Every acceptance gate in this document passes on the same current revision, every advertised Harness action works end-to-end, and every required Grok Build capability has a complete Harness-native implementation or a user-approved divergence.
> **Not done when:** The result is a topology change, a glyph/color reskin, an accepted snapshot refresh, or marker-only PTY evidence.
> **First actions:** Re-audit the tree, inventory every visible action and backend capability, reproduce known functional defects, freeze the reference, author the traceability manifest, then implement journey-by-journey.
> **Never:** Copy reference source/tests/harnesses or allow evidence to self-certify.
> **Stop:** Only under the Stop Rule at the end of this contract.

## Part I — Execution Plan

This part is the implementer's primary entrypoint. Execute the phases in order. The detailed specification in Part II remains binding and authoritative when a checklist item needs exact requirements, tolerances, field definitions, or evidence rules.

Checkboxes are execution controls, not self-authored completion claims. Machine-readable manifests, current-revision artifacts, test results, external postconditions, and independent review remain the source of truth.

### Checklist status

- `[ ]` — incomplete; work or evidence is still required.
- `[x]` — pass; all referenced exit criteria are satisfied on the current revision and evidence paths are recorded.
- `BLOCKED` — cannot proceed because required external evidence, environment, or a genuine invariant conflict is unavailable. Blocked is not pass.
- `DIVERGED` — the user explicitly approved the exact divergence and its ID is recorded. Diverged is not global parity.

Never mark a capability, journey, subsystem, configuration area, surface, or gate `[x]` because it has a type, registry entry, status line, seeded probe, unavailable result, unit test, mock transport, fixture, snapshot, or diagnostic summary. Completion requires the real public operation, external outcome, recovery behavior, and required evidence stack.

### Execution flow

```text
Phase 0  Mission lock and current-state reset
  -> Phase 1  Clean-room authority and hard invariants
  -> Phase 2  Reference freeze and deterministic capture lab
  -> Phase 3  Manifest, capability, subsystem, and config inventories
  -> Phase 4  Fail-closed evidence infrastructure
  -> Phase 5  P0 functional defects and real backend journeys
  -> Phase 6  Complete product capability implementation
  -> Phase 7  UI/UX, responsive, and animation parity
  -> Phase 8  Cell, pixel, trace, and timing closure
  -> Phase 9  Same-revision validation, coverage, and dogfood
  -> Phase 10 Independent review, final report, and stop decision
```

Do not skip forward because later work is easier. A phase may overlap only where explicitly allowed, and no later phase can certify an earlier incomplete exit gate.

### Master completion checklist

The goal is complete only when every item below is `[x]` on one fresh current revision:

- [ ] Every required capability, action, journey, subsystem, setting, schema, and manifest row has independent reference evidence.
- [ ] Every row has real backend, state, interaction, render, PTY, differential, side-effect, rollback, and recovery owners as applicable.
- [ ] The complete manifest passes without sampling, skipped states, reused smoke owners, or empty evidence layers.
- [ ] Every visible shortcut, menu entry, action row, picker option, slash command, tool, mouse target, and gesture invokes the exact advertised operation through the compiled Harness product.
- [ ] Every existing Harness subsystem was compared with the reference and deliberately replaced, reworked, retained as a seam, or retained with reference proof.
- [ ] Every required Grok Build capability is implemented completely or has an exact user-approved divergence.
- [ ] Configuration, settings, schemas, discovery, layering, migrations, effective values, capability validation, diagnostics, and redaction satisfy the config/schema contract.
- [ ] All known user-reported functional and visual defects are closed with regression and real-surface evidence.
- [ ] Semantic terminal cells have zero unapproved differences.
- [ ] Settled xterm.js frames have zero unapproved RGBA differences.
- [ ] Canonical interaction and animation traces have no missing, extra, or reordered frames.
- [ ] Timed behavior satisfies the frozen timing contract.
- [ ] Harness authority, event, replay, permission, cancellation, redaction, and lifecycle invariants remain green.
- [ ] The seventeen deleted broad visual coverage classes are restored or replaced one-for-one with equal-or-stronger owners.
- [ ] Two independent reviewers approve fresh clean-checkout evidence and holdouts on the same revision.
- [ ] No visual surface is merely old Harness behavior with changed punctuation, color, glyphs, or placement.
- [ ] No production control, capability, provider, integration, or settings entry is a placeholder, unrelated dispatch, mock-only success path, or structured-unavailable substitute for the required product.
- [ ] The mandatory final report is complete and its claim wording matches the actual approved masks or divergences.

### Acceptance-gate dashboard

- [ ] `A-MANIFEST` — every required row is complete and linked to executable evidence.
- [ ] `A-REFERENCE` — the pinned corpus and environment are deterministic across three reference runs.
- [ ] `A-CAPABILITIES` — every required capability works through its real public surface.
- [ ] `A-CORE-AUDIT` — every affected first-party subsystem is compared and deliberately classified.
- [ ] `A-CONFIG-SCHEMA` — public configuration and schema behavior is complete, secure, deterministic, and capability-aware.
- [ ] `A-FUNCTIONAL` — every advertised action performs the exact operation it advertises.
- [ ] `A-JOURNEYS` — complete compiled-product journeys reach correct final and recovery states.
- [ ] `A-STATE` — Harness state and interaction transitions match.
- [ ] `A-CELLS` — zero unapproved styled-cell differences.
- [ ] `A-PIXELS` — zero unapproved settled RGBA differences.
- [ ] `A-TRACE` — no missing, extra, or reordered canonical frames.
- [ ] `A-TIMING` — frozen timing behavior remains within bounds.
- [ ] `A-ANIMATION` — motion, spinner, cursor, progress, and disclosure choreography matches.
- [ ] `A-PTY` — the real Harness binary passes fail-closed PTY journeys.
- [ ] `A-INVARIANTS` — runtime authority and safety remain intact.
- [ ] `A-COVERAGE` — deleted visual coverage is restored or replaced one-for-one.
- [ ] `A-REVIEW` — independent visual, clean-room, and holdout review passes.
- [ ] `A-NO-RESKIN` — every surface is a complete experience rather than a reskin.

### Phase 0 — Mission lock and current-state reset

**Entry:** This document is opened as the binding implementation authority.

**Checklist:**

- [ ] Read the root and scoped `AGENTS.md` files, `docs/testing.md`, signoff manifest, and lane scripts listed in Detailed Specification §5.
- [ ] Load the repository-required coding skills before editing code, tests, schemas, scripts, or guidance.
- [ ] Record current branch, HEAD, merge-base, recent commits, full worktree status, scoped diff statistics, and unrelated concurrent changes.
- [ ] Treat Detailed Specification §6 as historical diagnostic context only; refresh every baseline fact before relying on it.
- [ ] Inventory all currently known user-visible bugs, dead shortcuts, wrong dispatches, stale transitions, placeholder capabilities, probe-only surfaces, and quality-gate failures.
- [ ] Convert every known functional defect into a P0 behavior/capability/journey row with a failing regression owner.
- [ ] Confirm that ordinary implementation decisions are autonomous and only destructive actions or genuine unresolved product conflicts require user input.

**Exit:** The current tree and defect state are known, stale completion claims are rejected, unrelated work is protected, and all known P0 defects are traceable.

### Phase 1 — Clean-room authority and hard invariants

**Entry:** Phase 0 exit is satisfied.

**Checklist:**

- [ ] Adopt the four-layer strategy: black-box reference observation, independent specification, Harness-native implementation/tests, and external differential acceptance.
- [ ] Record Grok Build as the primary product reference and OpenCode as the secondary config/schema ergonomics reference.
- [ ] Confirm no first-party production code or ordinary test links to, imports, reads, translates, or mechanically transforms reference source, tests, fixtures, snapshots, themes, identifiers, architecture, PTY harnesses, mock servers, or `xai-grok-*` crates.
- [ ] Freeze the authority pipeline: terminal input → local TUI state/action → at most one mutating `UiIntent` → CLI/coordinator → coordinator-owned work → append-only events → replay/projection → render.
- [ ] Freeze the hard invariants: coordinator authority, replay purity, contiguous event order, permission-before-execution, cancellation precedence, redaction, no `events.jsonl` rewrite during compaction, product identity, canonical tool/permission/session semantics, and runtime/TUI config separation.
- [ ] Confirm the TUI does not execute providers, tools, shell, network, hooks, compaction, scheduling, permissions, event append, or replay mutation directly.
- [ ] Record any genuine invariant conflict as `BLOCKED`; only the user can approve the exact divergence.

**Exit:** Clean-room boundaries, product authority, redesign freedom, and divergence policy are explicit and testable.

### Phase 2 — Reference freeze and deterministic capture lab

**Entry:** Phase 1 exit is satisfied and required reference environments are available.

**Checklist:**

- [ ] Record the complete reference receipt: binary/source/config-reference revisions and digests, Harness revision, manifest/scenario digests, OS/container, terminal parser, Chromium, xterm.js, renderer, fonts, sizing, DPR, locale, Unicode width, color modes, viewports, themes, workspaces, provider/tool/permission/question inputs, clocks, randomness, traces, config layers, and migrations.
- [ ] Run at least three isolated reference-versus-reference captures.
- [ ] Prove untimed semantic cells and settled PNGs are identical across those runs.
- [ ] Stabilize nondeterministic scenarios with controlled inputs or mark them `BLOCKED` with a documented reason.
- [ ] Freeze timed public contracts before comparing Harness; use a controlled clock where possible.
- [ ] Register unavoidable identity or dynamic masks before implementation. Default is no mask.
- [ ] Confirm masks are field-level, symmetric, exact, independently reviewed, and never cover geometry, spacing, borders, icons, color, focus, cursor, selection, interactive chrome, whole components, or arbitrary regions.

**Exit:** `A-REFERENCE` is green and every implementation row has a stable reference oracle.

### Phase 3 — Manifest, capability, subsystem, and config inventories

**Entry:** Phase 2 is complete, or a corpus custodian is producing frozen rows independently while the implementer works only on already frozen rows.

**Checklist:**

- [ ] Create or refresh the machine-readable parity manifest using the complete traceability schema in Detailed Specification §§4.2 and 9.
- [ ] Inventory every reference-visible action, shortcut, command, menu item, picker option, mouse target, state, transition, side effect, capability, and recovery path.
- [ ] Inventory every affected first-party Harness subsystem and classify it as `replace`, `rework`, `retain-seam-only`, or `retain-with-reference-proof`.
- [ ] Cover coordinator, CLI, sessions, tools, providers, permissions, TUI, compaction, workspaces, worktrees, sandbox, trust, memory, scheduling, teams, plugins, ACP, remote workspace, MCP OAuth, provider/auth lifecycle, updates, code intelligence, and terminal capability families.
- [ ] Create explicit worktree rows covering the complete minimum contract in Detailed Specification §4.6.
- [ ] Inventory every public setting and schema with stable ids, scope, layer, merge strategy, default, sensitivity, capability dependency, owner, restart behavior, migration, deprecation, and evidence owners.
- [ ] Cover every surface/state/journey generator in Detailed Specification §10 rather than a sampled subset.
- [ ] Ensure the corpus custodian is not the sole implementer or sole acceptance reviewer.
- [ ] Keep row statuses strictly `incomplete`, `blocked`, `pass`, or `diverged`; do not use prose such as “foundation complete” as a pass substitute.

**Exit:** The manifest is structurally complete, every required product/config/UI item has a row, and all subsystem/config dispositions are authored before implementation.

### Phase 4 — Fail-closed evidence infrastructure

**Entry:** Frozen reference rows and the manifest schema exist.

**Checklist:**

- [ ] Correct every audited signoff defect in Detailed Specification §18.
- [ ] Build one strict parity lane that fails for missing environment support, binaries, artifacts, scenarios, checkpoints, timeouts, assertions, owner tests, or review stages.
- [ ] Remove fail-open `|| true` behavior from parity-signoff paths.
- [ ] Capture full semantic cells: grapheme, width/continuation, foreground, background, modifiers, hyperlinks, cursor, dimensions, alternate screen, scroll, selection, focus, wrap, mouse, paste, and enhanced-key modes.
- [ ] Ensure expected cells come from the frozen reference or explicit Harness invariant, never Harness output compared with itself.
- [ ] Drive the real compiled Harness product through PTY journeys and inspect external postconditions.
- [ ] Provide equivalent isolated launch adapters for reference and Harness without scenario branching or synthesized success.
- [ ] Pin full-frame xterm.js/Chromium capture with identical fonts, geometry, DPR, theme, locale, and renderer.
- [ ] Restore or define one-for-one replacements for the seventeen deleted broad visual coverage classes.
- [ ] Produce a machine-readable rollup linking each row, gate, layer, command, artifact, and result.

**Exit:** L0–L5 infrastructure exists, fails closed, and cannot certify labels, probes, markers, mocks, or environment metadata as product parity.

### Phase 5 — P0 functional defects and real backend journeys

**Entry:** The relevant rows are frozen and the evidence lane can fail them correctly.

**Checklist:**

- [ ] Close `P0-START-01`, `P0-START-02`, `P0-START-03`, `P0-COMP-01`, and `P0-KEY-01` through real compiled-product journeys.
- [ ] Make `Ctrl+W` create and enter a real isolated worktree session rather than editing composer text or dispatching another action.
- [ ] Make `Ctrl+S` open the real picker, load replay-derived session state, enter the live shell, and remove startup content.
- [ ] Close every known dead shortcut, wrong dispatch, stale startup/overlay state, placeholder command, and unrelated palette action.
- [ ] Verify every visible action through TUI action, `UiIntent`, CLI/coordinator/backend owner, external outcome, rollback, cancellation, recovery, and rendered final state.
- [ ] Verify every affected CLI command and native tool has a real happy path and meaningful failure path.
- [ ] Prohibit synthetic destination `AppState`, event injection, fixture-only success, status banners, seeded probes, or unavailable outcomes as sole journey evidence.

**Exit:** `A-FUNCTIONAL`, P0 portions of `A-JOURNEYS`, and the advertised-control portion of `A-STATE` are green. No known functional defect remains ahead of cosmetic work.

### Phase 6 — Complete product capability implementation

**Entry:** Phase 5 is complete. Functional-first ordering remains binding for every new row.

For each capability row, execute the row loop below before selecting an unrelated row:

- [ ] Capture and freeze the exact reference state and public behavior.
- [ ] Add an independently authored failing state, interaction, backend, side-effect, or recovery test.
- [ ] Add frozen semantic-cell expectations where the capability renders in the terminal.
- [ ] Implement the real backend capability in the correct architectural owner.
- [ ] Wire the public TUI, CLI, tool, protocol, or background action to that owner.
- [ ] Implement success, invalid input, denial, cancellation, interruption, restart, rollback, and recovery as applicable.
- [ ] Pass focused unit and real integration owners.
- [ ] Pass the compiled-product journey and verify its external postcondition.
- [ ] Pass the real PTY trace.
- [ ] Pass semantic differential comparison for the row.
- [ ] Pass fixed-tick animation and settled pixel comparison for the row.
- [ ] Run adjacent states, responsive boundaries, capability fallbacks, and error paths.
- [ ] Record artifacts, provenance, residual risk, and updated manifest paths.
- [ ] Mark the row `pass` only after every required layer and gate is satisfied.

Capability-family completion checklist:

- [ ] Existing Harness subsystem audit and reference-backed replacement/rework.
- [ ] Worktree lifecycle, isolation, ownership, resume/fork/clone/export/recovery/deletion, and CoW fallback.
- [ ] OS sandbox enforcement, folder trust, supported VCS/Jujutsu behavior, and edit attribution.
- [ ] Atomic rewind, durable memory, foreign-session import, queue/interjection, and crash recovery.
- [ ] Scheduling, monitor, wait-any/all, foreground demotion, teams/mailbox/process/worktree lifecycle, and task cleanup.
- [ ] Plugin lifecycle/marketplace/integrations, ACP, remote workspace/hub, MCP OAuth/transports, and hooks.
- [ ] Provider protocols, fallback, authentication/OIDC, sleep/wake refresh, updates, and restart recovery.
- [ ] Persistent code intelligence and rich terminal input/copy/hyperlink/selection behavior.
- [ ] Complete config/schema product: generated schemas, layers, precedence, merge behavior, migrations, effective values, source explanation, capability diagnostics, secret handling, worktree/session scopes, and writable CLI/TUI settings journeys.

**Exit:** `A-CAPABILITIES`, `A-CORE-AUDIT`, `A-CONFIG-SCHEMA`, `A-FUNCTIONAL`, `A-JOURNEYS`, and backend portions of `A-STATE` are green with real product depth, not foundations or probes.

### Phase 7 — UI/UX, responsive, and animation parity

**Entry:** The backend operation for each active surface row works end-to-end.

**Checklist:**

- [ ] Create or refresh `crates/harness-tui/DESIGN.md` from measured reference captures before full-screen replacement.
- [ ] Define shell order, breakpoints, spacing, component anatomy, overlay geometry/z-order, border/glyph/color roles, modifiers, focus states, cursor, animations, transitions, fallbacks, and identity substitutions.
- [ ] Render complete primitive state sets in a showcase/fixture harness before composing the full shell.
- [ ] Apply the module disposition to layout, welcome, chrome, transcript, tools, markdown, syntax, diff, composer, permissions, questions, overlays, secondary surfaces, theme, focus, scrolling, selection, folding, resizing, mouse, and terminal capability presentation.
- [ ] Cover startup/welcome, primary shell, transcript/blocks, composer, overlays, backend journeys, config journeys, responsive/capability states, and animation/dynamic states listed in Detailed Specification §10.
- [ ] Cover required viewports and terminal capabilities: `120x50`, `120x40`, `100x30`, `80x24`, `79x24`, `60x20`, reference extremes, a width above 120, truecolor/reduced color, enhanced/legacy keys, mouse, and clipboard.
- [ ] Match spinner cadence, cursor transitions, overlay/disclosure/focus motion, progress, queue, interruption, retry, reconnect, recovery, and reduced-capability fallbacks.
- [ ] Confirm no component is an old Harness surface with only glyph, punctuation, color, or placement changes.

**Exit:** `A-NO-RESKIN`, UI portions of `A-STATE`, responsive portions of `A-JOURNEYS`, and implementation portions of `A-ANIMATION` are green.

### Phase 8 — Cell, pixel, trace, and timing closure

**Entry:** The active rows are functionally complete and rendered through live components.

**Checklist:**

- [ ] Resolve all semantic-cell differences in dimensions, graphemes, widths, colors, modifiers, hyperlinks, cursor, and emulator state.
- [ ] Capture full-frame settled PNGs; do not crop favorable regions.
- [ ] Require identical image dimensions and zero unapproved RGBA differences.
- [ ] Compare fixed canonical renderer ticks for dynamic states separately from settled frames.
- [ ] Require no missing, extra, or reordered canonical frames.
- [ ] Define settled as scripted external events complete plus three consecutive unchanged semantic-cell ticks.
- [ ] Treat a scenario deadline overrun as failure, never permission to skip.
- [ ] For unavoidable real-time behavior, run at least 30 trials and keep median and p95 within `max(one renderer tick, 10%)` of the frozen reference.
- [ ] Never widen timing bounds or masks after seeing Harness failures.
- [ ] Qualify all identity/dynamic divergences exactly; never claim global pixel identity when any remain.

**Exit:** `A-CELLS`, `A-PIXELS`, `A-TRACE`, `A-TIMING`, and `A-ANIMATION` are green.

### Phase 9 — Same-revision validation, coverage, and dogfood

**Entry:** Every manifest row is claimed pass or has an exact user-approved divergence with evidence.

**Checklist:**

- [ ] Freeze one identifiable current revision and fresh evidence root.
- [ ] Run `cargo fmt --all --check`.
- [ ] Run `cargo nextest run -p harness-tui`.
- [ ] Run `cargo nextest run -p harness-core`.
- [ ] Run `cargo nextest run -p harness`.
- [ ] Run `cargo nextest run -p harness-tools`.
- [ ] Run `scripts/test-lanes.sh quality-gates`.
- [ ] Run `scripts/test-lanes.sh all-deterministic`.
- [ ] Run `scripts/test-lanes.sh signoff-pty`.
- [ ] Run `bash scripts/harness-qa-dogfood.sh --self-test`.
- [ ] Run the strict differential parity lane and complete xterm.js suite.
- [ ] Prove real Git worktree creation, entry, isolation, resume, rollback, and cleanup.
- [ ] Prove startup-to-worktree, startup-to-resume, startup-to-first-turn, cancellation, and recovery through the compiled TUI.
- [ ] Prove every shortcut in every applicable focus state and every palette/slash entry.
- [ ] Prove all affected CLI/tool happy and failure paths, subsystem migrations/recovery, setting/schema journeys, required integrations, and animation ticks.
- [ ] Prove generated schemas, docs, examples, manifests, and migration tests are synchronized.
- [ ] Prove the seventeen deleted broad snapshot classes are restored or replaced one-for-one.
- [ ] Confirm the complete stack used the same source revision, manifest digest, environment digest, and reference digest.

**Exit:** `A-INVARIANTS`, `A-PTY`, `A-COVERAGE`, the final `A-MANIFEST` rollup, and every same-revision automated gate are green.

### Phase 10 — Independent review, final report, and stop decision

**Entry:** All automated gates pass from a clean checkout of the same revision.

**Checklist:**

- [ ] An independent visual reviewer inspects every reference/actual pair and every automated hotspot or mismatch.
- [ ] An independent code/clean-room reviewer confirms live Harness components, no embedded captures/hashes/test branches, and no copied source or test architecture.
- [ ] An evaluator runs undisclosed holdouts for Unicode widths, long content, resizing, scrolling, timing perturbations, and error paths.
- [ ] Reviewer disagreement is resolved; disagreement blocks completion.
- [ ] The mandatory final report includes every field in Detailed Specification §21.
- [ ] The final report lists exact commands, artifacts, digests, counts, masks, divergences, failed/unavailable lanes, defect closure, and provenance.
- [ ] The final claim uses the exact allowed wording and is qualified when identity fields, masks, or divergences remain.
- [ ] Re-read the master completion checklist and acceptance-gate dashboard against fresh evidence.

**Stop decision:**

- [ ] Stop only if every capability, action, journey, setting, schema, subsystem, manifest row, evidence layer, acceptance gate, invariant, review, and holdout is green on the same current revision; every affected Harness function is operational and polished; no known defect remains; and no visible control is a placeholder or wrong dispatch.
- [ ] Otherwise continue. A cleaner shell, removed sidebar, updated snapshots, passing build, passing subset, a sample reviewer approval, missing time, large scope, absent backend architecture, unavailable fixtures, or a visually convincing substitute is not a stop condition.
- [ ] The only alternative stop is an exact invariant conflict with an exact user-approved divergence.

## Part II — Detailed Binding Specification

The sections below preserve the complete detailed contract. They define the exact fields, capability floors, state matrices, evidence layers, tolerances, prohibitions, validation commands, report fields, and stop language referenced by Part I. Part I controls execution order; Part II controls requirement detail. Neither weakens the other.

### 0. Document Control

| Field | Contract |
|---|---|
| Document ID | `tui-reference-parity-contract` |
| Reference | Pinned Grok Build TUI, observed through isolated black-box execution |
| Product under change | The Harness product, including Ratatui TUI, CLI/coordinator handoff, workspaces, sessions, providers, tools, integrations, persistence, and owned parity/signoff infrastructure |
| Required claim | Complete functional, interaction, animation, and observable parity under the frozen environment and manifest, with only explicitly user-approved identity or product divergences |
| Does not replace | `docs/testing.md`, `docs/tui-signoff-manifest.v1.json`, event/config contracts, or crate `AGENTS.md` guidance |
| Evidence root | Fresh, provenance-bearing artifact directory for each acceptance run |
| Revision policy | Re-audit the working tree before trusting any baseline or previous evidence |

### 1. Executive Summary

#### Problem

Previous implementation attempts made the Harness shell wider and changed selected glyphs, colors, permissions, and key paths, but the TUI still rendered as a modified Harness interface. Existing tests mostly proved Harness against itself and did not prove reference visual or UX parity.

#### Outcome

Replace and extend the Harness product with a clean-room Harness-native implementation that matches the pinned reference across capabilities, screens, actions, state transitions, terminal cells, rendered pixels, input traces, animations, timing, responsive behavior, and real external side effects while preserving Harness identity and runtime authority.

This is not a TUI reskin. If the reference exposes a functional feature through the TUI, command palette, CLI, session flow, tool surface, or background runtime, Harness must implement that feature completely enough to work through its real public surface. Rendering the feature name without the underlying behavior is a failure.

#### Success metric

The complete capability-and-journey traceability manifest passes all L0-L6 evidence layers, all A-* acceptance gates, all Harness safety/invariant tests, real side-effect verification, and two independent reviews using fresh captures from the current revision.

#### Primary constraint

Reference behavior is an external observable contract. Reference source, tests, fixtures, PTY harnesses, themes, identifiers, and architecture are not implementation material.

### 2. Goals, Non-Goals, and Decision Rules

#### 2.1 Goals

The implementation shall:

- Match reference shell geometry, density, typography, glyphs, colors, borders, focus, cursor, selection, overlays, and responsive degradation.
- Match composer, transcript, tool, permission, question, queue, palette, session, status, and recovery choreography.
- Match startup-to-live transitions, streaming phases, cancellation, scrolling, resizing, and interaction timing where observable.
- Match every reference-visible action and backend-supported workflow with a real Harness-native implementation, including worktrees, session transitions, background operations, integrations, and recovery.
- Make every existing Harness function reachable through its intended surface work fully, without placeholders, stale presentation, dead shortcuts, test-only paths, or degraded behavior hidden behind Grok-like chrome.
- Match reference motion, animation, progress indicators, spinners, disclosure transitions, cursor behavior, and renderer-tick choreography where observable.
- Make configuration and schema behavior as polished and capability-complete as the product: OpenCode-style public config ergonomics, Grok-style typed settings/scopes/capability registry, and Harness-owned generated schemas, security, and authority.
- Produce independent, reproducible, machine-checkable evidence rather than self-authored visual claims.
- Preserve only the non-negotiable Harness safety and authority invariants: coordinator ownership, append-only event truth, replay purity, permission-before-execution, cancellation precedence, redaction, identity, and runtime/TUI configuration separation. Existing implementations, module boundaries, workflows, and user-visible behavior are not protected from reference-backed replacement.

#### 2.2 Non-goals and false completion

The following are explicitly insufficient:

- A full-width transcript.
- A bottom composer.
- Replacing `┃` with `❯`.
- Replacing tool icons with `◆`.
- Recoloring the old theme magenta.
- Deleting card borders.
- Renaming sidebar state to secondary surfaces.
- Updating snapshots to current output.
- Adding tests whose names contain `parity` or `P0`.
- Passing marker-based PTY smoke tests.
- Rendering a visible action whose shortcut invokes a different operation.
- Routing a reference feature to a generic placeholder, status banner, no-op, or unrelated Harness action.
- Constructing the destination `AppState` directly instead of proving the real startup, CLI, coordinator, filesystem, provider, or session transition.
- Declaring a missing backend capability out of scope because it requires changes outside `crates/harness-tui`.
- Editing the signoff manifest.
- Passing the Rust build.

#### 2.3 Decision rules

- Ordinary implementation decisions are autonomous.
- A missing reference capture, browser, font, PTY, or reviewer blocks completion; it never becomes a silent skip.
- A real Harness invariant may justify a documented divergence, but implementation convenience may not.
- A reference-visible feature that Harness lacks is implementation work, not permission to display a placeholder or silently substitute another operation.
- Only the user may approve omission or substitution of a required product capability. Until then, the capability remains incomplete or blocked.
- Known functional defects outrank visual polish. Do not work on antialiasing, one-cell packing, masks, or cosmetic timing while an advertised action is dead, misrouted, stale, or unowned end-to-end.
- A finite manifest proves only the states it enumerates. Never claim universal parity from a sample.

### 3. Implementer Role and Execution Mode

You are the autonomous senior Rust engineer responsible for extending and replacing the relevant Harness product surfaces with a clean-room implementation that matches the Grok Build CLI product as exactly as its public behavior can be measured.

Operate in:

`/home/urbanbreach/Projects/agent-harness`

This is an implementation mission, not a planning exercise. Inspect, specify, implement, test, run, compare, fix, and repeat until the acceptance contract in this prompt is satisfied.

Do not ask the user to approve ordinary implementation decisions. Ask only if a destructive action or a genuine product conflict cannot be resolved from the authority rules below.

#### Binding Objective

Deliver a Harness-native product whose externally observable capabilities, UI, UX, side effects, animations, and recovery behavior match the pinned Grok Build reference across every required feature, screen, state, transition, input trace, viewport, terminal capability, and backend journey in the parity manifest.

The target includes:

- Terminal-cell geometry.
- Component hierarchy.
- Colors and text modifiers.
- Borders, separators, glyphs, padding, density, and vertical rhythm.
- Cursor shape, position, and visibility.
- Focus, selection, scroll, fold, disclosure, and overlay state.
- Composer editing, history, slash, file mention, queue, submit, and cancellation behavior.
- Transcript, tool, permission, question, error, streaming, completion, and recovery presentation.
- Keyboard and mouse choreography.
- Resize and compact-terminal behavior.
- Frame ordering and observable timing for dynamic states.
- Real filesystem, Git/worktree, session, provider, tool, permission, queue, background-task, integration, and persistence outcomes behind every advertised action.
- Audit every existing Harness subsystem against the reference, then retain, rework, or replace it according to which design produces the more complete, reliable, and polished behavior without violating hard Harness invariants.

Harness product identity and runtime authority remain Harness-owned. Identity substitutions must preserve the reference region's geometry, style, animation, and interaction. Only the exact identity glyphs or text may differ.

### 4. Completion and Acceptance Contract

Completion requires all of the following on the same current revision:

1. Every required capability, action, journey, and parity-manifest row has independently captured reference evidence.
2. Every row has Harness-native backend, state, interaction, deterministic render, real-surface, and side-effect owners appropriate to that behavior.
3. The complete manifest passes without sampling or skipped states.
4. Semantic terminal-cell comparison has zero unapproved differences.
5. Settled xterm.js pixel comparison has zero unapproved RGBA differences under the pinned renderer environment.
6. Required interaction traces have no missing, extra, or reordered canonical states.
7. Timed interactions satisfy the frozen timing contract.
8. Harness core, permission, replay, cancellation, event, redaction, and lifecycle invariants remain green.
9. Two independent reviewers approve fresh evidence from a clean checkout.
10. No current visual surface remains merely an old Harness component with changed punctuation, color, or placement.
11. Every visible shortcut, menu entry, action row, picker option, slash command, tool, and gesture invokes the operation it advertises through the real Harness binary.
12. Every existing Harness capability has been compared with the equivalent reference behavior and deliberately retained, reworked, or replaced; the chosen result is complete, reachable, migrated where necessary, and polished to the reference standard.
13. Every required Grok Build capability in the capability inventory has a complete Harness-native implementation and end-to-end journey, unless the user explicitly approved the exact divergence.
14. No known user-reported functional or visual defect remains open.
15. Configuration, settings, schema, migration, discovery, layering, effective-value, capability-validation, and diagnostics behavior passes the dedicated config/schema acceptance gate.

If a required reference state has not been captured, the corresponding row is incomplete. If evidence is unavailable, the work is blocked, not complete.

#### 4.1 Acceptance gates

| Gate | Requirement | Evidence required |
|---|---|---|
| `A-MANIFEST` | Every required row is complete; no sampling or skipped states | Machine-readable manifest and rollup |
| `A-REFERENCE` | Reference corpus is pinned and ref-vs-ref deterministic | Receipt plus three identical reference runs |
| `A-CAPABILITIES` | Required Grok Build and existing Harness capabilities are implemented completely | Capability inventory, backend owners, and side-effect receipts |
| `A-CORE-AUDIT` | Every affected first-party Harness subsystem was compared against the reference and deliberately classified | Subsystem matrix, comparison receipts, replacement/rework tests, and migration evidence |
| `A-CONFIG-SCHEMA` | Public configuration and schema behavior is complete, deterministic, secure, and capability-aware | Generated schemas, settings registry, layered-config receipts, migration tests, effective-config output, and real CLI/TUI journeys |
| `A-FUNCTIONAL` | Every advertised action performs the exact advertised operation | State, intent, coordinator, filesystem/network, and result evidence |
| `A-JOURNEYS` | Complete real-binary user journeys reach the correct final state | Journey traces from startup through side effects and recovery |
| `A-STATE` | Harness state and interaction transitions match | Harness-native state/intent tests |
| `A-CELLS` | Zero unapproved semantic-cell differences | Styled cell grids and diff report |
| `A-PIXELS` | Zero unapproved settled RGBA differences | Same-pipeline xterm.js PNG diff |
| `A-TRACE` | No missing, extra, or reordered interaction frames | Trace and frame-sequence report |
| `A-TIMING` | Frozen timed behavior remains within bounds | Repeated timing report |
| `A-ANIMATION` | Motion, spinner, progress, cursor, and disclosure frame choreography matches | Fixed-tick frame sequences and settled-state reports |
| `A-PTY` | Real Harness binary works through a PTY | Fail-closed PTY artifacts and cleanup receipt |
| `A-INVARIANTS` | Harness runtime authority and safety remain intact | Owner nextest and lane results |
| `A-COVERAGE` | Deleted visual coverage is restored or replaced one-for-one | Snapshot/test replacement matrix |
| `A-REVIEW` | Independent visual and clean-room reviewers approve fresh evidence | Reviewer reports and holdout results |
| `A-NO-RESKIN` | No surface is only a punctuation, color, or placement change | Module disposition plus source review |

#### 4.2 Traceability row

Every requirement row must join the following chain:

```text
behavior_id
  -> requirement_id and priority
  -> capability_id and visible_action_id
  -> setting_id / schema_id where configuration participates
  -> reference_receipt_id
  -> input trace, seeds, viewport, and expected states
  -> Harness TUI action and UiIntent
  -> CLI/coordinator/backend owner
  -> required external side effect and rollback behavior
  -> Harness fixture/state/render/PTY/differential owners
  -> L1-L6 evidence paths
  -> acceptance_gate_ids
  -> status: incomplete | blocked | pass | diverged
```

The manifest row is the single join key. The final report must roll up required, passed, blocked, and deliberately diverged rows.

#### 4.3 Functional control invariant

Every visible action, shortcut, menu entry, picker option, slash command, button-equivalent row, mouse target, and documented gesture must execute a real Harness-owned operation end-to-end.

A control is incomplete if it:

- Only renders the reference label, icon, shortcut, or animation.
- Dispatches to an unrelated action because the intended backend is absent.
- Returns a placeholder banner, simulated success, canned fixture, or no-op.
- Works only in unit tests, injected event streams, capture mode, or a synthetic `AppState`.
- Mutates local TUI state without completing the advertised CLI, coordinator, filesystem, provider, session, tool, or integration operation.
- Leaves stale welcome, overlay, focus, cursor, selection, or transcript state after the operation completes.

Examples of binding behavior:

- `New worktree · Ctrl+W` creates an isolated Git worktree, creates or selects the associated Harness session, enters the worktree, renders the live shell, and removes the welcome panel.
- `Resume session · Ctrl+S` opens the real session picker, loads the selected replay-derived session, restores its supported state, enters the live shell, and removes startup content.
- A visible tool, MCP, LSP, task, settings, model, permission, or session command must invoke its real owner and render success, failure, cancellation, and recovery truthfully.

Text-presence tests do not prove a control. Constructing the destination state directly does not prove the journey.

#### 4.4 Capability completeness and divergence policy

The required reference feature set is every user-visible or publicly callable capability exposed by the pinned Grok Build binary, its TUI, CLI, agent protocol, tool surface, workspace runtime, or documented user workflow. Internal xAI administration, private telemetry, and service-only operations that are not exposed to users are not required unless they affect observable behavior.

For each required capability:

1. Implement the complete Harness-native behavior through the correct architectural owner.
2. Expose it through the matching TUI, CLI, tool, protocol, or background surface.
3. Cover success, invalid input, permission denial, cancellation, interruption, restart, and recovery where applicable.
4. Prove the external outcome rather than only internal dispatch.

If a genuine Harness invariant conflicts with the reference, record the exact conflict and ask the user to approve the exact divergence. Until approval, the row is blocked. The implementer may not silently omit, rename, substitute, or cosmetically imitate the capability.

#### 4.5 Required backend and product capability inventory

The manifest must include at least the following capability families. This list is a floor, not a ceiling; black-box discovery may add more.

##### Existing Harness subsystem audit and replacement

Audit every existing Harness function and subsystem, not only surfaces already represented in the parity manifest. For each subsystem, capture the current Harness behavior, measure the equivalent Grok Build behavior, and classify the implementation as `replace`, `rework`, `retain-seam-only`, or `retain-with-reference-proof`.

Existing behavior is not automatically the contract. When the reference design is more complete, reliable, coherent, recoverable, or polished, implement the reference behavior clean-room in Harness and migrate persisted state or external contracts only where real compatibility requires it. The implementer may decide the internal decomposition autonomously.

The audit must cover at least:

- Coordinator-owned provider, tool, permission, question, task, hook, compaction, cancellation, and lifecycle behavior.
- CLI `tui`, `run`, `prompt`, `doctor`, `auth`, `models`, `replay`, `sessions`, schema, and config-validation flows.
- Session list, inspect, reopen, continue, replay, export, tree, fork, clone, title, lineage, resume, crash-tail repair, snapshot, and revert behavior.
- Native filesystem, edit, bash, task, background, batch, question, skill, todo, plan, web, code search, LSP, MCP, session, and GitHub tool behavior.
- Provider routing, model selection, authentication, streaming, retries, redaction, and error presentation.
- Permission rules, external-directory safety, deny/ask/allow decisions, durable grants, and cancellation precedence.
- TUI composer, history, slash, mentions, queue, focus, selection, copy, scroll, overlays, secondary surfaces, mouse, terminal restoration, and responsive behavior.

For each retained subsystem, evidence must show that retaining it is at least as capable and polished as the measured reference. Familiarity, existing tests, lower diff size, or fear of migration are not retention reasons.

Compaction is explicitly redesignable. Compare reference and Harness triggers, context accounting, summarization, checkpoint/state representation, interruption, retry, recovery, transcript markers, user control, and provider handoff. Rework or replace Harness compaction to match the better reference behavior while preserving only append-only event truth, replay purity, redaction, and compatibility with already persisted sessions where required.

##### Workspace, isolation, and version control

- Real Git worktree creation, naming, collision handling, listing, selection, entry, apply/sync where reference-visible, cleanup, and failure rollback.
- Worktree-aware session creation, fork, resume, recovery, and concurrent isolation.
- Fast or copy-on-write worktree behavior where observable, with safe fallback on unsupported filesystems.
- OS-level sandbox profiles and child-process/network restrictions matching the public reference modes.
- Persistent folder trust for repository-local executable configuration.
- Reference-supported version-control workflows, including Jujutsu where exposed.
- Attribution of agent/tool edits versus external edits where exposed by status, diff, or recovery surfaces.

##### Sessions, memory, and persistence

- Prompt-level rewind that restores both conversation and file state atomically.
- Durable cross-session memory, indexing, retrieval, search, update, flush, and reference-equivalent memory-management workflows.
- Foreign-session discovery/import for supported coding agents where exposed by the reference.
- Prompt queue persistence, ordering, cancellation, send-now, and multi-client semantics where observable.
- Mid-turn user interjection and queued-input delivery without corrupting the active turn.
- Native crash detection, previous-crash reporting, deterministic restart, and recovery UX.

##### Agents, background work, and orchestration

- Recurring scheduled work and loop workflows.
- Monitoring and control of long-running commands, tools, and agents.
- Wait-any and wait-all synchronization for multiple background operations.
- Foreground shell-command demotion to background where reference-visible.
- Full multi-agent/team behavior exposed by the reference, including mailbox, process, workspace, worktree, cancellation, and lifecycle coordination.
- Complete task/subagent status, result, interruption, retry, and cleanup behavior.

##### Integrations and extension surfaces

- Runtime plugin installation, validation, activation, deactivation, upgrade, removal, and marketplace workflows for reference-visible plugin types.
- Plugin-provided skills, commands, agents, hooks, MCP, and LSP integrations.
- ACP agent mode over the reference-supported transports for IDE/editor integration.
- Remote workspace and computer-hub connection, binding, tool execution, upload, disconnect, and recovery behavior where exposed.
- MCP OAuth and reference-supported remote transports and lifecycle behavior.
- File-discovered, plugin-provided, and pre-tool blocking hooks where reference-visible.

##### Providers, identity, and platform lifecycle

- Reference-visible provider protocols and model-selection behavior beyond the existing OpenAI-compatible path.
- Automatic provider/model fallback behavior where observable, with exact error and retry semantics.
- Generic browser/device authentication and enterprise OIDC/SSO where exposed.
- Sleep/wake-aware credential refresh where it affects observable reliability.
- Binary update, minimum-version enforcement, update failure recovery, and restart behavior where exposed.

##### Code intelligence and terminal input

- Persistent incremental codebase graph and navigation where exposed by the reference.
- Clipboard, hyperlink, selection, and rich terminal capability paths present in the reference.

#### 4.6 Worktree minimum product contract

Worktrees are mandatory, not a decorative startup row.

At minimum:

- `Ctrl+W` from startup invokes worktree creation rather than composer editing.
- The operation validates that the workspace is a supported repository and reports actionable errors otherwise.
- Worktree path and branch naming are deterministic, collision-safe, and visible to the user.
- Partial creation is rolled back on failure.
- The new session is rooted in the worktree and all path-safety, permissions, tools, LSP, MCP, hooks, snapshots, and replay use that root.
- Resume, fork, clone, export, recovery, and deletion preserve or safely resolve worktree ownership.
- Concurrent worktree sessions do not share mutable session state or tool working directories accidentally.
- PTY evidence verifies the worktree on disk, active branch/path, session association, live-shell transition, and startup-panel removal.

#### 4.7 Functional-first execution order

Work in this order:

1. Reproduce and lock every known user-visible functional defect.
2. Complete advertised actions and backend capabilities.
3. Complete state transitions, failure handling, cancellation, and recovery.
4. Complete animations, frame choreography, responsive behavior, and accessibility.
5. Only then close cell, color, antialiasing, and pixel residuals.

Do not spend a work loop on antialiasing, mask tuning, one-cell spacing, or cosmetic snapshot churn while a known shortcut invokes the wrong action, a session transition leaves stale UI, a backend capability is missing, or a real-binary journey lacks evidence.

#### 4.8 Configuration and schema parity contract

Configuration is product behavior, not incidental plumbing. A feature is incomplete if it works only through hard-coded defaults, cannot be configured through its intended public surface, produces an inaccurate schema, or silently ignores an invalid or unavailable setting.

Use a deliberate hybrid:

- **OpenCode-style public ergonomics:** JSON/JSONC, `$schema`, global/project layering, provider/model/agent/permission/MCP structure, explicit migrations, normalization, and actionable diagnostics.
- **Grok-style semantic settings:** a typed settings registry with stable ids, scopes, defaults, visibility, mutability, capability dependencies, restart requirements, and effective-value inspection.
- **Harness-owned authority:** Rust typed definitions, generated schemas, secure credential handling, coordinator-owned side effects, runtime/TUI separation, redaction, and replay-safe configuration metadata.

Do not copy source code or product-specific configuration names. Reimplement the observable contract clean-room and adapt it to Harness identity and authority.

Every public setting must have:

- Stable `setting_id` and `schema_id`.
- A typed value and generated JSON Schema representation.
- Default value and explicit absence semantics.
- Scope and layer: `system`, `user`, `profile`, `project`, `workspace`, `worktree`, `session`, `command-line`, or environment overlay as applicable.
- Deterministic merge strategy and precedence.
- Sensitivity/redaction classification.
- Capability dependency and unavailable-platform behavior.
- Runtime owner, TUI/CLI surface, and restart requirement.
- Migration, deprecation, conflict, and removal behavior.
- Unit, schema, integration, and real-surface evidence owners.

The effective configuration must be inspectable and explainable. The product must provide an equivalent of:

```text
harness config show --effective
harness config explain <setting-or-path>
harness config sources
```

with secrets redacted and each resolved value attributed to its source layer.

Required configuration behavior:

- Canonical generated schemas for runtime, TUI, effective settings, providers/models, permissions, extensions/plugins, workspaces/worktrees, and other public capability manifests.
- Deterministic discovery and precedence for global, user, project, workspace/worktree, session, command-line, and environment layers.
- Explicit merge behavior for scalars, maps, lists, rules, provider catalogs, agent definitions, and keybindings.
- Strict unknown-field handling after migration normalization; no silently ignored active configuration.
- Conflict errors when canonical fields and compatibility aliases specify different values.
- Versioned migrations with round-trip, rejection, deprecation, and rollback tests.
- Capability-aware validation that distinguishes schema-invalid, semantically invalid, unavailable, permission-blocked, unauthenticated, and valid-but-inactive settings.
- Config validation must be side-effect free; `doctor` may report capability/auth availability but must not silently execute providers, tools, MCP, hooks, or network operations.
- Runtime config and TUI config remain separate public contracts, but share the settings registry, discovery model, diagnostics, and schema generation infrastructure.
- Credentials, tokens, cookies, private keys, and provider secrets never persist in config files, schemas, effective-config output, events, or evidence.
- Worktree/session-scoped settings resolve against the active worktree/session rather than the process working directory by accident.
- Settings shown in the TUI and CLI must reflect effective values, inherited values, overrides, unavailable capabilities, and reset/revert actions accurately.

The config/schema audit must compare current Harness behavior with both the pinned Grok Build reference and the current OpenCode reference. Existing Harness config behavior may be replaced or reworked when the reference behavior is more coherent, expressive, or reliable, subject only to the hard authority and security invariants.

The audit must cover at least:

- Runtime/TUI file discovery and project-root traversal.
- Provider and model registries, variants, fallback metadata, transport capability, and authentication references.
- Agent/profile/category definitions, tools, permissions, prompts, skills, and inheritance.
- Permission rules, selector precedence, safety defaults, and per-worktree/session overrides.
- MCP, LSP, hooks, plugins/extensions, commands, and capability availability.
- Worktree, workspace trust, sandbox, memory, scheduler, queue, and session settings.
- Config aliases, legacy compatibility paths, schema versions, migration diagnostics, and deprecation policy.
- Effective-config inspection, `doctor`, editor schema integration, redaction, and failure output.

No feature may be marked complete until its config/schema contract, effective-value behavior, and real operational journey agree.

### 5. Read First

Read the current versions of:

- `AGENTS.md`
- `crates/harness-tui/AGENTS.md`
- `crates/harness-tui/src/app/AGENTS.md`
- `crates/harness-testkit/AGENTS.md`
- `crates/harness-testkit/tests/AGENTS.md`
- `crates/harness-core/AGENTS.md`
- `crates/harness-core/src/coord/AGENTS.md`
- `docs/AGENTS.md`
- `docs/testing.md`
- `docs/tui-signoff-manifest.v1.json`
- `scripts/AGENTS.md`
- `scripts/test-lanes.sh`

Load the repository-required coding skills before editing Rust, scripts, tests, schemas, or AGENTS guidance.

### 6. Historical Audited Starting Point — Refresh Before Use

Do not trust prior completion claims. Re-run the audit because the tree may have moved.

The latest audit found:

- Branch: `ui-ux-experiments`.
- Committed `HEAD`: `ed95105d`.
- Audited `dev` and merge-base: `f54f7c2d`.
- The committed branch primarily removed the persistent sidebar and made the transcript and composer full-width.
- Approximately three quarters of committed line churn was tests, snapshots, documentation, or evidence wiring rather than replacement of observable renderers.
- The working tree then added a large uncommitted reskin attempt.
- The reskin changed composer punctuation, transcript rails, tool markers, magenta accents, permission chrome, some overlays, and several input behaviors.
- It did not replace the complete layout, transcript, tool, markdown, diff, overlay, secondary-surface, or state architecture.
- Seventeen broad startup, stream, permission, narrow, split, and tool-lifecycle snapshots were deleted while narrower reskinned snapshots were accepted.

Known superficial changes in the current on-disk attempt include:

- Composer rail and separator grammar reduced to a lone `❯`.
- Tool headers forced to `◆` while residual `◈`, `$`, and other old glyph paths remain.
- Card glyphs blanked and accent colors shifted to a magenta family.
- User transcript rails hidden without replacing the underlying transcript surface model.
- Model and session surfaces collapsed into palette-like chrome.
- Permission choices restyled with `●` and `○`.
- Ctrl+C, double-Escape, PageUp/PageDown, and focus behavior partially changed.

Known inherited or only partially changed areas include:

- `crates/harness-tui/src/layout.rs`
- `crates/harness-tui/src/ui.rs`
- `crates/harness-tui/src/ui_lifecycle.rs`
- `crates/harness-tui/src/ui_secondary.rs`
- `crates/harness-tui/src/ui_markdown.rs`
- `crates/harness-tui/src/ui_tool_titles.rs`
- `crates/harness-tui/src/ui_tool_output.rs`
- `crates/harness-tui/src/ui_diff_render.rs`
- `crates/harness-tui/src/ui_transcript_layout.rs`
- `crates/harness-tui/src/ui_overlays/auth_dialog.rs`
- `crates/harness-tui/src/ui_subagent_footer.rs`
- `crates/harness-tui/src/ui_terminal.rs`
- `crates/harness-tui/src/app/composer.rs`
- `crates/harness-tui/src/app/permissions.rs`
- `crates/harness-tui/src/app/secondary_surfaces.rs`
- `crates/harness-tui/src/keybindings.rs`
- `crates/harness-tui/src/overlay.rs`

Treat the committed topology change and dirty reskin as a failed partial implementation. Retain only proven Harness authority/safety seams and useful behavioral coverage; compare all other implementation and presentation choices against the reference before deciding whether to keep them. Do not preserve old behavior simply to reduce the diff.

Before editing, run and inspect the equivalent of:

```bash
git branch --show-current
git status --short
git merge-base HEAD dev
git log --oneline --reverse dev..HEAD
git diff --stat dev -- crates/harness-tui crates/harness/src/tui crates/harness-core/src docs/tui-signoff-manifest.v1.json scripts/test-lanes.sh
git diff --name-status dev -- crates/harness-tui crates/harness/src/tui crates/harness-core/src docs/tui-signoff-manifest.v1.json scripts/test-lanes.sh
git diff --stat HEAD -- crates/harness-tui docs/tui-signoff-manifest.v1.json scripts/test-lanes.sh
git diff --name-status HEAD -- crates/harness-tui docs/tui-signoff-manifest.v1.json scripts/test-lanes.sh
```

Do not overwrite unrelated working-tree changes.

#### Measured reference comparison: audited Harness versus Grok

The following fresh captures were produced through the same PTY, xterm.js, Chromium, font, 120x32-cell, and device-pixel-ratio pipeline. They are diagnostic evidence for this contract, not acceptance goldens.

| State | Harness evidence | Grok evidence | Measured result |
|---|---|---|---|
| Startup | `.../evidence/harness-startup/` | `.../evidence/grok-startup/` | Same PNG dimensions: 2448x1152. Raw PNG diff: 253,377 pixels, `diffRatio=0.0898`, `similarityScore=91`. |
| Typed draft | `.../evidence/harness-draft/` | `.../evidence/grok-draft/` | Same PNG dimensions. Raw PNG diff: 206,126 pixels, `diffRatio=0.0731`, `similarityScore=93`. |
| Harness startup text | `terminal.txt` | - | 32 rows; centered HARNESS logo, onboarding hints, bare prompt, model line, bottom status. |
| Grok startup text | - | `terminal.txt` | 31 visible rows; breadcrumb, clipboard warning, bordered welcome panel, logo, changelog, four action rows, bordered composer, login footer. |
| Harness draft transition | `terminal.txt` | - | Typing changes the composer text and footer hint, but the welcome logo and onboarding content remain visible. |
| Grok draft transition | - | `terminal.txt` | Typing dismisses the welcome panel; the body clears, the bordered composer remains, and the shortcut footer changes to `Enter:send`, `Shift+Tab:mode`, and `Ctrl+x:shortcuts`. |

The captures prove that the current Harness is not merely off by palette or glyph choice. Its startup information architecture, composer anatomy, footer grammar, and startup-to-edit transition are different.

#### Required first-slice requirements from the measured comparison

The first implementation slice must explicitly own and evidence:

| Requirement | Reference behavior to reproduce | Current Harness gap | Acceptance evidence |
|---|---|---|---|
| `P0-START-01` | Welcome uses a bordered primary panel with logo, title/version, changelog, and action rows with right-aligned shortcuts | Harness shows centered logo and hints without the bordered action panel | Reference/Harness semantic cells and xterm.js captures at 120x32, 100x30, and 80x24 |
| `P0-START-02` | Top breadcrumb and contextual warning occupy stable shell regions | Harness has no equivalent measured breadcrumb/warning composition | Cell geometry, style, and screenshot comparison with identity/path fields declared explicitly |
| `P0-START-03` | Typing transitions from welcome to the active composer shell | Harness keeps welcome content visible while editing | Same input trace; compare frame sequence, focus, cursor, and cleared/retained regions |
| `P0-COMP-01` | Composer is a bordered strip with prompt, cursor, and right-aligned model badge | Harness composer is a different unboxed arrangement | Styled cell grid plus settled PNG comparison |
| `P0-KEY-01` | Contextual shortcut footer changes with composer state | Harness uses a different footer vocabulary and placement | Exact footer geometry and interaction trace, with only identity fields masked |

The raw PNG scores above are not passes. They include deliberate identity/path differences and therefore require the registered mask policy. They are included to prevent future agents from describing this baseline as “close.”

Evidence paths used for this audit:

```text
<evidence-root>/harness-startup/
<evidence-root>/harness-draft/
<evidence-root>/reference-startup/
<evidence-root>/reference-draft/
```

### 7. Clean-room Reference Decision

Do not copy or directly run Grok Build tests against Harness.

The reference tests are tightly bound to the reference binary, its app state, ACP/effect architecture, mock inference server, product strings, settings registry, OAuth setup, leader flows, and PTY harness. Adapting them until they compile against Harness would be a port of their test architecture, not an independent parity proof.

Use the reference tests only as a catalog of observable scenarios that should be independently rediscovered and re-authored.

#### Required Strategy

Use these four separate layers:

1. **Black-box reference observation**: run a pinned reference binary in an isolated capture lab.
2. **Independent parity specification**: author a Harness-neutral surface, state, and interaction manifest from observed behavior.
3. **Harness-native implementation tests**: test Harness events, projections, `AppState`, UI actions, `UiIntent`, rendering, CLI routing, and PTY behavior without reference dependencies.
4. **External differential acceptance**: drive the pinned reference and Harness binaries with equivalent public inputs, then compare semantic cells, pixels, frame sequences, and interaction outcomes.

For configuration and schema work, use two independent references:

- Grok Build is the primary product-behavior reference for settings, workspace/worktree, trust, provider, session, and capability configuration.
- The current OpenCode checkout is a secondary reference for public JSON/JSONC ergonomics, provider/model/agent/permission/MCP shape, layering, migration, and schema behavior.

Compare both references, then implement the resulting contract in Harness-native Rust. Neither reference source, schema implementation, package layout, migration code, or product-specific identifier may be copied.

#### Prohibited Reference Transfer

Do not copy, translate, mechanically transform, or depend on:

- Reference Rust source.
- Reference tests.
- Reference snapshots.
- Reference YAML scenarios.
- Reference fixtures.
- Reference identifiers.
- Reference module decomposition.
- Reference state or effect architecture.
- Reference PTY harness code.
- Reference mock server code.
- Reference themes or string tables.
- Reference glyph assets or logos.
- `xai-grok-*` crates.
- Compile-time or runtime paths into `inspirations/grok-build` from first-party Harness code.

The isolated acceptance evaluator may launch a licensed reference binary as a black-box process. Harness production code and ordinary Harness tests must not link to or read from the reference tree.

### 8. Reference Freeze Before Implementation

Do not start substantial presentation implementation until the reference contract exists.

Create a reference receipt containing at least:

- Reference binary path and SHA-256 digest.
- Reference source revision if known.
- Secondary config/schema reference revision and digest where OpenCode behavior is used.
- Harness source revision.
- Parity manifest version and digest.
- Scenario corpus version and digest.
- Operating system or container image digest.
- Terminal emulator/parser version.
- Chromium version.
- xterm.js version and renderer mode.
- Exact terminal font files and hashes.
- Font size, line height, letter spacing, and ligature policy.
- Device-pixel ratio.
- GPU or software-rendering policy.
- Locale and Unicode width assumptions.
- `TERM`, `COLORTERM`, and color mode.
- Viewport dimensions.
- Theme and configuration inputs.
- Seeded workspace contents.
- Seeded provider, tool, permission, and question responses.
- Clock and random-seed policy.
- Exact input trace.
- Config layers, setting values, environment overlays, schema versions, and migration inputs used by the scenario.

Before comparing Harness, prove the reference capture pipeline is deterministic:

- Run at least three isolated reference-versus-reference captures.
- Untimed semantic cells and settled PNGs must be identical.
- A nondeterministic reference scenario must be stabilized with controlled inputs or excluded with a documented reason before implementation begins.
- Do not hide nondeterminism with a broad pixel tolerance.

### 9. Independent Parity Manifest

Create a machine-readable manifest owned by Harness test infrastructure. Do not copy the reference scenario files.

Every row must contain:

- Stable behavior ID.
- Stable capability ID and visible action ID where applicable.
- Stable subsystem ID for every core implementation under audit.
- Surface and state.
- Reference binary digest.
- Reference observation receipt.
- Preconditions and seeded external inputs.
- Viewport and terminal environment.
- Input sequence with fixed capture checkpoints.
- Expected TUI action and emitted `UiIntent`.
- Expected CLI/coordinator/backend owner.
- Expected filesystem, Git, process, provider, network, session, queue, or persistence side effect.
- Expected rollback, cancellation, and recovery outcome.
- Expected focus owner.
- Expected cursor state.
- Expected scroll and selection state.
- Expected overlays and z-order.
- Expected semantic-cell artifact.
- Expected PNG artifact.
- Expected frame sequence.
- Timing contract if observable.
- Harness fixture owner.
- Harness state/interaction test owner.
- Harness render test owner.
- Harness PTY test owner.
- Real compiled-binary journey owner.
- Backend integration and side-effect test owner.
- Differential evaluator owner.
- Identity substitution fields.
- Deliberate divergence ID, if any.
- Current Harness disposition: `replace` | `rework` | `retain-seam-only` | `retain-with-reference-proof`.
- Reference comparison receipt and disposition rationale.
- Configuration scope, layer, merge strategy, default, sensitivity, capability dependency, and migration owner where applicable.
- Completion status.

The corpus custodian who captures and freezes reference expectations must not be the sole implementer or sole acceptance reviewer.

### 10. Required Surface and State Coverage

The manifest must cover every applicable state, not one representative screenshot.

At minimum include:

#### Startup and Welcome

- Fresh startup.
- Configured startup.
- Authentication/setup state where Harness has an equivalent.
- Trust or safety gate equivalents.
- Compose-first focus.
- Transition into a live session.
- `Ctrl+W` worktree creation through a real Git repository and resulting live session.
- `Ctrl+S` session resume through the real picker and resulting live session.
- Changelog/help action through its real destination.
- Quit action with complete terminal restoration.
- First prompt submission through provider start and live-shell transition.
- Failure journeys for unsupported repository, worktree collision, resume failure, and interrupted startup handoff.
- Wide, narrow, and short layouts.

#### Primary Session Shell

- Idle.
- Editing.
- Streaming.
- Waiting on a tool.
- Waiting on permission.
- Waiting on a question.
- Cancelled.
- Failed.
- Recovered.
- Completed.
- Scrolled away from follow mode.
- Follow mode restored.

#### Transcript and Blocks

- User messages.
- Assistant messages.
- Thinking/reasoning summaries.
- Read, edit, bash, search, web, MCP, LSP, task, and unknown tool states.
- Running, successful, failed, interrupted, folded, truncated, expanded, and raw states.
- Diffs and syntax-highlighted code.
- Background tasks and subagents.
- Compaction and context markers.
- Long wrapped content.
- Selection, copy, hyperlink, and disclosure behavior.

#### Composer

- Empty, focused, blurred, multiline, and long wrapped drafts.
- Cursor movement and selection.
- History.
- Bracketed paste.
- Slash mode.
- File mention mode using real workspace files.
- Model or agent selection.
- Normal, plan, approval, and other safely mapped modes.
- Busy, queued, send-now, interrupt, and disabled states.
- Draft preservation across permissions, questions, and overlays.
- Clear, rewind, cancellation, and recovery gestures.
- Mid-turn interjection and prompt-queue behavior.

#### Overlays and Secondary Surfaces

- Command palette.
- Argument/model picker.
- Session picker and resume.
- Help and shortcuts.
- Settings, themes, and toggles.
- Permission prompt.
- Question prompt.
- Plan approval or Harness-equivalent approval flow.
- Status and details surfaces.
- Tasks, subagents, MCP, LSP, modified files, and session lineage.
- Fullscreen block or detail viewers where mapped.
- Overlay preemption, dismissal, query clearing, and focus restoration.

#### Backend Capability Journeys

- Worktree create, enter, resume, fork, failure rollback, and cleanup.
- Sandbox and folder-trust allow, deny, persistence, and invalid-policy paths.
- Durable memory search, update, flush, restart, and retrieval.
- Plugin install, activate, invoke, disable, update, remove, and failure rollback.
- ACP and remote workspace connect, operate, disconnect, reconnect, and failure recovery.
- Foreign-session discovery/import and unsupported/corrupt-session handling.
- Recurring schedule, monitor, wait-any, wait-all, background demotion, cancellation, and restart.
- Prompt queue and mid-turn interjection ordering under concurrent input.
- Provider/auth/OIDC login, refresh, fallback, expiry, sleep/wake, and failure recovery.
- Update and crash-recovery journeys where reference-visible.

#### Configuration and Schema Journeys

- Fresh install with no config, starter config, global config, project config, workspace/worktree config, session overlay, command-line override, and environment overlay.
- Effective-value inspection and source explanation with redacted secrets.
- Scalar, map, list, permission-rule, provider, agent, model-profile, MCP, LSP, hook, and keybinding merge behavior.
- Canonical-field/compatibility-alias normalization, conflicts, deprecations, and version migrations.
- Unknown-field, malformed-value, semantically invalid, unavailable-capability, permission-blocked, unauthenticated, and valid-but-inactive diagnostics.
- Provider/model catalog validation, credential reference resolution, fallback selection, and transport-capability reporting.
- Worktree-scoped and session-scoped settings resolving against the correct owner.
- Runtime/TUI config separation and shared settings-registry metadata.
- `config validate`, `config show --effective`, `config explain`, `config sources`, and `doctor` through the real compiled CLI.
- TUI settings display, edit, reset, reload/restart behavior, and persistence where the reference exposes it.
- Generated schema freshness, editor completion shape, round-trip serialization, and schema-version migration.

#### Responsive and Capability States

- `120x50` reference-primary viewport.
- `120x40` Harness signoff viewport.
- `100x30`.
- `80x24`.
- `79x24`.
- `60x20`.
- Selected extreme resize boundaries used by the reference.
- At least one wide viewport greater than 120 columns.
- Truecolor and reduced-color behavior.
- Enhanced and legacy key reporting.
- Mouse present and absent.
- Clipboard capability present and absent where behavior differs.

#### Animation and Dynamic States

- Startup, streaming, tool-running, waiting, permission, question, background, and completion animations.
- Spinner glyph sequence and cadence.
- Cursor movement, show/hide, and shape transitions.
- Overlay open/close, disclosure, selection, and focus transitions.
- Progress, queued, interrupted, retry, reconnect, and recovery frame choreography.
- Reduced-motion or terminal-capability fallback where the reference provides one.

No sampled subset can certify the complete surface. One failing row fails acceptance.

### 11. TUI Design Contract

Create or replace `crates/harness-tui/DESIGN.md` before rebuilding product surfaces.

It must define the measured reference contract for:

- Shell vertical order.
- Breakpoints and compact behavior.
- Padding and spacing rhythm.
- Transcript block anatomy.
- Composer anatomy and dynamic height.
- Overlay dimensions, placement, and z-order.
- Border and separator grammar.
- Glyph roles.
- Color roles and exact resolved values in the canonical environment.
- Text modifiers and emphasis hierarchy.
- Focus, hover, selected, disabled, busy, warning, success, and error states.
- Cursor rules.
- Animation and tick behavior.
- State transitions.
- Capability fallbacks.
- Harness identity substitutions.

The contract must come from measured reference captures, not the current Harness theme or the implementer's preferences.

Do not start full-screen implementation until core primitives and their complete state sets render against the reference contract in a dedicated state showcase or fixture harness.

### 12. Implementation Freedom and Required Replacement

The current presentation layer is disposable. Existing core implementations are also redesignable when reference comparison shows that replacement or substantial rework produces better behavior.

You may delete and rebuild Harness TUI painters, layout contracts, presentation models, local interaction reducers, overlay composition, transcript rendering, composer rendering, theme tokens, CLI handoff, and incomplete backend seams when that is the clearest path to parity.

You may add or extend coordinator commands, events, session/workspace services, providers, authentication, tools, integrations, persistence, and platform adapters when a required reference capability has no complete Harness owner. Place new authority in the correct crate; do not force backend behavior into the TUI merely because the action originates there.

Do not preserve old widgets, reducers, services, workflows, or backend implementations merely because they already function or have extensive tests. Retain only hard authority/safety seams and implementations that pass reference comparison. Rework tests that encode inferior legacy behavior; never weaken safety invariants to obtain parity.

For each first-party product module and subsystem classify it as:

- `replace`
- `rework`
- `retain-seam-only`
- `retain-with-reference-proof`

Default to `replace` or `rework` when the reference behavior is materially better. Use `retain-seam-only` for hard Harness boundaries, and use `retain-with-reference-proof` only when measured evidence justifies retention.

The classification must explicitly cover:

- Layout and shell composition.
- Welcome/startup painters.
- Global chrome and status lines.
- Transcript and tool renderers.
- Markdown, syntax, and diff rendering.
- Composer and control dock.
- Permissions and questions.
- Every overlay and picker.
- Secondary operator surfaces.
- Theme and glyph tokens.
- Focus, scroll, selection, fold, and resize behavior.
- Mouse hit regions.
- Runtime terminal capability presentation.
- Every visible action and its backend owner.
- Coordinator, compaction, provider, permission, task, hook, session, replay, workspace, worktree, sandbox, memory, plugin, protocol, background, update, and recovery implementations and capability surfaces.

A file containing a reference-like glyph does not count as reworked if its component anatomy, state choreography, or geometry remains old Harness behavior.

### 13. Harness Authority and Safety Invariants

The TUI remains presentation-only, but the product scope is not presentation-only. Missing backend capability must be implemented in the CLI, coordinator, core, provider, tool, workspace, session, integration, or platform layer that owns it.

Preserve these boundaries:

```text
terminal input
  -> local TUI action and local state transition
  -> zero or one mutating UiIntent at a commit boundary
  -> CLI/coordinator routing
  -> coordinator-owned provider/tool/permission/task/lifecycle work
  -> append-only events
  -> replay/projection-derived state
  -> TUI render
```

The TUI must not:

- Execute providers or native tools.
- Execute shell commands or network requests.
- Append events.
- Resolve permissions independently.
- Schedule tasks or child agents.
- Run hooks or compaction.
- Mutate replay state.
- Bypass cancellation or permission rules.
- Add test-only success paths.
- Render stored reference screenshots instead of live components.

Preserve only these hard invariants; their current implementation strategies, data flow details, and UX remain subject to reference-backed redesign:

- Coordinator-only event append, scheduling, permission resolution, and lifecycle authority.
- Replay purity.
- Contiguous sequence-ordered event replay.
- Permission-before-execution.
- Cancellation wins over late results.
- Redaction before persistence and evidence.
- Compaction without rewriting `events.jsonl`.
- Harness permission names, tool IDs, provider data, session semantics, and product identity.
- Runtime and TUI configuration separation.
- Worktree, sandbox, plugin, remote-workspace, memory, scheduler, updater, and crash-recovery authority in dedicated non-TUI owners.

### 14. Required Evidence Stack

#### L0: Harness Invariants

Run the owner tests for core, providers, tools, permissions, replay, projection, cancellation, lineage, redaction, and compaction to establish the baseline. Hard-invariant failures block parity work. Tests that intentionally encode inferior legacy behavior must be replaced with reference-backed behavior tests rather than used to force retention.

Run the owner config/schema, migration, discovery, precedence, redaction, generated-schema, provider-catalog, manifest, and docs-reference tests. A stale schema, undocumented public key, migration conflict, or effective-config discrepancy blocks parity work.

#### L1: State and Interaction Tests

Independently author Harness-native tests that assert actual before-and-after state:

- Focus owner.
- Overlay stack.
- Cursor.
- Scroll offset and follow mode.
- Selection anchors and copied content.
- Fold/disclosure state.
- Composer draft and history.
- Queue state.
- `UiIntent` emission.
- Replay restrictions.
- TUI action to `UiIntent` mapping.
- CLI/coordinator/backend dispatch.
- Real external outcome and rollback state.
- Effective configuration, source-layer attribution, schema validation, migration result, and capability-availability state.

Do not substitute source-text assertions or marker presence for state proof.

For an advertised operation, the state test must fail if the shortcut is rebound to another action, the intent is dropped, the backend uses a no-op, the side effect is absent, or stale UI survives the transition.

#### L2: Semantic Cell Grid

Extend the current symbol-only test capture so important frames record, per cell:

- Grapheme or symbol.
- Display width and continuation state.
- Resolved foreground color.
- Resolved background color.
- Text modifiers.
- Hyperlink metadata where used.
- Cursor position, shape, and visibility.

Also record:

- Terminal dimensions.
- Alternate-screen state.
- Scroll offset.
- Selection anchors.
- Focus owner.
- Wrap, mouse, paste, and enhanced-key modes that affect interaction.

Expected values must come from the frozen reference corpus or an explicit Harness invariant. A snapshot generated only from current Harness output is a regression snapshot, not parity evidence.

#### L3: Real Harness PTY

Drive the compiled Harness application through a real PTY with exact input traces and viewport changes.

For action journeys, also inspect the real postcondition outside the terminal: Git worktree and branch state, filesystem contents, session store, provider/tool result, process state, queue state, plugin registry, remote connection, generated artifact, or other capability-specific outcome.

Capture every canonical checkpoint, not only terminal markers.

PTY evidence must fail closed. When the parity lane is requested:

- Missing environment support fails.
- Missing artifacts fail.
- Timeouts fail.
- Skipped scenarios fail.
- A missing binary fails.
- An assertion failure fails the lane.

Do not return early as a passing test when an opt-in variable is missing. Do not reuse one smoke scenario as evidence for unrelated flows.

Do not use an event-injection helper, test binary, preconstructed `AppState`, or direct state mutation as the sole acceptance owner for a public journey. Such fixtures may support lower layers but never replace the compiled product path.

#### L4: Differential Black-Box Capture

Launch the pinned reference binary and Harness in isolated but equivalent sandboxes.

Use equivalent:

- Workspace files.
- Provider responses.
- Tool outputs.
- Permission requests.
- Questions.
- Model metadata.
- Timing inputs.
- Terminal dimensions.
- Key and mouse traces.
- Configuration layers, settings, provider/model selections, permissions, feature flags, workspace/worktree roots, and capability availability.

System-specific launch adapters may translate public startup flags and seeded data. They must not branch within a scenario or alter expected UI behavior.

Capture semantic frames after every declared checkpoint and every canonical transition.

Where the journey has a non-terminal side effect, compare the normalized public outcome as well as the frames. Launch adapters may translate environment setup, but may not synthesize success or skip the real operation.

#### L5: Deterministic xterm.js Pixel Capture

Render both PTY streams through the same pinned xterm.js and Chromium pipeline using the exact same font files, dimensions, device-pixel ratio, theme, locale, and renderer mode.

Capture full terminal PNGs. Do not crop to favorable regions.

For settled frames:

- Require identical image dimensions.
- Require zero unapproved RGBA pixel differences.
- Do not use SSIM, percentage similarity, or per-channel tolerance while claiming pixel-perfect parity.

For dynamic states, compare fixed canonical renderer ticks and settled frames separately.

#### L6: Independent Review and Holdouts

After automated gates pass:

1. A visual reviewer must inspect every reference/actual pair and every automated hotspot or mismatch report.
2. A code and clean-room reviewer must confirm the UI is implemented from live Harness components and does not embed reference captures, hashes, test-only branches, copied source, or copied test architecture.
3. An evaluator must run undisclosed holdout traces involving Unicode widths, long content, resizing, scrolling, timing perturbations, and error paths. Expectations are captured from the pinned reference after implementation, not supplied to the implementer.

Both reviewers must be independent of the implementer. Reviewer disagreement blocks completion.

### 15. Exact Acceptance Tolerances

#### Semantic Cells

Require zero unapproved differences in:

- Dimensions.
- Grapheme.
- Cell width and continuation.
- Foreground and background colors.
- Modifiers.
- Hyperlinks where present.
- Cursor position, shape, and visibility.

#### Emulator and UI State

Require zero unapproved differences in:

- Alternate-screen state.
- Focus owner.
- Scroll offset.
- Selection anchors.
- Overlay stack and z-order.
- Wrap mode.
- Mouse mode.
- Paste mode.
- Enhanced key mode.
- Interaction outcomes.

#### Frame Sequence

Require zero missing, extra, or reordered canonical frames at fixed renderer ticks. PTY byte chunking itself is not compared.

#### Settling

A frame is settled only after scripted external events complete and three consecutive renderer ticks produce unchanged semantic cells.

Exceeding a predeclared scenario deadline is a failure. It is not permission to skip the capture.

#### Timing

Prefer a controlled clock for deterministic transitions.

For genuinely timed public behavior that cannot use a controlled clock:

- Freeze the timing contract before Harness comparison.
- Run at least 30 trials per timed scenario.
- Compare median and p95.
- Require both to remain within `max(one renderer tick, 10%)` of the reference.
- Do not widen bounds after seeing Harness results.

### 16. Identity, Dynamic Fields, Masks, and Divergences

Masks default to none.

Eliminate dynamic differences through identical seeds, clocks, paths, IDs, model data, and external responses before considering a mask.

Any unavoidable exemption must be:

- Field-level, not a rectangle chosen after failure.
- Registered before implementation.
- Applied symmetrically.
- Reviewed independently.
- Restricted to the exact differing glyph content.

Masks may not cover:

- Geometry.
- Spacing.
- Borders.
- Icons.
- Color.
- Focus.
- Cursor.
- Selection.
- Interactive chrome.
- Whole components or arbitrary screen regions.

Harness identity differences are explicit product divergences. The Harness identity region must retain the reference bounds, alignment, style, and animation while substituting Harness-safe glyphs or text.

If any divergence or mask remains, the final claim must say "exact outside the declared identity or dynamic fields." Do not claim global pixel identity.

Implementation convenience, missing time, old Harness behavior, and inability to reproduce the reference are not valid divergence reasons.

### 17. Evidence Integrity and Anti-Gaming Rules

- Harness production code must not read reference captures, expected hashes, evaluator manifests, scenario IDs, or golden files.
- Do not branch on test mode, capture environment, reference digest, terminal title, known fixture text, or evaluator identity.
- Do not add alternate render paths used only by tests.
- Do not paste terminal screenshots or pre-rendered text into the live UI.
- Do not derive expected output from actual Harness output.
- Do not mass-accept snapshots.
- Do not delete failing coverage to make the suite pass.
- Do not replace full-frame assertions with substring checks.
- Do not use `contains(A) || contains(B)` to accept multiple visibly different outcomes.
- Do not prove an interaction by pressing a key without asserting the resulting state and frame.
- Do not prove an advertised backend feature by asserting only that its label, shortcut, palette entry, intent, or status banner exists.
- Do not bind a reference shortcut to an unrelated legacy action. A displayed shortcut and its actual dispatch must be the same operation in every applicable focus state.
- Do not route a missing capability to `Placeholder`, `NewSession`, a generic palette action, or a local-only state transition while retaining the reference label.
- Do not construct a live session, worktree, resumed session, completed tool, installed plugin, connected remote, or recovered state directly as alleged end-to-end evidence.
- Do not leave known user-reported defects untracked while continuing cosmetic parity work.
- Do not construct a new unrelated `AppState` halfway through an alleged end-to-end journey.
- Do not use fake file mentions that never touch the workspace index.
- Do not let manifest descriptions outrun executable evidence.
- Do not let a lane report success when any owned stage failed.
- No golden refresh may accompany an implementation change without separate reference-corpus approval and a documented reference reason.

The seventeen deleted broad snapshots are a coverage regression unless restored or replaced one-for-one with equal-or-stronger tests covering the same geometry and state classes.

### 18. Signoff Infrastructure Corrections

The current evidence system cannot certify reference parity without changes.

Correct at least these audited defects:

- Symbol-only snapshots omit colors and modifiers.
- Current snapshots compare Harness against itself.
- PTY tests are marker-based and often opt-in.
- Default nextest profiles exclude PTY/native binaries.
- Some PTY paths return success without running when an environment variable is absent.
- `scripts/test-lanes.sh` signoff stages use fail-open `|| true` behavior.
- The signoff manifest checks shape and names, not that owner tests ran or markers appeared.
- Several unrelated flows reuse a generic PTY smoke owner.
- Native visual tests prove environment metadata rather than capturing and comparing the UI.
- A test named as live TUI parity only checks environment/config preconditions.
- Existing visual renderer/helper capability is not wired into a fail-closed parity gate.

Create a dedicated strict parity lane if changing existing optional lanes would violate another documented contract. The strict lane must fail on every missing or failed stage and produce one machine-readable summary linking all artifacts.

### 19. Required Work Loop

Before more pixel-polish work:

1. Inventory every visible action, public capability, and affected first-party subsystem in the pinned reference and current Harness.
2. Compare each affected Harness core subsystem against the equivalent reference behavior and author its disposition before editing it.
3. Inventory every public setting, schema, config layer, migration, alias, capability dependency, and effective-value surface in the pinned reference and current Harness.
4. Compare current Harness configuration behavior against the reference and author its schema/settings disposition before editing it.
5. Drive every currently advertised Harness action and configuration journey through the compiled binary.
6. Convert every discovered dead shortcut, wrong dispatch, stale transition, placeholder, missing side effect, inferior legacy behavior, broken Harness function, stale schema, or config mismatch into a P0 manifest row and failing regression test.
7. Finish those P0 functional rows, core-subsystem dispositions, and config/schema dispositions before returning to antialiasing or cosmetic residuals.

For every capability and parity row:

1. Capture and freeze the reference state before implementation.
2. Add an independently authored failing Harness state, interaction, backend, or side-effect test.
3. Add an independently sourced semantic-cell expectation where the feature renders in the terminal.
4. Implement the real backend capability in its correct owner.
5. Wire the public TUI, CLI, tool, protocol, or background action to that owner.
6. Replace or rework the complete component experience, including animations and recovery states.
7. Pass focused unit and integration owners.
8. Pass the compiled-binary journey and verify its external postcondition.
9. Pass the real Harness PTY trace.
10. Pass semantic differential comparison.
11. Pass xterm.js pixel and fixed-tick animation comparison.
12. Run adjacent states, error paths, cancellation, restart, and responsive boundaries.
13. Record artifacts and residual risk.

Do not defer all visual comparison until the end. Do not proceed to unrelated surfaces while the current row still differs.

Do not defer functional integration until after the UI looks correct. A visually matching but nonfunctional surface is an earlier and more severe failure than a pixel mismatch.

### 20. Required Validation

Discover current exact targets, then run the narrowest owners and broader gates. At minimum the final revision must include successful current runs of:

```bash
cargo fmt --all --check
cargo nextest run -p harness-tui
cargo nextest run -p harness-core
cargo nextest run -p harness
cargo nextest run -p harness-tools
scripts/test-lanes.sh quality-gates
scripts/test-lanes.sh all-deterministic
scripts/test-lanes.sh signoff-pty
bash scripts/harness-qa-dogfood.sh --self-test
```

Also run the new strict differential parity lane, complete xterm.js capture suite, and independent review gates.

For every newly required backend capability, run its real owner tests and at least one compiled-product journey. The final revision must additionally prove:

- Worktree creation, entry, isolation, resume, failure rollback, and cleanup in a temporary real Git repository.
- Startup-to-worktree, startup-to-resume, startup-to-first-turn, cancellation, and recovery journeys through the compiled TUI.
- Every visible shortcut dispatches the advertised action in each applicable focus state.
- Every palette and slash-command entry is executable or explicitly unavailable; no included production entry dispatches to a placeholder.
- Every existing Harness CLI command and native tool affected by the redesign retains a passing happy path and meaningful failure path.
- Every affected core subsystem has a reference comparison, disposition, replacement/rework test coverage, and migration/recovery evidence where its behavior changes.
- Every public setting has generated schema, scope/precedence, migration, effective-value, redaction, capability-validation, and real CLI/TUI evidence.
- Required plugin, memory, ACP, remote-workspace, scheduler, queue/interjection, provider/auth, update, and crash-recovery journeys pass when their capability rows are implemented.
- Runtime/TUI config validation, effective-config, source-explanation, migration, redaction, and capability-availability journeys pass through the compiled CLI and applicable TUI settings surface.
- Generated schemas match the typed runtime definitions and remain synchronized with docs, examples, manifests, and migration tests.
- Animation frame sequences and timing pass at the canonical renderer ticks.

When contracts change, run the relevant documentation, schema, event, manifest, and lane tests. When native evidence is claimed, run the actual native capture lane and retain its provenance.

Build and tests are supporting evidence. They do not replace the differential acceptance gate.

### 21. Mandatory Final Report

Report:

- Harness revision.
- Reference binary digest.
- Environment digest.
- Manifest digest.
- Number of required rows and number passed.
- Number of required capabilities, actions, and end-to-end journeys and number passed.
- Existing Harness capability regression result.
- Core subsystem audit: required, compared, replaced, reworked, retained with proof, blocked, and diverged counts.
- Config/schema audit: settings, schemas, layers, migrations, aliases, capability checks, effective-value journeys, and redaction results.
- Worktree, sandbox, memory, plugin, protocol, background, provider/auth, update, recovery, compaction, and core-subsystem comparison results.
- Exact commands executed.
- Semantic-cell comparison result.
- Pixel comparison result.
- Interaction/frame-sequence result.
- Real side-effect and rollback result for every action journey.
- Animation sequence result.
- Timing result.
- Holdout result.
- Independent visual-review result.
- Independent code/clean-room-review result.
- Restored or replacement coverage for the seventeen deleted snapshots.
- Remaining divergences and masks.
- Any lane that could not run and why.
- Every known user-reported defect and its closing evidence.
- Artifact root and provenance receipt.

The only acceptable unqualified parity claim is:

> Against reference binary digest R, environment E, and manifest M, Harness revision H produced zero unapproved semantic-cell and RGBA differences at every required checkpoint, matched all canonical interaction and frame sequences, and satisfied the frozen timing bounds.

If identity substitutions, masks, or divergences remain, qualify the claim precisely. Never say simply "pixel-perfect," "1:1," or "complete" when the evidence does not establish that statement.

### 22. Stop Rule

Do not stop because the shell looks cleaner, because the old sidebar is gone, because snapshots were updated, because tests pass, or because an independent reviewer likes a sample screen.

Stop only when every required capability, action, journey, and manifest row passes the same fresh, fail-closed backend, side-effect, semantic, pixel, interaction, animation, timing, invariant, and independent-review gates on the current revision, every existing Harness function affected by the work is fully operational and polished, no known user-reported defect remains, and no visible control is a placeholder or wrong dispatch.

The only alternative stop condition is a genuine Harness invariant that makes one exact capability impossible and the user has explicitly accepted that exact divergence. Missing implementation time, large scope, absent backend architecture, unavailable fixtures, or a visually convincing substitute are not stop conditions.
