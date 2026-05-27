# AGENTS: crates/harness-tui

## OVERVIEW
Ratatui interface crate for startup, live, replay, overlays, geometry, transcript rendering, and terminal-visual verification.

Read the workspace root `AGENTS.md` first for search scope and cross-crate verification. Test orchestration details live in `crates/harness-testkit/tests/AGENTS.md`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Runtime entrypoints | `src/lib.rs`, `src/runtime.rs` | Startup/live/replay wiring and exact-name shell contract tests. |
| App state | `src/app.rs`, `src/app/` | Event ingestion and UI state; prefer submodule extraction over widening `src/app.rs`. |
| Rendering | `src/ui.rs`, `src/ui_*.rs`, `src/ui_transcript.rs` | Main surface, chrome, overlays, transcript, secondary views. |
| View models | `src/view_model.rs` | Presentation shaping before rendering. |
| Geometry | `src/layout.rs` | Breakpoints and pane sizing. |
| Theme tokens | `src/theme.rs` | Color/token system and shell geometry defaults. |
| Keybindings | `src/keybindings.rs` | Action map and palette commands. |
| PTY regression tests | `tests/pty_e2e.rs`, `tests/snapshots/` | Deterministic fallback / CI UI regression lane. |

## SHELL CONTRACT
- Compose-first home screen: entry point is the composer, not a replay browser.
- Transcript-first session shell: live sessions prioritize transcript rendering with the operator sidebar for context/tooling.
- Operator sidebar is persistent right-hand operator state, file context, and tool status.
- No default tab chrome; tab-like chrome is opt-in per context.
- Debug/inspector surfaces stay secondary-only, never the primary workflow.

## RENDERING DEPENDENCIES
Approved rendering stack; do not add alternatives without explicit signoff:
- `syntect` for syntax highlighting.
- `imara-diff` for diff visualization.

## CONVENTIONS
- Keep layout math in `src/layout.rs` / `src/theme.rs`, not scattered through rendering code.
- Preserve replay read-only behavior and live-mode event-driven updates.
- Keep tool/transcript/orchestration states structured; do not render opaque text dumps as the canonical state.
- When reducing oversized files, move `#[cfg(test)]` blocks and focused helpers into sibling modules before redesigning runtime behavior.
- Keybinding overrides have plumbing in `src/lib.rs`; keep shipped theming pinned to `Theme::default()` unless a new contract is approved.
- Native Ghostty screenshots are preferred local visual signoff; PTY snapshots are the deterministic safety net.

## ANTI-PATTERNS
- Do not hardcode geometry assumptions outside layout/theme contracts.
- Do not let replay mode emit live submission intents or other write-capable behavior.
- Do not change PTY-facing renderer settings, snapshot geometry helpers, font lookup order, or capture assumptions casually.

## COMMANDS
```bash
cargo test -p harness-tui
cargo test -p harness-tui --test pty_e2e
```
