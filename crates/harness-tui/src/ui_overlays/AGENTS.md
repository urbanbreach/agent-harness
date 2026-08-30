# AGENTS: crates/harness-tui/src/ui_overlays

## OVERVIEW
Render owners for the TUI overlay stack: the dispatch root `../ui_overlays.rs` plus one focused render module per overlay kind. State lives in `../app/`; stack/focus rules live in `../overlay.rs`; geometry comes from `../layout.rs` via `FrameLayoutPlan`.

Read `../../AGENTS.md` and `../app/AGENTS.md` first.

## WHERE TO LOOK
| Overlay kind | Location | Notes |
|------|----------|-------|
| Dispatch | `../ui_overlays.rs` | `render_overlays` walks `app.overlay_stack()` and routes each `OverlayKind`; owns palette/slash/file-mention lists, dim backdrop, subagent actions, error details. |
| Command palette family | `../ui_overlays.rs` | `render_command_palette_overlay` serves palette, session history, model switcher, toggles, lineage, fork selector. |
| Auth dialog | `auth_dialog.rs`, `auth_dialog/` | `render_auth_dialog_overlay`; prompt panel, provider rows, select state. |
| Permission/question | `permission_modal.rs` | Exact line/title/draft helpers (`permission_modal_*`/`question_permission_*`); the dock body renders via `render_inline_permission_dock` in `../ui_chrome.rs` (permission dock arm stays empty here). |
| Status dialog | `status_dialog.rs` | `render_status_dialog_overlay`; operator summary (MCP/LSP/workspace/todos); `exact_test_status_dialog_*` helpers. |
| Session history/lineage | `session_history.rs` | History list, fork selector, rename dialog, lineage browser. |
| Model switcher | `model_switcher.rs` | `render_model_switcher_overlay`; overlay title helper. |
| Other modal owners | `foreign_import_picker.rs`, `memory_browser.rs`, `new_worktree_dialog.rs`, `plan_view.rs`, `prompt_stash_dialog.rs`, `settings_editor.rs`, `theme_dialog.rs`, `toggles_menu.rs`, `worktree_picker.rs` | One focused render module per overlay kind. |
| Dashboard surfaces | `../dashboard/`, `../dashboard_peek/`, `../dashboard_roster/`, `../dashboard_details/` | Not overlay files; the dashboard peek/details/disclosure surfaces render from these modules and stay out of the overlay stack. |
| Overlay stack/focus | `../overlay.rs` | `OverlayKind`, `OverlayState`, `OverlayStack`, `OverlayController`; pointer/focus blocking. |
| Overlay geometry | `../layout.rs`, `../layout/overlays.rs` | `FrameLayoutPlan` overlay rects (`plan.root`, `plan.slash_overlay`, `plan.palette_overlay`). |
| Snapshots | `snapshots/` | Deterministic exact-render expectations for overlay output. |

## CONTRACTS
- `render_overlays` is the single dispatch seam: adding an `OverlayKind` requires app-state visibility, stack/focus handling in `../overlay.rs`, geometry in `FrameLayoutPlan`, and a render arm here.
- Overlay renderers are pure functions of `(&mut Frame, &AppState, &Theme, plan)`; they must not mutate state or emit intents.
- Keep focus/geometry rules centralized: overlays use the rects from `FrameLayoutPlan`; do not re-derive geometry inside render modules.
- Exact render owners export `#[cfg(test)]` helpers so deterministic suites pin line-level output without PTY.
- Replay mode stays read-only: overlay renderers must not gate live-only submission.

## TESTS
```bash
cargo nextest run -p harness-tui --test deterministic_render_test
```
Crate-internal overlay suites live in `../overlay_tests.rs` and `../app/tests/`; owner-suite details in `../../tests/AGENTS.md`.

## ANTI-PATTERNS
- Do not move runtime invariants or app state into overlay render modules.
- Do not bypass the overlay stack: always push/pop via `OverlayController`/`AppState`, never render a modal whose state the app does not reflect.
- Do not widen `ui_overlays.rs` when a focused per-kind module is the safer move.
- Do not claim visual correctness for an overlay without the matching exact-render or PTY evidence.
