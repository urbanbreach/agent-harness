# AGENTS: crates/harness-tui

## OVERVIEW
Ratatui interface crate for startup, live, replay, overlays, slash/status/model/toggle dialogs, geometry, transcript rendering, and terminal-visual verification.

Read the workspace root `AGENTS.md` first for search scope and cross-crate verification. Test orchestration details live in `crates/harness-testkit/tests/AGENTS.md`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Runtime entrypoints | `src/lib.rs`, `src/runtime.rs` | Startup/live/replay wiring and exact-name shell contract tests. |
| App state | `src/app.rs`, `src/app/` | Event ingestion, session navigation/projection, and UI state; prefer submodule extraction over widening `app.rs`. |
| Rendering | `src/ui.rs`, `src/ui_*.rs`, `src/ui_transcript.rs` | Main surface, chrome, overlays, transcript, secondary views. |
| View models | `src/view_model.rs` | Presentation shaping before rendering. |
| Geometry | `src/layout.rs` | Breakpoints and pane sizing. |
| Theme tokens | `src/theme.rs` | Color/token system and shell geometry defaults. |
| Keybindings | `src/keybindings.rs` | Action map and palette commands. |
| Session navigation | `src/app/session_navigation.rs`, `src/app/session_projection.rs` | `/resume`, `/new`, lineage tree, fork/clone display state. |
| Snapshot tests | `tests/snapshots/`, `src/snapshots/` | Deterministic renderer expectations. |

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
- Keep layout math in `layout.rs` / `theme.rs`, not scattered through rendering code.
- Preserve replay read-only behavior and live-mode event-driven updates.
- Keep tool/transcript/orchestration states structured; do not render opaque text dumps as the canonical state.
- Slash commands and `$` overlay items are typed workflow/UI intents; do not route them through shell snippets.
- When reducing oversized files, move `#[cfg(test)]` blocks and focused helpers into sibling modules before redesigning runtime behavior.
- Keybinding overrides have plumbing in `src/lib.rs`; keep shipped theming pinned to `Theme::default()` unless a new contract is approved.
- Native Ghostty screenshots are preferred local visual signoff; PTY snapshots are the deterministic safety net.
- Keep animation deadlines in `src/runtime.rs` independent from redraw cadence; high-frequency mouse movement may request redraws continuously and must never postpone animation ticks.

## ANTI-PATTERNS
- Do not hardcode geometry assumptions outside layout/theme contracts.
- Do not let replay mode emit live submission intents or other write-capable behavior.
- Do not change PTY-facing renderer settings, snapshot geometry helpers, font lookup order, or capture assumptions casually.

## COMMANDS
```bash
cargo test -p harness-tui
cargo test -p harness-tui --lib
cargo test -p harness-tui --test model_switcher_metadata
cargo test -p harness-tui --test session_navigation_keybindings
RUST_TEST_THREADS=1 cargo test -p harness-tui --test pty_e2e
scripts/test-lanes.sh signoff-pty
```
