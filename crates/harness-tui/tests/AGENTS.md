# AGENTS: crates/harness-tui/tests

## OVERVIEW
Owner-test suites for the TUI crate: deterministic rendering, PTY owner lanes, and per-family owners (model/session, dashboard, composer, transcript, terminal/theme). Crate-internal unit suites live under `../src/app/tests/`, `../src/lib_tests/`, and `../src/tests.rs`.

Read `../AGENTS.md` first. PTY/live/native lane details live in `crates/harness-testkit/tests/AGENTS.md`.

## WHERE TO LOOK
| Suite | Location | Notes |
|-------|----------|-------|
| Deterministic render | `deterministic_render_test.rs`, `support/deterministic_render_fixtures.rs`, `support/p21_tool_display_fixtures.rs` | Main snapshot owner; shell/composer/replay/permission states without PTY. |
| PTY owner lane | `pty_e2e.rs`, `support/pty_e2e_impl.rs` | Env-gated (`HARNESS_TUI_PTY_SIGNOFF=1`), single-threaded deterministic PTY evidence. |
| Model/session | `model_switcher/`, `model_switcher_metadata_test.rs`, `support/model_switcher_fixtures.rs`; `session_navigation_keybindings_test.rs`, `lineage_view_model_test.rs`, `lineage_view_model_dialog_test.rs` | Provider/model switcher and replay/session/lineage navigation behavior. |
| Dashboard | `dashboard_test.rs`, `dashboard_controls_test.rs`, `dashboard_details_test.rs`, `dashboard_dispatch_test.rs`, `dashboard_integration_test.rs`, `dashboard_peek_test.rs`, `dashboard_roster_test.rs`, `dashboard_reachability_test.rs` | Read-model, controls/details/dispatch, peek, roster, and reachability coverage. |
| Composer | `composer_atoms_test.rs`, `composer_editing_test.rs`, `composer_integration_test.rs`, `production_composer_reachability_test.rs` | Prompt buffer/editing/integration and reachability. |
| Transcript | `transcript_blocks_test.rs`, `transcript_block_viewer_test.rs`, `transcript_identity_test.rs`, `transcript_integration_test.rs`, `transcript_pager_test.rs`, `transcript_scroll_test.rs`, `transcript_selection_test.rs`, `transcript_timeline_test.rs`, `scroll_normalizer_test.rs`, `transcript_live_adapter_test.rs`, `transcript_incremental_performance_test.rs` | Grammar blocks, scroll/selection, live adapter, and incremental rendering. |
| Terminal/theme/runtime | `terminal_*.rs`, `terminal_title_*.rs`, `frame_output_presenter_test.rs`, `frame_scheduler_test.rs`, `theme_family_test.rs`, `theme_system_test.rs`, `runtime_*.rs`, `live_turn_*.rs`, `settled_turn_metadata_test.rs`, `pre_response_working_indicator_test.rs`, `presentation_trace_test.rs` | Capability decode, frame output/title, theme families, runtime scheduling, and live-turn state machines. |
| Support helpers | `support/` | Shared deterministic fixtures and PTY implementation. |
| Snapshots | `snapshots/` | Committed insta snapshots for deterministic and responsive suites. |

## CONVENTIONS
- Deterministic suites are the default lane; PTY owner tests are env-gated and single-threaded (`HARNESS_TUI_PTY_SIGNOFF=1`).
- Update snapshots only after deciding behavior (not fixtures) changed; use `cargo insta review -p harness-tui --accept`.
- Test support helpers stay runtime-independent: no wall-clock, network, display, or harness invocation.

## COMMANDS
```bash
cargo nextest run -p harness-tui
cargo nextest run -p harness-tui --test deterministic_render_test
RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --test pty_e2e --test-threads 1
cargo insta review -p harness-tui --accept
```

## ANTI-PATTERNS
- Do not claim PTY/live/native visual evidence without the matching lane and artifact provenance.
- Do not treat `support/` or `fixtures/` as product code; keep them test-local.
- Do not edit committed snapshots to paper over behavior changes.
- Do not add host-specific geometry or font assumptions to shared fixtures.
