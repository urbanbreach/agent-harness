#![allow(
    deprecated,
    reason = "deprecated compaction event variants kept for backward compatibility tests"
)]

use super::operator_rail_test_fixtures::*;
use super::*;
use crate::app::{ActiveContextUsage, ActivityUsage};
use crate::UnwrapOrAbort;

pub(crate) fn exact_test_operator_rail_section_model_builds_pinned_summary() {
    let _guard = operator_sidebar_config_test_guard();
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_integrations_config(
        harness_core::config::IntegrationsConfig::default(),
    );
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let app = operator_rail_test_app();
    let model = build_operator_rail_model(&app);

    assert_eq!(model.title, None);
    assert_eq!(model.body.sections[0].heading(), "▼ MCP");
    assert_eq!(model.body.sections[1].heading(), "▼ LSP");
    assert_eq!(model.body.sections[2].heading(), "▼ Modified Files");
    assert_eq!(
        model.body.sections[2].item_texts(),
        ["src/ui_secondary.rs".to_string()]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_compaction_applied_updates_active_context_usage_estimate() {
    let mut app = AppState::new_live(None, false, None);
    app.active_context_usage = Some(ActiveContextUsage::estimate(500));
    app.activities.push_back(ActivityEntry {
        request_id: "req_000001".to_string(),
        profile_label: "build".to_string(),
        model_id: "model-1".to_string(),
        provider_id: "default".to_string(),
        status: ActivityStatus::Done,
        user_message: None,
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: String::new(),
        usage: Some(ActivityUsage {
            prompt_tokens: 400,
            completion_tokens: 100,
            total_tokens: 500,
        }),
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 2,
        first_mono_ms: 10,
        last_mono_ms: 20,
        revision: 0,
    });
    assert_eq!(
        app.active_context_usage(),
        Some(ActiveContextUsage::estimate(500))
    );
    assert_eq!(app.compaction_usage_metrics().completed_count, 0);

    app.ingest_event(operator_rail_test_event(
        3,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        harness_core::event::EventV1::CompactionApplied(
            harness_core::event::CompactionAppliedEvent {
                checkpoint_id: "checkpoint_000003".to_string(),
                agent_id: "agent_000001".to_string(),
                through_seq: 2,
                through_request_id: Some("req_000001".to_string()),
                tokens_before_estimate: Some(500),
                tokens_after_estimate: Some(120),
                summary_tokens_estimate: Some(80),
                compacted_turns: Some(1),
                preserved_turns: Some(1),
                reduction_tokens_estimate: Some(380),
                reduction_percent_estimate: Some(76),
                estimate_source: None,
            },
        ),
    ));

    assert_eq!(
        app.active_context_usage(),
        Some(ActiveContextUsage::estimate(120))
    );
    let metrics = app.compaction_usage_metrics();
    assert_eq!(metrics.completed_count, 1);
    assert_eq!(metrics.summary_tokens_estimate, 80);
    assert_eq!(metrics.reduction_tokens_estimate, 380);
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_sanitizes_control_chars_in_sidebar_strings() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_control_chars".to_string(),
            path: "src/ui_secondary.rs\nModified Files · 99".to_string(),
            new_file_digest: "digest-edit-control".to_string(),
            diff_rel_path: Some("artifacts/\tcontrol\nrows.diff".to_string()),
            diff_digest: Some("digest-edit-control-diff".to_string()),
        }),
    ));

    let model = build_operator_rail_model(&app);
    let modified_files = model
        .body
        .sections
        .iter()
        .find_map(|section| match section {
            OperatorRailBodySection::ModifiedFiles { items, .. } => Some(items),
            _ => None,
        })
        .unwrap_or_abort();

    assert_eq!(
        modified_files
            .iter()
            .map(OperatorRailItem::display_text)
            .collect::<Vec<_>>(),
        ["src/ui_secondary.rs Modified Files · 99".to_string()]
    );
    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("▼ Modified Files"));
    assert!(sidebar.contains("src/ui_secondary.rs Modified Files · 99"));
    assert!(!sidebar.contains("rows.diff\n▼ LSP"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_section_model_hides_empty_sources_but_preserves_order() {
    let _guard = operator_sidebar_config_test_guard();
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_integrations_config(
        harness_core::config::IntegrationsConfig::default(),
    );
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let empty_model = build_operator_rail_model(&AppState::new_live(None, false, None));
    assert_eq!(
        empty_model
            .body
            .sections
            .iter()
            .map(OperatorRailBodySection::heading)
            .collect::<Vec<_>>(),
        vec![
            "▼ MCP".to_string(),
            "▼ LSP".to_string(),
            "▶ Modified Files".to_string(),
        ]
    );
    assert_eq!(empty_model.body.sections[0].heading(), "▼ MCP");
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_matches_sidebar_text_styles() {
    let _guard = operator_sidebar_config_test_guard();
    let mut integrations = harness_core::config::IntegrationsConfig::default();
    integrations.remote_search.endpoint = "https://mcp.exa.ai/mcp".to_string();
    integrations.mcp.servers.insert(
        "fixture".to_string(),
        harness_core::config::McpServerConfig::Http {
            endpoint: "https://example.com/mcp".to_string(),
            headers: std::collections::BTreeMap::new(),
            timeout_secs: 30,
            enabled: true,
        },
    );
    harness_core::config::set_registered_integrations_config(integrations);
    harness_core::config::set_registered_mcp_server_connection_states(
        std::collections::BTreeMap::from([(
            "fixture".to_string(),
            harness_core::config::McpServerConnectionState::Connected,
        )]),
    );
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let session_path = std::env::temp_dir().join(format!(
        "harness-tui-sidebar-style-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_abort()
            .as_nanos()
    ));
    std::fs::create_dir_all(session_path.join("artifacts")).unwrap_or_abort();
    std::fs::write(
        session_path.join("artifacts/edit-1.diff"),
        "diff --git a/src/ui_secondary.rs b/src/ui_secondary.rs\n--- a/src/ui_secondary.rs\n+++ b/src/ui_secondary.rs\n@@ -1 +1 @@\n-old\n+new\n",
    )
    .unwrap_or_abort();

    crate::app::set_pending_live_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("worker", "mock:model-1")
            .with_mode_label("Demo"),
    );
    let mut app = AppState::new_live(Some(session_path.clone()), false, None);
    app.ingest_event(operator_rail_test_event(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_style_1".to_string(),
            path: "src/ui_secondary.rs".to_string(),
            new_file_digest: "digest-style-edit-1".to_string(),
            diff_rel_path: Some("artifacts/edit-1.diff".to_string()),
            diff_digest: Some("digest-style-diff-1".to_string()),
        }),
    ));
    let lsp_request_id = "req_style_lsp";
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        lsp_request_id,
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: lsp_request_id.into(),
                text: "Find the Rust definition".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        lsp_request_id,
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_style_lsp".into(),
                tool_id: "code.lsp".to_string(),
                args_summary: r#"{"operation":"goto_definition","path":"src/ui_secondary.rs"}"#
                    .to_string(),
                args_digest: "digest-style-lsp".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("code.lsp".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        lsp_request_id,
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_style_lsp".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Found definition in src/ui_secondary.rs".to_string()),
                output_digest: Some("digest-style-lsp-output".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("code.lsp".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));

    let theme = *app.theme();
    let lines = operator_sidebar_lines_for_test(&app);

    let mcp_connected = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("fixture Connected")
        })
        .unwrap_or_abort();
    assert_eq!(mcp_connected.spans[0].style.fg, Some(theme.status.success));
    assert_eq!(mcp_connected.spans[1].style.fg, Some(theme.text.primary));
    assert_eq!(mcp_connected.spans[2].style.fg, Some(theme.text.secondary));

    let mcp_disconnected = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("websearch Disconnected")
        })
        .unwrap_or_abort();
    assert_eq!(mcp_disconnected.spans[0].style.fg, Some(theme.status.error));
    assert_eq!(mcp_disconnected.spans[1].style.fg, Some(theme.text.primary));
    assert_eq!(
        mcp_disconnected.spans[2].style.fg,
        Some(theme.text.secondary)
    );

    let lsp_line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("rust")
        })
        .unwrap_or_abort();
    assert_eq!(lsp_line.spans[0].style.fg, Some(theme.status.success));
    assert_eq!(lsp_line.spans[1].style.fg, Some(theme.text.secondary));

    let modified_file_line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("src/ui_secondary.rs · +1 -1")
        })
        .unwrap_or_abort();
    assert_eq!(
        modified_file_line.spans[0].style.fg,
        Some(theme.text.secondary)
    );
    assert_eq!(
        modified_file_line.spans[1].style.fg,
        Some(theme.text.secondary)
    );
    assert_eq!(
        modified_file_line.spans[3].style.fg,
        Some(theme.status.success)
    );
    assert_eq!(
        modified_file_line.spans[5].style.fg,
        Some(theme.status.error)
    );

    std::fs::remove_dir_all(session_path).unwrap_or_abort();
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_section_model_surfaces_pending_permissions_first() {
    let app = operator_rail_test_app();
    let model = build_operator_rail_model(&app);
    assert_eq!(model.body.sections[0].heading(), "▼ MCP");
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_uses_generated_session_title() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req-title",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-title".into(),
                text: "Please implement the flaky retry button".to_string(),
            },
        ),
    ));

    let theme = *app.theme();
    let pending =
        build_operator_rail_title_text(build_operator_rail_model(&app).title.as_ref(), &theme, 80);
    assert_eq!(
        pending.lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>(),
        "Generating title..."
    );
    assert_eq!(
        pending.lines[0].spans[0].style.fg,
        Some(theme.text.secondary)
    );

    app.ingest_event(operator_rail_test_event(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::SessionTitleUpdated(
            harness_core::event::SessionTitleUpdatedEvent {
                title: "Retry button implementation".to_string(),
            },
        ),
    ));

    let generated =
        build_operator_rail_title_text(build_operator_rail_model(&app).title.as_ref(), &theme, 80);
    assert_eq!(
        generated.lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>(),
        "Retry button implementation"
    );
    assert_eq!(
        generated.lines[0].spans[0].style.fg,
        Some(theme.text.primary)
    );
    assert!(operator_sidebar_text_for_test(&app)
        .join("\n")
        .contains("Retry button implementation"));
    assert!(!operator_sidebar_text_for_test(&app)
        .join("\n")
        .contains("Please implement the flaky retry button"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_section_model_separates_mcp_from_native_tool_activity() {
    let _guard = operator_sidebar_config_test_guard();
    let mut integrations = harness_core::config::IntegrationsConfig::default();
    integrations.remote_search.endpoint = "https://mcp.exa.ai/mcp".to_string();
    harness_core::config::set_registered_integrations_config(integrations);
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let app = operator_rail_activity_test_app();
    let model = build_operator_rail_model(&app);
    assert_eq!(model.body.sections[0].heading(), "▼ MCP");
    assert_eq!(
        operator_sidebar_mcp_items(&app)
            .into_iter()
            .map(|item| item.display_text())
            .collect::<Vec<_>>(),
        ["websearch Connected".to_string()]
    );
    assert_eq!(
        operator_sidebar_lsp_items(&app)
            .into_iter()
            .map(|item| item.display_text())
            .collect::<Vec<_>>(),
        ["rust".to_string()]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_section_model_uses_runtime_mcp_activity_without_config() {
    let _guard = operator_sidebar_config_test_guard();
    let app = operator_rail_activity_test_app();
    let model = build_operator_rail_model(&app);
    assert_eq!(model.body.sections[0].heading(), "▼ MCP");
    assert_eq!(
        operator_sidebar_mcp_items(&app)
            .into_iter()
            .map(|item| item.display_text())
            .collect::<Vec<_>>(),
        ["websearch Connected".to_string()]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_section_model_keeps_native_prefix_tools_out_of_mcp() {
    let _guard = operator_sidebar_config_test_guard();
    let mut integrations = harness_core::config::IntegrationsConfig::default();
    integrations.remote_search.endpoint = "https://mcp.exa.ai/mcp".to_string();
    harness_core::config::set_registered_integrations_config(integrations);
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let mut app = operator_rail_test_app();

    app.ingest_event(operator_rail_test_event(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_native_prefix_tool".into(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5.4".to_string(),
                prompt_summary: "Apply the sidebar patch".to_string(),
                request_digest: "digest-native-prefix-tool".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        5,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_edit".into(),
                tool_id: "edit.hashline_apply".to_string(),
                args_summary: serde_json::json!({"path": "src/ui_secondary.rs"}).to_string(),
                args_digest: "digest-native-prefix-edit".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        6,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_edit".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Applied inline hashline edit".to_string()),
                output_digest: Some("digest-native-prefix-edit-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));

    let model = build_operator_rail_model(&app);
    assert_eq!(
        model
            .body
            .sections
            .iter()
            .map(OperatorRailBodySection::heading)
            .collect::<Vec<_>>(),
        vec![
            "▼ MCP".to_string(),
            "▼ LSP".to_string(),
            "▼ Modified Files".to_string(),
        ]
    );
    let mcp_items = model.body.sections[0].item_texts();
    assert!(
        mcp_items == ["websearch Disconnected".to_string()]
            || mcp_items == ["No MCP servers configured".to_string()],
        "unexpected MCP summary items: {mcp_items:?}"
    );

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(
        sidebar.contains("websearch Disconnected") || sidebar.contains("No MCP servers configured")
    );
    assert!(!sidebar.contains("edit.hashline_apply"));
    assert!(!sidebar.contains("hashline_apply"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_low_activity_presentation_prefers_primary_stack() {
    exact_test_operator_rail_section_model_keeps_native_prefix_tools_out_of_mcp();
    exact_test_operator_rail_section_model_separates_mcp_from_native_tool_activity();

    let app = operator_rail_modified_file_only_app();
    let rail = build_operator_rail_model(&app);
    let render_text = |text: Text<'static>| {
        text.lines
            .into_iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let wide = render_text(build_operator_rail_body_text(&rail.body, app.theme(), 42));
    let narrow = render_text(build_operator_rail_body_text(&rail.body, app.theme(), 32));

    for rendered in [&wide, &narrow] {
        assert!(
            rendered.contains("▼ MCP"),
            "missing MCP section\n{rendered}"
        );
        assert!(
            rendered.contains("▼ LSP"),
            "missing LSP section\n{rendered}"
        );
        assert!(
            rendered.contains("▼ Modified Files"),
            "missing modified files section\n{rendered}"
        );
        assert!(
            rendered.contains("• demo.txt"),
            "missing modified file row\n{rendered}"
        );
    }
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_section_model_counts_generic_mcp_activity() {
    let _guard = operator_sidebar_config_test_guard();
    let mut integrations = harness_core::config::IntegrationsConfig::default();
    integrations.remote_search.endpoint = "https://mcp.exa.ai/mcp".to_string();
    integrations.mcp.servers.insert(
        "fixture".to_string(),
        harness_core::config::McpServerConfig::Http {
            endpoint: "https://example.com/mcp".to_string(),
            headers: std::collections::BTreeMap::new(),
            timeout_secs: 30,
            enabled: true,
        },
    );
    harness_core::config::set_registered_integrations_config(integrations);
    harness_core::config::set_registered_mcp_server_connection_states(
        std::collections::BTreeMap::from([(
            "fixture".to_string(),
            harness_core::config::McpServerConnectionState::Connected,
        )]),
    );
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let mut app = operator_rail_test_app();

    let worker = harness_core::event::EventActor::new(
        harness_core::event::ActorKind::Worker,
        Some("w1".to_string()),
    );
    app.ingest_event(operator_rail_test_event(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-mcp".into(),
                text: "Use the MCP fixture".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        5,
        worker.clone(),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_mcp_1".into(),
                tool_id: "mcp.fixture.tool.call".to_string(),
                args_summary: r#"{"tool":"echo"}"#.to_string(),
                args_digest: "digest-mcp-1".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("mcp.fixture.echo".to_string()),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        6,
        worker.clone(),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_mcp_1".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("fixture ok".to_string()),
                output_digest: Some("digest-output-mcp-1".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("mcp.fixture.echo".to_string()),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        7,
        worker.clone(),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_search_1".into(),
                tool_id: "search.web".to_string(),
                args_summary: r#"{"query":"docs"}"#.to_string(),
                args_digest: "digest-search-1".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        8,
        worker,
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_search_1".into(),
                status: harness_core::event::ToolCallStatus::Failed,
                output_summary: Some("search failed".to_string()),
                output_digest: Some("digest-output-search-1".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));

    let model = build_operator_rail_model(&app);

    assert_eq!(model.body.sections[0].heading(), "▼ MCP");
    assert_eq!(
        operator_sidebar_mcp_items(&app)
            .into_iter()
            .map(|item| item.display_text())
            .collect::<Vec<_>>(),
        [
            "fixture Connected".to_string(),
            "websearch Disconnected".to_string(),
        ]
    );

    app.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        Rect::new(0, 0, 140, 40),
        None,
        Some(OperatorSidebarSection::Mcp),
        None,
    );

    let collapsed_sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(collapsed_sidebar.contains("▶ MCP (1 active, 1 error)"));
    assert!(!collapsed_sidebar.contains("• fixture Connected"));
    assert!(!collapsed_sidebar.contains("• websearch Disconnected"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_collapses_modified_files_section_body() {
    let mut app = operator_rail_test_app();
    app.ingest_event(operator_rail_test_event(
        50,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_more_1".to_string(),
            path: "src/ui.rs".to_string(),
            new_file_digest: "digest-edit-more-1".to_string(),
            diff_rel_path: None,
            diff_digest: None,
        }),
    ));
    app.ingest_event(operator_rail_test_event(
        51,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_more_2".to_string(),
            path: "src/app.rs".to_string(),
            new_file_digest: "digest-edit-more-2".to_string(),
            diff_rel_path: None,
            diff_digest: None,
        }),
    ));

    app.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        Rect::new(0, 0, 140, 40),
        None,
        Some(OperatorSidebarSection::ModifiedFiles),
        None,
    );

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("▶ Modified Files"));
    assert!(!sidebar.contains("src/ui_secondary.rs"));
    assert!(!sidebar.contains("src/ui.rs"));
    assert!(!sidebar.contains("src/app.rs"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_sidebar_hit_target_maps_section_headers() {
    let mut app = operator_rail_test_app();
    app.live_details_drawer_open = true;
    app.ingest_event(operator_rail_test_event(
        50,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_more_1".to_string(),
            path: "src/ui.rs".to_string(),
            new_file_digest: "digest-edit-more-1".to_string(),
            diff_rel_path: None,
            diff_digest: None,
        }),
    ));

    let frame_area = Rect::new(0, 0, 160, 30);
    let sidebar_area = crate::layout::FrameLayoutPlan::for_app(&app, frame_area)
        .details_overlay
        .unwrap_or_abort();
    let inner = operator_sidebar_inner_area(&app, sidebar_area, app.theme()).unwrap_or_abort();
    let column = inner.x.saturating_add(1);
    let hits = (inner.y..inner.y.saturating_add(inner.height))
        .filter_map(|row| operator_sidebar_section_hit_target(&app, frame_area, column, row))
        .collect::<std::collections::BTreeSet<_>>();

    assert!(hits.contains(&OperatorSidebarSection::Mcp));
    assert!(hits.contains(&OperatorSidebarSection::Lsp));
    assert!(hits.contains(&OperatorSidebarSection::ModifiedFiles));
}

#[cfg(test)]
fn build_operator_rail_body_text(
    body: &OperatorRailBody,
    theme: &Theme,
    width: u16,
) -> Text<'static> {
    Text::from(build_operator_rail_body_layout(body, theme, width, 0).lines)
}
