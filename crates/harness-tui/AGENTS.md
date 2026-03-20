# AGENTS: crates/harness-tui

## OVERVIEW
Ratatui interface crate for startup, live, and replay surfaces, plus overlays, geometry contracts, transcript rendering, and snapshot-heavy verification.

Read the workspace root `AGENTS.md` first for crate ownership, search exclusions, and the cross-crate verification matrix.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| TUI runtime loop and mode entrypoints | `src/lib.rs` | Startup/live/replay wiring and exact-name shell contract tests |
| App state and event ingestion | `src/app.rs`, `src/app/` | Projection-like UI state machine; prefer submodule extraction over widening `app.rs` |
| Rendering | `src/ui.rs` | Main surface, overlays, secondary surfaces, transcript |
| Geometry contracts | `src/layout.rs` | Breakpoints and pane sizing |
| Theme tokens | `src/theme.rs` | Shell geometry + color/token system |
| Keybindings | `src/keybindings.rs` | Action map and palette commands |
| PTY regression tests | `tests/pty_e2e.rs`, `tests/snapshots/` | High-signal UI regression lane |

## SHELL CONTRACT (T14+)
The UI implements a strict surface hierarchy:
- **Compose-first home screen**: entry point is the composer, not a replay browser.
- **Transcript-first session shell**: live sessions prioritize transcript rendering with the operator sidebar for context/tooling.
- **Operator sidebar**: persistent right-hand surface for operator state, file context, and tool status.
- **No default tab chrome**: surfaces are chromeless by default; tab-like chrome is opt-in per context.
- **No debug inspector in the primary path**: debug/inspector surfaces stay secondary-only and should not be advertised as the default workflow.

## RENDERING DEPENDENCIES
Approved rendering stack (do not add alternatives without explicit signoff):
- `syntect` for syntax highlighting
- `imara-diff` for diff visualization

## CONVENTIONS
- Keep layout math in `layout.rs` / `theme.rs`, not scattered through rendering code.
- Preserve replay read-only behavior and live-mode event-driven updates.
- When reducing oversized files, move `#[cfg(test)]` blocks and focused state helpers into sibling modules before redesigning runtime behavior.
- Snapshot/PTY tests are the fast safety net for UI behavior; update them with intent, not casually.
- Keybinding overrides already have plumbing in `src/lib.rs`; keep shipped theming pinned to `Theme::default()` unless a new contract is explicitly approved.
- Screenshot-generated PTY/live-visual artifacts are the primary verification workflow; prefer visual parity over text assertions.
- Key shell contract terms: compose-first, transcript-first, operator sidebar, no default tab chrome, no debug inspector in primary path.

## ANTI-PATTERNS
- Do not hardcode geometry assumptions outside the layout/theme contract.
- Do not let replay mode emit live submission intents or other write-capable behavior.
- Do not collapse tool/transcript/orchestration states into opaque text dumps; the UI keeps them structured.

## COMMANDS
```bash
cargo test -p harness-tui
cargo test -p harness-tui pty_e2e
```
