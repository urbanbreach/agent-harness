# Harness / Grok Build UI Difference Catalog

**Status:** active, bounded discrepancy catalog. This catalog does not authorize
an autonomous parity loop, relabel historical evidence, or change the reference
identity in
[`configs/tui-fidelity-reference-authority.json`](configs/tui-fidelity-reference-authority.json).

Each row is an independently assignable UI discrepancy. A task owns one row,
its named Harness surfaces, and its named scenarios. Product identity,
event/coordinator authority, permission-before-execution, replay purity, and
redaction remain Harness-specific. Grok source, tests, fixtures, architecture,
identifiers, and product copy are inspection material only.

| ID | User-visible discrepancy | Harness owners | Required scenarios | Completion boundary |
|---|---|---|---|---|
| UI-01 | Startup and draft transitions differ in shell hierarchy or focus choreography. | `crates/harness-tui/src/welcome_surface/`, `crates/harness-tui/src/ui_composer.rs` | `baseline-startup`, `baseline-draft` | Both scenarios agree with the active reference at every declared checkpoint; no unavailable action is added. |
| UI-02 | Composer placement, growth, queue/interjection feedback, or footer stability differs across turn states. | `crates/harness-tui/src/ui_composer.rs`, `crates/harness-tui/src/layout.rs` | `baseline-idle`, `baseline-queue`, `baseline-stream` | Geometry and state transitions agree at the catalogued viewports without changing Harness commands or provider truth. |
| UI-03 | Transcript streaming, tool, diff, completion, or recovery blocks differ in ordering and disclosure. | `crates/harness-tui/src/ui_transcript_sections.rs`, `crates/harness-tui/src/ui_transcript_render.rs` | `baseline-stream`, `baseline-tool`, `baseline-diff`, `baseline-complete`, `baseline-recover` | Event order remains authoritative and all declared checkpoints compare exactly outside approved identity substitutions. |
| UI-04 | Manual scroll, return-to-live, resize anchoring, selection, or mouse behavior differs. | `crates/harness-tui/src/transcript_scroll/`, `crates/harness-tui/src/transcript_integration/` | `baseline-scroll`, `baseline-resize`, `baseline-mouse` | The same action sequence produces the same viewport/focus state while detached and after reattachment. |
| UI-05 | Permission, question, palette, or other modal surfaces differ in focus, dismissal, or stacking. | `crates/harness-tui/src/overlay.rs`, `crates/harness-tui/src/ui_overlays.rs` | `baseline-permission`, `baseline-question`, `baseline-modal-surfaces` | Keyboard and mouse outcomes, z-order, and cancellation agree; no fake backend success is introduced. |
| UI-06 | Dashboard, media, theme, CJK, or reduced-terminal behavior differs. | `crates/harness-tui/src/ui_dashboard.rs`, `crates/harness-tui/src/ui_media.rs`, `crates/harness-tui/src/theme_system/` | `baseline-dashboard`, `baseline-media`, `baseline-themes`, `baseline-cjk`, `baseline-reduced-capabilities` | Each applicable scenario passes its exact terminal tier and viewport; unsupported capabilities remain absent or truthfully unavailable. |

The scenario registry at
`crates/harness-testkit/src/tui_fidelity_scenarios/baseline/registry.json` is
the path authority for these scenario families. Adding a discrepancy requires
a new bounded row and a real registered scenario; broad masks, aggregate
similarity thresholds, and inherited historical pass claims are not accepted.
