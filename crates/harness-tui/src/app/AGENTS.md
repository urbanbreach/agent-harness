# AGENTS: crates/harness-tui/src/app

## OVERVIEW
App state machine modules for live/replay/startup TUI state, event projection, permissions, composer input, session navigation, model switching, transcript state, and operator sidebar data.

Read `../../AGENTS.md` first. Rendering stays in `ui*.rs`; layout/theme math stays in `layout.rs` and `theme.rs`.

## STATE MAP
| Area | Location | Role |
|------|----------|------|
| Aggregate state | `../app.rs` | `AppState`, event ingestion, overlay stack, Deref onto `SessionProjection`. |
| Local secondary UI | `secondary_surfaces.rs` | `SecondarySurfaceState`: status dialog open, selected section, focus, and former sidebar presentation toggles only — never event data. |
| Lifecycle | `lifecycle.rs` | `Focus`, `ShellKind`, `ReviewSurface`, `UiIntent`, startup/post-run actions. |
| Projection | `session_projection.rs`, `session_projection/` | Events → activities, pending permissions, compaction state, memory caps. Sole event-derived truth. |
| Activity/tool rows | `activity.rs`, `tool_call.rs`, `tool_output.rs` | Task/tool/message state shown by transcript and sidebar renderers. |
| Permissions/questions | `permissions.rs`, `permissions/`, `permission_prompt.rs`, `question_prompt.rs` | Pending → decision/confirm → resolved modal lifecycle. |
| Session navigation | `session_stack.rs`, `session_navigation.rs`, `session_history.rs`, `lineage.rs` | Parent/child stack, saved sessions, lineage browser, fork/clone state. |
| Composer | `composer.rs`, `composer_editing.rs`, `prompt_input.rs`, `prompt_history.rs`, `prompt_stash*.rs` | Prompt buffer, history, stashing, submission helpers. |
| Model/auth/toggles | `model_metadata.rs`, `model_switcher.rs`, `model_favorites.rs`, `auth_dialog*`, `toggles.rs` | Runtime choices and overlay state backing. |
| Transcript state | `transcript_state.rs`, `transcript_view.rs`, `transcript_cache.rs` | Scroll, selection, expansion, cache epoch. |

## INTERMODULE CONTRACTS
- Mutate events/activities through `AppState::ingest_event`, `replace_events`, or `SessionProjection` helpers; do not write projection fields ad hoc.
- Event/activity changes must invalidate transcript rendering through the existing render-epoch/cache path.
- `session_stack` is navigation history; `session_projection` is replay-derived event state. Keep them separate.
- `permissions.rs` owns modal/question state transitions; external modules should request actions through its public helpers.
- `UiIntent` additions require runtime intent handling, key/command routing, and deterministic render coverage.
- Prompt history/stash/favorites paths are session-derived local state; do not treat them as replay artifacts.

## TESTS
```bash
cargo nextest run -p harness-tui --test deterministic_render_test
cargo nextest run -p harness-tui --test session_navigation_keybindings_test
cargo nextest run -p harness-tui --test model_switcher_metadata_test
cargo nextest run -p harness-tui --test lineage_view_model_test
```

## ANTI-PATTERNS
- Do not add raw fields to `AppState` when a named sub-state struct/module fits.
- Do not put rendering/layout math into app modules.
- Do not make replay mode emit live submission intents.
- Do not bypass overlay stack pointer/focus rules when adding dialogs.
- Do not update snapshots before deciding whether state behavior or only fixtures changed.
