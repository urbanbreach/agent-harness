# Sources ledger

Local source paths are the primary sources for this comparison.

[S001] `crates/harness-tui/src/ui.rs` — Harness shell render pipeline; observed 2026-08-31.
[S002] `crates/harness-tui/src/layout.rs` — Harness shell/dock/sidebar geometry; observed 2026-08-31.
[S003] `crates/harness-tui/src/app/transcript_viewport.rs` — Harness transcript viewport state; observed 2026-08-31.
[S004] `crates/harness-tui/src/ui_transcript_block_grammar.rs` — Harness transcript grouping grammar; observed 2026-08-31.
[S005] `crates/harness-tui/src/transcript_scroll/follow.rs` — Harness follow/page-flip behavior; observed 2026-08-31.
[S006] `crates/harness-tui/src/dashboard/model.rs` — Harness dashboard groups and activities; observed 2026-08-31.
[S007] `crates/harness-tui/src/ui_secondary/sidebar_data.rs` — Harness operator rail derivation; observed 2026-08-31.
[S008] `crates/harness-tui/src/ui_chrome.rs` — Harness header/footer identity and status; observed 2026-08-31.
[S009] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/app_view.rs` — Grok top-level views; observed 2026-08-31.
[S010] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/agent.rs` — Grok pane stack and scrollback floor; observed 2026-08-31.
[S011] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/dashboard/state.rs` — Grok grouping, pins, peek/attach; observed 2026-08-31.
[S012] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/timeline.rs` — Grok timeline rail; observed 2026-08-31.
[S013] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/groups.rs` — Grok verb/truncation groups; observed 2026-08-31.
[S014] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/mod.rs` — Grok streaming entry mutation; observed 2026-08-31.
[S015] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/status_line/segments.rs` — Grok status segments; observed 2026-08-31.
[S016] `crates/harness-tui/src/shell_geometry/hit_map.rs` — Harness z-ordered pointer hit map; observed 2026-08-31.
[S017] `crates/harness-tui/src/overlay.rs` — Harness global overlay precedence; observed 2026-08-31.
[S018] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/agent_view/render.rs` — Grok segmented status, modal, and shortcut composition; observed 2026-08-31.
[S019] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/status_bar.rs` — Grok segmented top status bar; observed 2026-08-31.
[S020] `crates/harness-tui/src/ui_transcript_sections.rs` — Harness tool-call coalescing; observed 2026-08-31.
[S021] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/verb_group.rs` — Grok tool verb grouping/truncation; observed 2026-08-31.
[S022] `crates/harness-tui/src/ui_transcript_layout.rs` — Harness sticky user prompt; observed 2026-08-31.
[S023] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/sticky.rs` — Grok per-turn sticky prompt behavior; observed 2026-08-31.
[S024] `crates/harness-tui/src/ui_markdown_table.rs` — Harness table renderer; observed 2026-08-31.
[S025] `inspirations/grok-build/crates/codegen/xai-grok-markdown/src/render.rs` — Grok rich table renderer; observed 2026-08-31.
[S026] `crates/harness-tui/src/ui_markdown.rs` — Harness markdown link rendering; observed 2026-08-31.
[S027] `inspirations/grok-build/crates/codegen/xai-grok-markdown/src/parse.rs` — Grok rich link/citation parsing; observed 2026-08-31.
[S028] `crates/harness-tui/src/ui_streaming_markdown.rs` — Harness open-fence streaming renderer; observed 2026-08-31.
[S029] `inspirations/grok-build/crates/codegen/xai-grok-markdown/src/streaming.rs` — Grok incremental markdown streaming; observed 2026-08-31.
[S030] `crates/harness-tui/src/app/key_interaction.rs` — Harness transcript key actions; observed 2026-08-31.
[S031] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/actions/defaults.rs` — Grok response-navigation bindings; observed 2026-08-31.
[S032] `crates/harness-tui/src/app/prompt_input.rs` — Harness paste/input behavior; observed 2026-08-31.
[S033] `crates/harness-tui/src/lifecycle.rs` — Harness clickable stop lifecycle; observed 2026-08-31.
[S034] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/agent_view/prompt.rs` — Grok active-turn cancel behavior; observed 2026-08-31.
[S035] `crates/harness-tui/src/slash/commands.rs` — Harness static slash commands; observed 2026-08-31.
[S036] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/slash/registry.rs` — Grok mutable command registry; observed 2026-08-31.
[S037] `crates/harness-tui/src/theme.rs` — Harness static status glyphs; observed 2026-08-31.
[S038] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/turn_status.rs` — Grok animated turn status; observed 2026-08-31.
[S039] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/modal.rs` — Grok modal parent restoration; observed 2026-08-31.
[S040] `crates/harness-tui/src/dashboard_integration/responsive.rs` — Harness capped dashboard overlay geometry; observed 2026-08-31.
[S041] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/app_view.rs` — Grok full-surface dashboard render; observed 2026-08-31.
[S042] `crates/harness-tui/src/keybindings/palette_model.rs` — Harness multiline palette action; observed 2026-08-31.
[S043] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/slash/commands/multiline.rs` — Grok multiline toggle; observed 2026-08-31.
[S044] `crates/harness-tui/src/app/session_navigation.rs` — Harness slash completion behavior; observed 2026-08-31.
[S045] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/agent_view/prompt.rs` — Grok command acceptance/execution split; observed 2026-08-31.
[S046] `crates/harness-tui/src/app/task_pane.rs` — Harness task pane; observed 2026-08-31.
[S047] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/tasks_pane.rs` — Grok production task pane; observed 2026-08-31.
[S048] `crates/harness-tui/src/ui_overlays.rs` — Harness overlay chrome; observed 2026-08-31.
[S049] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/modal_window.rs` — Grok modal chrome; observed 2026-08-31.
[S050] `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/theme/groknight.rs` — Grok neutral theme palette; observed 2026-08-31.
[S051] `crates/harness-tui/src/ui_composer/bordered.rs` — Harness bordered composer; observed 2026-08-31.
[S052] `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/render/prompt_widget/mod.rs` — Grok borderless prompt; observed 2026-08-31.
[S053] `crates/harness-tui/src/input/scroll_normalizer.rs` — Harness input-bounded scroll behavior; observed 2026-08-31.
[S054] `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/input/mouse.rs` — Grok scroll coast/momentum; observed 2026-08-31.
[S055] `crates/harness-tui/src/app/motion.rs` — Harness global reduced-motion contract; observed 2026-08-31.
[S056] `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/appearance/config.rs` — Grok per-block animation settings; observed 2026-08-31.
[S057] `crates/harness-tui/src/scheduling/runtime_wheel.rs` — Harness wheel-input clamping; observed 2026-08-31.
[S058] `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/input/mouse.rs` — Grok bounded wheel backlog; observed 2026-08-31.
[S059] `crates/harness-tui/src/transcript_integration/incremental/reflow.rs` — Harness full transcript reflow; observed 2026-08-31.
[S060] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/mod.rs` — Grok visible-only exact measurement; observed 2026-08-31.
[S061] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/layout.rs` — Grok binary-searched paint window; observed 2026-08-31.
[S062] `crates/harness-tui/src/app/session_projection.rs` — Harness provider error presentation; observed 2026-08-31.
[S063] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/error_display.rs` — Grok curated error taxonomy; observed 2026-08-31.
[S064] `crates/harness-tui/src/terminal/fallback.rs` — Harness mouse capability gating; observed 2026-08-31.
[S065] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/event_loop.rs` — Grok mouse-reporting override; observed 2026-08-31.
[S066] `crates/harness-tui/src/app/lifecycle.rs` — Harness disconnect lifecycle; observed 2026-08-31.
[S067] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/agent_view/session.rs` — Grok reconnect replay/rollback; observed 2026-08-31.
[S068] `crates/harness-tui/src/startup_logo.rs` — Harness startup logo; observed 2026-08-31.
[S069] `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/welcome/logo.rs` — Grok animated wordmark; observed 2026-08-31.
[S070] `crates/harness-tui/src/terminal/frame_output/backend.rs` — Harness emitted-frame capture; observed 2026-08-31.
[S071] `inspirations/grok-build/crates/codegen/xai-grok-pager-pty-harness/src/screen.rs` — Grok emulator-backed screen oracle; observed 2026-08-31.
