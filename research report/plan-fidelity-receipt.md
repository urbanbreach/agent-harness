# Plan fidelity receipt

Plan: `.omo/plans/grok-build-harness-parity.md`
Harness source pin: `0bc7bf7996c2a96c61dbf427da311469f801d2b0`
Grok mirror pin: `bc7f02eddd3d84085849dc19ed216f11c23b0571`
Mirror `SOURCE_REV`: `d5a0335a47221e8c9519936cb693e9b6450227ec`

## Verification method

- LSP symbols/references verified current Rust owners and existing anchor/recovery types.
- ast-grep verified structural call sites for dashboard return state, interject action, and hyperlink emission.
- Direct reads verified current config/docs/test contracts and the research source rows.
- The coverage validator found all 218 corpus identifiers and zero source-less implementation tasks.
- The dependency validator parsed the plan and reported:

```text
PLAN FIDELITY ORDER GREEN
tasks=12 dependency_edges=11 cycles=0 contradictions=0 source_backed=12
errors=
```

## Task fidelity

| Task | Classification | Current-source evidence | Research disposition |
| --- | --- | --- | --- |
| 1 reply-capable PTY | verified/current gap | Existing owner uses `portable-pty` + reply-less `vt100`; signoff lane is fail-closed | P0-06; C036/C041/C050/C056 implemented as query/state/provenance delta, not “no emulator” |
| 2 xterm browser evidence | verified/current gap | No committed xterm/browser-terminal harness; browser automation remains a product non-goal | Test-only QA infrastructure; no browser product surface |
| 3 disconnect truth | verified/current narrow gap | queued-drain calls `apply_runtime_event_stream_closed`; empty wake clears receiver; reopen copy already truthful | P0-05 chooses the explicitly permitted truthful-reopen branch; real reconnect excluded |
| 4 dashboard surface | verified/current gap | dashboard is capped; both entry paths construct return state with anchor `None`; exact `TranscriptContentAnchor` already exists | P0-01/C021/C051 implemented by reuse, not a new anchor/event model |
| 5 folds/navigation | verified/current/conditional | one `FoldController` exists; exact full-history anchors exist; completed-response navigation/affordance is incomplete | P0-02/C044; C007/C011 refuted duplicate grouping; C052 pin reserve remains RED-gated; C031 preserves exact layout |
| 6 composer semantics | verified/current refinement | multiline state exists; interject action is sequential cancel+submit; `QueueAction` already distinguishes dispositions | P0-04; C022 refuted stale absence claim, behavior/presentation mismatch remains actionable |
| 7 rich content | verified/current gap | tables conceal formatting, markdown drops destinations, OSC-8 helper/map exists, open-fence parser exists | P0-03/C053; reuse current parsers/maps and preserve display-column contract |
| 8 slash metadata | verified/current gap | `SlashCommandLeaf` has id/metadata/aliases only; ranking/registry are static | P1-01/C054; dynamic ACP transfer explicitly excluded |
| 9 modal chrome | verified/current refinement | stack/focus/interaction geometry exists; render chrome remains bespoke | P1-02; C019 refuted focus-loss invention; preserve stack authority |
| 10 task/status grammar | verified/current gap | activity/task/live-state owners exist; sleeping/tasks-complete vocabulary absent | C024 and detailed comparison deferred row become owner-local acceptance work after transcript grammar |
| 11 theme integration | verified/current with shared-workspace constraint | runtime/theme-system authority remains split; startup motion/tests already exist; unrelated role/index edits are present | P1-03/C026 actionable for mapping; C035 branding guardrail; startup change superseded/regression-only |
| 12 resize/follow/glyph | verified/current mixed gap | 16 ms debouncer exists but production ingress bypasses it; exact viewport taxonomy exists; follow/glyph owners exist | P1-04/C038 actionable; C028/C030 coasting transfer excluded; C031 exact anchors preserved |

## Claim disposition audit

- **Supported/current:** C021, C026, C031, C035, C038, C044, C051, C053, C054, C055, C056 are represented by Tasks 1-12 or guardrails.
- **Refuted/excluded:** C007, C011, C019, C022 are not implemented as originally phrased; their current-source corrections are explicit.
- **Conditional:** C052 has a named failing test gate; no production reserve is permitted while the characterization remains green.
- **Partial:** every remaining C claim is represented by task references, cross-cutting acceptance, or an explicit Must-NOT-Have.
- **Observed pattern wording:** Grok-side evidence is described as a pinned observed pattern, never an upstream contract.

## Corrected provenance

- `crates/xai-grok-shell/.../agent_ops.rs` -> `inspirations/grok-build/crates/codegen/xai-grok-shell/src/agent/mvp_agent/agent_ops.rs`
- `scrollback/state/sticky.rs` -> `scrollback/sticky.rs`
- `scrollback/state/wrap_restore.rs` -> `wrap_restore.rs`
- `xai-grok-pager-render/src/render/prompt_widget/mod.rs` -> `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/prompt_widget/mod.rs`

## Ordering audit

- Wave 1 evidence precedes every visual acceptance claim.
- Dashboard/transcript/composer owner work can proceed in parallel, with explicit shared `AppState`/keybinding merge barriers.
- Rich content waits for transcript grammar; slash waits for composer semantics; modal chrome waits for shell geometry.
- Task/status and resize/follow work wait for transcript identity/grammar.
- Theme work is independent but must preserve current unrelated working-tree hunks.
- Final verifiers run only after all twelve implementation todos.

Verdict: **PASS** — every source-backed task is verified/current, current-source-corrected, conditional, superseded, or explicitly excluded; the dependency graph is acyclic and internally consistent.
