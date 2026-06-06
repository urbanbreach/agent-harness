# AGENTS: crates/harness-tui

## OVERVIEW
Ratatui interface crate for startup, live, replay, overlays, app state, shell geometry, transcript rendering, operator sidebar surfaces, and terminal-visual verification.

Read root `AGENTS.md` first. E2E lane details live in `crates/harness-testkit/tests/AGENTS.md`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Runtime entrypoints | `src/lib.rs`, `src/runtime.rs`, `src/event.rs`, `src/event_log.rs` | Startup/live/replay wiring, event ingestion, exact-name shell contract tests. |
| App state | `src/app.rs`, `src/app/` | Activity, permissions, model switcher, session navigation/projection, transcript cache/state, tools, onboarding, file mentions. |
| Rendering shell | `src/ui.rs`, `src/ui_chrome.rs`, `src/ui_composer.rs`, `src/ui_lifecycle.rs`, `src/ui_terminal.rs` | Main surface, chrome, composer, lifecycle states, terminal panel. |
| Transcript rendering | `src/ui_transcript.rs`, `src/ui_transcript_*` | Transcript layout, selection, scrollbar, style, sections, tool rendering, exact tests. |
| Tool/diff rendering | `src/ui_tool_*.rs`, `src/ui_diff*.rs`, `src/ui_syntax_highlight.rs` | Tool rows, metadata, outputs, diffs, syntax highlighting. |
| Overlays/sidebar | `src/overlay.rs`, `src/ui_overlays.rs`, `src/ui_overlays/`, `src/ui_secondary.rs`, `src/ui_secondary/` | Model/session/status/toggles overlays and operator rail/sidebar data. |
| Geometry | `src/layout.rs`, `src/layout/` | Breakpoints, frame plan, pane sizing, overlay geometry, wheel hit areas. |
| Theme/text | `src/theme.rs`, `src/theme/`, `src/text.rs`, `src/time_format.rs` | Color/token system, formatting helpers, shell defaults. |
| Keybindings | `src/keybindings.rs`, `src/keybindings/` | Action map and palette command labels. |
| View models/tests | `src/view_model.rs`, `src/lib_tests/`, `src/tests/`, `tests/` | Presentation shaping and deterministic render/PTY/signoff coverage. |
| Snapshots/signoff | `src/snapshots/`, `tests/snapshots/`, `tests/tui_signoff_manifest_test.rs` | Deterministic render expectations and required signoff manifest. |

## SHELL CONTRACT
- Compose-first home screen: entry point is the composer, not a replay browser.
- Transcript-first session shell: live sessions prioritize transcript rendering with operator sidebar context.
- Operator sidebar is persistent right-hand operator state, file context, orchestration, and tool status on wide layouts.
- Replay mode is read-only; it must not emit live submission intents.
- Debug/inspector surfaces stay secondary; no default tab chrome.

## RENDERING RULES
- Keep layout math in `src/layout.rs`, `src/layout/`, and `src/theme.rs`, not scattered through render helpers.
- Transcript rendering is split across `ui_transcript.rs` and `ui_transcript_*`; keep measured layout, cache keys, and the character-cell selection model coherent across those files.
- Approved rendering stack: `syntect` for syntax highlighting, `imara-diff` for diff visualization.
- Keep tool/transcript/orchestration states structured; do not render opaque text dumps as canonical state.
- Native visual screenshots are local provenance signoff; PTY snapshots and deterministic render tests are the portable safety net.
- Avoid widening `app.rs`, `ui.rs`, or large `ui_transcript_*` files when a focused sibling module is the safer move.

## TESTS
```bash
cargo test -p harness-tui
cargo test -p harness-tui --test deterministic_render_test
cargo test -p harness-tui --test model_switcher_metadata_test
cargo test -p harness-tui --test session_navigation_keybindings_test
cargo test -p harness-tui --test tui_signoff_manifest_test
cargo test -p harness-tui --test pty_e2e
RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo test -p harness-tui --test pty_e2e
scripts/test-lanes.sh signoff-pty
```

Use `cargo insta review -p harness-tui --accept` only after intentionally updating snapshots and checking fixture drift versus behavior drift.

## ANTI-PATTERNS
- Do not hardcode geometry assumptions outside layout/theme contracts.
- Do not change renderer settings, snapshot geometry helpers, font lookup order, focus regions, or capture assumptions casually.
- Do not make replay mode write-capable.
- Do not widen large renderer/app files when a focused sibling module is the safer move.
- Do not update snapshots without checking whether behavior changed or the fixture drifted.
- Do not claim native screenshot signoff from PTY or deterministic snapshot evidence.
