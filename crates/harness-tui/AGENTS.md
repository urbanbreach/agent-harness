# AGENTS: crates/harness-tui

## OVERVIEW
Ratatui interface crate for startup, live, and replay surfaces, plus overlays, geometry contracts, transcript rendering, and snapshot-heavy verification.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| TUI runtime loop and mode entrypoints | `src/lib.rs` | Startup/live/replay wiring, theme override flow |
| App state and event ingestion | `src/app.rs` | Projection-like UI state machine |
| Rendering | `src/ui.rs` | Main surface, overlays, details, transcript |
| Geometry contracts | `src/layout.rs` | Breakpoints and pane sizing |
| Theme tokens | `src/theme.rs` | Shell geometry + color/token system |
| Keybindings | `src/keybindings.rs` | Action map and palette commands |
| PTY regression tests | `tests/pty_e2e.rs`, `tests/snapshots/` | High-signal UI regression lane |

## CONVENTIONS
- Keep layout math in `layout.rs` / `theme.rs`, not scattered through rendering code.
- Preserve replay read-only behavior and live-mode event-driven updates.
- Snapshot/PTY tests are the fast safety net for UI behavior; update them with intent, not casually.
- Theme overrides and keybinding overrides already have plumbing in `src/lib.rs`; reuse it.

## ANTI-PATTERNS
- Do not hardcode geometry assumptions outside the layout/theme contract.
- Do not let replay mode emit live submission intents or other write-capable behavior.
- Do not collapse tool/transcript/orchestration states into opaque text dumps; the UI keeps them structured.

## COMMANDS
```bash
cargo test -p harness-tui
cargo test -p harness-tui pty_e2e
```
