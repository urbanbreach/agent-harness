# HARNESS TUI KNOWLEDGE BASE

## OVERVIEW

Ratatui library crate for startup, live-session, replay, and review shells.
`src/lib.rs` defines the public module and export boundary; reference centrality is unmeasured.

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Public crate surface | `src/lib.rs` | Module declarations and deliberate top-level re-exports |
| Aggregate UI state | `src/app.rs`, `src/app/` | `AppState`, projections, overlays, interaction routing |
| Frame composition | `src/ui.rs`, `src/ui_*` | Layout-to-render pipeline and transcript surfaces |
| Terminal runtime | `src/runtime.rs`, `src/terminal/` | Event loop, capability setup, frame output, teardown |
| Session cost and token speed | `src/app/session_usage.rs`, `src/ui_lifecycle.rs` | Provider usage, model prices and live generation throughput |
| Layout and theme contracts | `src/layout.rs`, `src/theme.rs`, `src/theme_*` | Breakpoints, geometry, semantic colors, glyph fallback |
| Public state-machine domains | `src/*/mod.rs` | Facades for composer, dashboard, transcript, input, scheduling |
| Integration contracts | `tests/` | Rendering, lifecycle, PTY, Unicode, snapshots, side effects |

## CONVENTIONS

- Keep runtime I/O, state mutation, immutable projection, and painting as separate layers.
- Public subsystems use a `mod.rs` facade with explicit `pub use`; implementation leaves stay private or crate-visible.
- Model behavior with typed enums and `Result` errors; workspace lints reject panic/unwrap/expect paths in production.
- Use semantic theme tokens and glyph catalogs rather than local colors or terminal-specific symbols.
- Treat dimensions as terminal cells: use grapheme/display-width helpers, saturating arithmetic, clamps, and zero-area exits.
- Preserve deterministic order with stable IDs, explicit registries, `BTreeMap`/`BTreeSet`, and supplied clocks.
- Keep replay and completed-session surfaces read-only; mutations cross the coordinator boundary as `UiIntent`.
- Co-locate focused unit tests; use `tests/` for package-level behavior and real-surface contracts.

## Composer steering

Plain Enter steers a running turn when the draft is plain text. Alt+i and
Ctrl+Alt+Enter queue a separate follow-up after the active turn. Both actions
submit normally when idle; tagged or attached drafts fall back to queueing
during an active turn. A steering echo carries a `STEERING` badge until the turn
takes it; then the row stays inside the running activity in output order. A
message that misses its turn starts a turn of its own. Alt+Up returns pending
steering and plain follow-ups to the editor without stopping the turn.

## Monitor presentation

A monitor call is a single inline transcript row (`Monitor <description> ·
<command>`) with no output disclosure; failures are viewer-only like other inline
rows. Running monitors stay out of the tasks pane and the still-running watchers
and show only in the watching segment of the row under the composer. The parent
call supplies the label; `MonitorState` marks muted monitors until resume or task
completion. The segment shows labels, elapsed time and mute counts in at most 48
cells, with an ASCII glyph fallback.

## ANTI-PATTERNS

- Do not perform coordinator work from renderers or view models; repeated projection/render calls must be pure.
- Do not derive cell geometry from bytes or scalar character counts, and never split CJK, combining, or ZWJ graphemes.
- Do not expose raw tool JSON, secrets, unsafe hyperlinks, control sequences, or absolute sensitive paths.
- Do not let a lower overlay receive key or pointer input while a modal owns the surface.
- Do not turn replay into a writable shell or invent fallback state absent from recorded events.
- Do not enable terminal features optimistically; restore only successful setup while always leaving raw/alternate-screen safely.
- Do not update snapshots to hide behavioral drift; inspect the rendered contract that changed.
- Do not run Linux PTY signoff as an ordinary test: it is explicitly gated by `HARNESS_TUI_PTY_SIGNOFF=1`.
