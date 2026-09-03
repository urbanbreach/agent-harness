# AGENTS: crates/harness-tui

## OVERVIEW
Ratatui interface crate for startup, live, replay, overlays, geometry, transcript rendering, terminal capability, and terminal-visual verification.

Read root `AGENTS.md` first. E2E lane details live in `crates/harness-testkit/tests/AGENTS.md`. Owner-test suites live in `tests/AGENTS.md`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Runtime | `src/lib.rs`, `src/runtime.rs`, `src/runtime_*.rs`, `src/event.rs`, `src/mouse.rs`, `src/gestures/`, `src/input/` | Startup/live/replay wiring, event decode, input ingress and scroll normalization. |
| App state | `src/app.rs`, `src/app/`, `src/app/AGENTS.md` | `AppState`, event ingestion, projection, permissions, composer, sessions, host/operator probe state. |
| Rendering | `src/ui.rs`, `src/ui_chrome.rs`, `src/ui_*.rs`, `src/ui_composer/`, `src/ui_secondary.rs`, `src/ui_secondary_events_tab.rs` | Main surface, chrome, composer, secondary views, live-turn status. |
| Transcript | `src/ui_transcript.rs`, `src/ui_transcript_*.rs`, `src/transcript_blocks/`, `src/transcript_block_viewer/`, `src/transcript_identity/`, `src/transcript_integration/`, `src/transcript_pager/`, `src/transcript_scroll/`, `src/transcript_selection/`, `src/transcript_timeline/` | Sections, entries, grammar blocks, selection, scroll, pager, timeline, media. |
| Overlays/secondary | `src/overlay.rs`, `src/ui_overlays.rs`, `src/ui_overlays/`, `src/ui_overlays/AGENTS.md` | `OverlayKind`/stack, modal/palette/dialog render owners. |
| Layout/responsive/theme | `src/layout.rs`, `src/layout/`, `src/responsive.rs`, `src/responsive/`, `src/shell_geometry/`, `src/theme.rs`, `src/theme/`, `src/theme_system/`, `src/theme_family/`, `src/theme_tokens.rs`, `src/theme_tokens/`, `src/viewport.rs` | Frame plan, breakpoints, cursor/hit maps, tokens, family resolution. |
| Keybindings/slash | `src/keybindings.rs`, `src/keybindings/`, `src/slash.rs`, `src/slash/` | Action map, palette model, and slash commands. |
| Terminal | `src/terminal.rs`, `src/terminal/`, `src/terminal_title/`, `src/terminal_notifications/` | Capability probe, decode, frame clock, title/notifications, output. |
| Dashboard | `src/dashboard/`, `src/dashboard_controls/`, `src/dashboard_details/`, `src/dashboard_dispatch/`, `src/dashboard_integration/`, `src/dashboard_peek/`, `src/dashboard_roster/` | Session dashboard read model, eligibility, peek, dispatch, controls. |
| Presentation | `src/presentation.rs`, `src/presentation/`, `src/view_model.rs` | Render demand/cause tracking and presentation shaping. |
| Snapshots | `src/snapshots/`, `src/ui_overlays/snapshots/`, `tests/snapshots/` | Deterministic render expectations. |
| Render test helpers | `src/lib_tests/`, `src/lib_tests.rs`, `src/overlay_tests.rs`, `src/render_test.rs` | Deterministic shell/view fixtures and overlay regression helpers. |

## SHELL CONTRACT
- Compose-first home screen: entry point is the composer, not a replay browser.
- Transcript-first live shell: full-width scrollback/transcript above a bottom composer; no persistent right-hand operator sidebar as primary chrome.
- Operator facts (MCP/LSP/modified files/child tasks/todos) remain reachable via secondary surfaces: status dialog (`open_status_dialog` / `toggle_operator_sidebar` alias), details overlay, palette, slash commands, and transcript blocks.
- Replay mode is read-only; it must not emit live submission intents. Replay may still use a details sidebar for inspection.
- Debug/inspector surfaces stay secondary; no default tab chrome.
- Topology contract owner: `tests/shell_topology_contract_test.rs` (viewports 80x24 / 100x30 / 120x40 and width >= 121).

## RENDERING RULES
- Keep layout math in `src/layout.rs` and `src/theme.rs`, not scattered through render helpers.
- Transcript rendering is split across `src/ui_transcript.rs` and `src/ui_transcript_*`; keep measured layout, cache keys, and the character-cell selection model coherent across those files.
- Approved rendering stack: `syntect` for syntax highlighting, `imara-diff` for diff visualization.
- Keep tool/transcript/orchestration states structured; do not render opaque text dumps as canonical state.
- Native visual screenshots are local provenance signoff; PTY snapshots are deterministic safety net.

## TESTS
```bash
cargo nextest run -p harness-tui
cargo nextest run -p harness-tui --test deterministic_render_test
cargo nextest run -p harness-tui --test model_switcher_metadata_test
cargo nextest run -p harness-tui --test session_navigation_keybindings_test
cargo nextest run -p harness-tui --test pty_e2e --ignore-default-filter
RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --test pty_e2e --test-threads 1 --ignore-default-filter
```
Owner-suite commands and conventions live in `tests/AGENTS.md`. Use `cargo insta review -p harness-tui --accept` only after intentionally updating snapshots.

## ANTI-PATTERNS
- Do not hardcode geometry assumptions outside layout/theme contracts.
- Do not change renderer settings, snapshot geometry helpers, font lookup order, focus regions, or capture assumptions casually.
- Do not make replay mode write-capable.
- Do not widen large renderer/app files when a focused sibling module is the safer move.
- Do not update snapshots without checking whether behavior changed or the fixture drifted.
