# AGENTS: crates/harness-tui/src/app

## OVERVIEW
App state machine modules for live/replay/startup TUI state: event projection, interaction reducer, composer, session navigation, permissions, model/auth toggles, transcript state, and host/operator probe state.

Read `../../AGENTS.md` first. Rendering stays in `../ui*.rs`; layout/theme math stays in `../layout.rs` and `../theme.rs`; overlay render owners live in `../ui_overlays/AGENTS.md`.

## STATE MAP
| Area | Location | Role |
|------|----------|------|
| Aggregate state | `../app.rs` | `AppState`, event ingestion, overlay stack, Deref onto `SessionProjection`. |
| Interaction | `key_interaction.rs`, `mouse_interaction.rs`, `interaction_reducer/` | Key/mouse intent reduction, render purity, transition tables. |
| Lifecycle | `lifecycle.rs`, `recovery_state.rs`, `pending_live.rs`, `motion.rs`, `secondary_surfaces.rs` | `Focus`, `ShellKind`, `ReviewSurface`, `UiIntent`, startup/post-run and secondary-surface state. |
| Projection | `session_projection.rs`, `session_projection/` | Events → activities, pending permissions, compaction state, memory caps. Sole event-derived truth. |
| Activity/tool rows | `activity.rs`, `tool_call.rs`, `tool_output.rs`, `child_session.rs` | Task/tool/message state shown by transcript and sidebar renderers. |
| Permissions/questions | `permissions.rs`, `permissions/`, `permission_prompt.rs`, `question_prompt.rs` | Pending → decision/confirm → resolved modal lifecycle. |
| Session navigation | `session_stack.rs`, `session_navigation.rs`, `session_history.rs`, `session_pins.rs`, `session_live_routing.rs`, `lineage.rs` | Parent/child stack, saved sessions, lineage browser, fork/clone state. |
| Composer | `composer.rs`, `composer_editing.rs`, `prompt_input.rs`, `prompt_history.rs`, `prompt_stash*.rs`, `file_mentions.rs`, `palette_controller.rs`, `footer_state.rs` | Prompt buffer, history, stashing, mentions, submission helpers. |
| Model/auth/toggles | `model_metadata.rs`, `model_switcher.rs`, `model_favorites.rs`, `auth_dialog*`, `auth_display.rs`, `toggles.rs` | Runtime choices and overlay state backing. |
| Operator probe state | `operator_sidebar.rs`, `shell_status.rs`, `terminal_diagnostics.rs`, `terminal_panel.rs`, `workspace_display.rs`, `notifications.rs`, `tips.rs`, `footer_state.rs` | Host/MCP/LSP/workspace status surfacing for status dialog and secondary panels. |
| Auxiliary dialogs | `settings_editor.rs`, `memory_browser.rs`, `plan_view.rs`, `new_worktree_dialog.rs`, `worktree_picker.rs`, `foreign_import.rs`, `session_slash.rs` | Overlay state backing rendered by `../ui_overlays/`. |
| Transcript state | `transcript_state.rs`, `transcript_view.rs`, `transcript_viewport.rs`, `transcript_cache.rs` | Scroll, selection, expansion, cache epoch. |
| Tests | `tests.rs`, `tests/`, `exact_tests.rs` | In-crate unit/integration suites and exact-render helpers. |

## INTERMODULE CONTRACTS
- Mutate events/activities through `AppState::ingest_event`, `replace_events`, or `SessionProjection` helpers; do not write projection fields ad hoc.
- Event/activity changes must invalidate transcript rendering through the existing render-epoch/cache path.
- `session_stack` is navigation history; `session_projection` is replay-derived event state. Keep them separate.
- `permissions.rs` owns modal/question state transitions; external modules should request actions through its public helpers.
- `UiIntent` additions require runtime intent handling, key/command routing, and deterministic render coverage.
- Prompt history/stash/favorites paths are session-derived local state; do not treat them as replay artifacts.
- Operator probe state (MCP/LSP/workspace/todos) is display data for secondary surfaces; keep it in app state, never recomputed by renderers.
- Interaction changes must keep the reducer render-purity contract green (`interaction_reducer/render_purity.rs`).

## TESTS
```bash
cargo nextest run -p harness-tui --test deterministic_render_test
cargo nextest run -p harness-tui --test session_navigation_keybindings_test
cargo nextest run -p harness-tui --test model_switcher_metadata_test
cargo nextest run -p harness-tui --test lineage_view_model_test
```
Crate-internal app suites run with `cargo nextest run -p harness-tui` (they live under `src/app/tests/`); owner-suite details live in `../../tests/AGENTS.md`.

## ANTI-PATTERNS
- Do not add raw fields to `AppState` when a named sub-state struct/module fits.
- Do not put rendering/layout math into app modules.
- Do not make replay mode emit live submission intents.
- Do not bypass overlay stack pointer/focus rules when adding dialogs.
- Do not update snapshots before deciding whether state behavior or only fixtures changed.
