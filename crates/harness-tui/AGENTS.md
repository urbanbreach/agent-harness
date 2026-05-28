# AGENTS: crates/harness-tui

## OVERVIEW
Ratatui interface crate for startup, live, replay, overlays, geometry, transcript rendering, and terminal-visual verification.

Read root `AGENTS.md` first. E2E lane details live in `crates/harness-testkit/tests/AGENTS.md`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Runtime entrypoints | `src/lib.rs`, `src/runtime.rs` | Startup/live/replay wiring and exact-name shell contract tests. |
| App state | `src/app.rs`, `src/app/` | Event ingestion, overlays, permissions, session navigation; prefer submodule extraction over widening `app.rs`. |
| Rendering | `src/ui.rs`, `src/ui_*.rs`, `src/ui_transcript.rs` | Main surface, chrome, overlays, transcript, secondary views. |
| Geometry | `src/layout.rs` | Breakpoints, frame plan, pane sizing, wheel hit areas. |
| Theme tokens | `src/theme.rs` | Color/token system and shell geometry defaults. |
| Keybindings | `src/keybindings.rs` | Action map and palette command labels. |
| View models | `src/view_model.rs` | Presentation shaping before rendering. |
| Snapshots | `src/snapshots/`, `tests/snapshots/` | Deterministic render expectations. |

## SHELL CONTRACT
- Compose-first home screen: entry point is the composer, not a replay browser.
- Transcript-first session shell: live sessions prioritize transcript rendering with operator sidebar context.
- Operator sidebar is persistent right-hand operator state, file context, and tool status.
- Replay mode is read-only; it must not emit live submission intents.
- Debug/inspector surfaces stay secondary; no default tab chrome.

## RENDERING RULES
- Keep layout math in `src/layout.rs` and `src/theme.rs`, not scattered through render helpers.
- `ui_transcript.rs` uses measured layout, cache keys, and a character-cell selection model; update tests when changing any of them.
- Approved rendering stack: `syntect` for syntax highlighting, `imara-diff` for diff visualization.
- Keep tool/transcript/orchestration states structured; do not render opaque text dumps as canonical state.
- Native visual screenshots are local provenance signoff; PTY snapshots are deterministic safety net.

## TESTS
```bash
cargo test -p harness-tui
cargo test -p harness-tui --test deterministic_render_test
cargo test -p harness-tui --test pty_e2e
RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo test -p harness-tui --test pty_e2e
```
Use `cargo insta review -p harness-tui --accept` only after intentionally updating snapshots.

## ANTI-PATTERNS
- Do not hardcode geometry assumptions outside layout/theme contracts.
- Do not change renderer settings, snapshot geometry helpers, font lookup order, focus regions, or capture assumptions casually.
- Do not make replay mode write-capable.
- Do not widen large renderer/app files when a focused sibling module is the safer move.
