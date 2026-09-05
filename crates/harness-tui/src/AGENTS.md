# TUI SOURCE GUIDE

## OVERVIEW

State, rendering, terminal, and focused UI-domain implementations for `harness-tui`.
Score 13: 114 direct files (3), 57 subdirectories (2), 100% Rust (2), `lib.rs` boundary (2),
>30 measured symbols (2), and >10 measured exports (2); reference centrality unmeasured.

## STRUCTURE

```text
src/
|- app/                    # AppState extensions, projection, input and overlays
|- terminal/               # capability, decoding, presentation and frame output
|- composer_*/             # atom-safe editing, completion and integrated composer state
|- dashboard*/             # deterministic read models, controls, dispatch and responsive UI
|- transcript_*/           # identity, blocks, scrolling, selection, paging and integration
|- input/ + scheduling/    # normalized events, deadlines, pacing and redraw arbitration
|- theme*/ + layout/       # semantic appearance and terminal-cell geometry contracts
|- ui_overlays/            # modal rendering and shared hit geometry
|- ui_secondary/           # operator rail projections and interactions
`- lib_tests/ + tests/     # crate-private rendering/projection regression suites
```

## WHERE TO LOOK

| Change | Primary owner | Coupled contract |
|--------|---------------|------------------|
| Event-to-screen state | `app/session_projection.rs` | `app/session_projection/`, transcript integration |
| Keyboard or pointer priority | `app/key_interaction.rs`, `app/mouse_interaction.rs` | overlay ownership and stale press invalidation |
| Top-level paint dispatch | `ui.rs` | `layout.rs`, `ui_chrome.rs`, `ui_overlays.rs` |
| Transcript visual model | `ui_transcript_sections.rs`, `ui_transcript_types.rs` | tool render, selection, surface layout |
| Runtime wake/redraw behavior | `runtime.rs`, `runtime_wait_set.rs` | `scheduling/`, frame output, presentation telemetry |
| Composer text changes | `composer_atoms/`, `composer_editing/` | completion, ghost suggestions, queue integration |
| Modal geometry | `layout/permission.rs`, `ui_overlays/modal_interaction.rs` | paint and hit-map parity |
| Theme changes | `theme.rs`, `theme_family/`, `theme_tokens.rs` | fallback levels and ASCII glyph mode |

## CONVENTIONS

- Extend `AppState` in concern-specific sibling modules; match visibility to the narrowest consumer.
- Route durable, live, runtime, and historical events through their distinct ingestion paths.
- Build transcript output in stages: semantic sections, visual surfaces, measured layout, then paint/hit maps.
- Share the exact measured rectangles between rendering and interaction; generation changes invalidate stale pointer state.
- Keep caches bounded and key them with every content, width, fold, theme, or lifecycle input that affects output.
- Treat queue, completion, lifecycle, dashboard, and permission behavior as explicit state machines with stale-input errors.
- Supply time to pacing and animation logic; reduced motion settles instead of scheduling idle redraws.
- Compatibility aliases and deprecated event variants exist for persisted sessions; remove only with migration evidence.

## ANTI-PATTERNS

- Never append events, mutate `AppState`, or emit intents from `view_model.rs` or the `render_app` path.
- Never reserve the removed primary operator-sidebar rectangle in the live shell.
- Never group permission/question waits as ordinary tool context or resolve them from display prose.
- Never reflow already-rendered streaming code rows merely because a fence later closes.
- Never reuse transcript selection cells after reflow, resize, fold, or unresolved endpoint mapping.
- Never persist ghost suggestions, silently claim unavailable diagnostics succeeded, or auto-fallback a failed model.
- Never make cache eviction, cache identity, or ordering nondeterministic.
