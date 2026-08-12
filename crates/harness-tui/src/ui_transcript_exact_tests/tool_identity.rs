use super::super::*;
use crate::UnwrapOrAbort;

#[cfg(test)]
pub(crate) fn exact_test_native_tool_transcript_rows_show_reference_timestamps_and_task_metadata() {
    let theme = Theme::default();

    let mut native_read = transcript_section_model_test_tool_call("tc-native-read", "fs.read");
    native_read.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("fs.read".to_string()),
        effective_tool_id: Some("fs.read".to_string()),
        canonical_tool_id: Some("fs.read".to_string()),
        alias_source_tool_id: None,
    });
    native_read.args_summary = r#"{"path":"src/ui.rs","offset":12,"limit":24}"#.to_string();
    native_read.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    native_read.status = ToolCallDisplayStatus::Succeeded;
    native_read.output_summary = Some("24 lines read from src/ui.rs".to_string());
    native_read.truncated_output = native_read.output_summary.clone();
    native_read.last_mono_ms = 1_250;
    native_read.last_timestamp = Some("2026-03-22T14:35:44Z".to_string());

    let mut alias_read = transcript_section_model_test_tool_call("tc-alias-read", "read");
    alias_read.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("read".to_string()),
        effective_tool_id: Some("fs.read".to_string()),
        canonical_tool_id: Some("fs.read".to_string()),
        alias_source_tool_id: Some("read".to_string()),
    });
    alias_read.args_summary = native_read.args_summary.clone();
    alias_read.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    alias_read.status = ToolCallDisplayStatus::Succeeded;
    alias_read.output_summary = native_read.output_summary.clone();
    alias_read.truncated_output = native_read.truncated_output.clone();
    alias_read.last_mono_ms = native_read.last_mono_ms;
    alias_read.last_timestamp = native_read.last_timestamp.clone();

    let native_read_section = build_transcript_tool_call_section(
        &native_read,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    let alias_read_section = build_transcript_tool_call_section(
        &alias_read,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(
        native_read_section.header.title,
        alias_read_section.header.title
    );
    assert_eq!(
        native_read_section.header.icon,
        alias_read_section.header.icon
    );
    assert_eq!(
        native_read_section.header.visual_style,
        alias_read_section.header.visual_style
    );
    assert_eq!(native_read_section.header.subtitle, None);
    assert_eq!(alias_read_section.header.subtitle, None);
    assert_eq!(alias_read_section.header.disclosure_state, None);
    assert!(alias_read_section
        .detail_blocks
        .iter()
        .all(|block| !matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, .. }
                if text.contains("Compat alias ·")
        )));

    let mut task_call = transcript_section_model_test_tool_call("tc-task", "task");
    task_call.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("task".to_string()),
        effective_tool_id: Some("task".to_string()),
        canonical_tool_id: Some("task".to_string()),
        alias_source_tool_id: None,
    });
    task_call.args_summary =
        r#"{"description":"audit transcript parity","subagent_type":"researcher"}"#.to_string();
    task_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    task_call.status = ToolCallDisplayStatus::Succeeded;
    task_call.output_json = Some(serde_json::json!({
        "description": "audit transcript parity",
        "profile": "researcher",
        "mode": "foreground",
        "status": "completed",
        "duration_ms": 1600,
        "result_summary": "Found the inline transcript path.",
        "child_tool_call_count": 3,
        "child_session_id": "agent_worker",
        "child_request_id": "req_child",
    }));
    task_call.lineage = Some(crate::app::TaskLineageEntry {
        parent_tool_call_id: Some("tc-task".to_string()),
        parent_task_id: None,
        parent_request_id: Some("req_parent".to_string()),
        child_session_id: Some("agent_worker".to_string()),
        child_request_id: Some("req_child".to_string()),
    });
    task_call.timing_elapsed_ms = Some(1600);
    task_call.last_mono_ms = 1_600;
    task_call.last_timestamp = Some("2026-03-22T14:36:01Z".to_string());

    let task_section = build_transcript_tool_call_section(
        &task_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(
        task_section.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );
    assert_eq!(
        task_section.child_session_id.as_deref(),
        Some("agent_worker")
    );
    assert_eq!(task_section.header.icon, Some("✓"));
    assert_eq!(
        task_section.header.visual_style,
        TranscriptToolCallVisualStyle::TaskInline
    );
    let task_render =
        append_tool_call_section_lines(&task_section, &theme, 120, theme.surface.panel);
    assert!(task_render.interaction_rows.iter().any(|interaction| matches!(
        interaction.as_ref().map(|row| &row.target),
        Some(TranscriptMouseTarget::SubagentSession { session_id }) if session_id == "agent_worker"
    )));
    assert!(
        task_render.interaction_rows.iter().all(|interaction| matches!(
            interaction.as_ref().map(|row| &row.target),
            Some(TranscriptMouseTarget::SubagentSession { session_id }) if session_id == "agent_worker"
        )),
        "task inline rows should navigate to the child session, matching Harness's clickable task card"
    );
    assert!(
        task_render
            .interaction_rows
            .iter()
            .flatten()
            .all(|row| row.hit_width < 120),
        "task inline hitboxes should stop at the rendered card text instead of spanning the transcript row"
    );
    let task_hit_width = task_render.interaction_rows[0]
        .as_ref()
        .unwrap_or_abort()
        .hit_width;
    let task_hit_start = task_render.interaction_rows[0]
        .as_ref()
        .unwrap_or_abort()
        .hit_start;
    let task_hit_layout = MeasuredTranscriptLayout {
        sections: vec![std::rc::Rc::new(MeasuredTranscriptSection {
            activity_first_seq: 0,
            top_row: 0,
            leading_gap_height: 0,
            content_height: task_render.lines.len(),
            surfaces: vec![MeasuredTranscriptSurface {
                kind: TranscriptRenderSurfaceKind::AssistantTool,
                top_offset: 0,
                height: task_render.lines.len(),
                width: 120,
                show_outer_rail: false,
                rail_glyph: TRANSCRIPT_RAIL_GLYPH,
                rail_color: theme.border.subtle,
                surface: theme.surface.panel,
                lines: task_render.lines.clone(),
                interaction_rows: Some(task_render.interaction_rows.clone()),
                selection_rows: None,
                diff_hunk_offsets: Vec::new(),
                selected_rail: false,
                tool_rail_motion: None,
            }],
            lines: task_render.lines.clone(),
        })],
        total_height: task_render.lines.len(),
    };
    assert!(matches!(
        transcript_mouse_target_at(
            &task_hit_layout,
            Rect::new(0, 0, 120, 10),
            0,
            task_hit_start,
            0,
        ),
        Some(TranscriptMouseTarget::SubagentSession { session_id }) if session_id == "agent_worker"
    ));
    assert!(matches!(
        transcript_mouse_target_at(
            &task_hit_layout,
            Rect::new(0, 0, 120, 10),
            0,
            task_hit_start
                .saturating_add(task_hit_width)
                .saturating_sub(1),
            0,
        ),
        Some(TranscriptMouseTarget::SubagentSession { session_id }) if session_id == "agent_worker"
    ));
    assert_eq!(
        transcript_mouse_target_at(
            &task_hit_layout,
            Rect::new(0, 0, 120, 10),
            0,
            task_hit_start.saturating_sub(1),
            0,
        ),
        None
    );
    assert_eq!(
        transcript_mouse_target_at(
            &task_hit_layout,
            Rect::new(0, 0, 120, 10),
            0,
            task_hit_start.saturating_add(task_hit_width),
            0,
        ),
        None
    );
    let task_lines = task_render.lines;
    let task_text = transcript_test_line_texts(task_lines).join("\n");
    assert!(task_text.contains("Researcher Task — audit transcript parity"));
    assert!(!task_text.contains("audit transcript parity · Researcher Agent"));
    assert!(!task_text.contains("3 toolcalls · 1.6s"));
    assert!(!task_text.contains("Found the inline transcript path."));
    assert!(!task_text.contains("Compat alias ·"));
    assert!(!task_text.contains("Task audit transcript parity"));

    let expanded_task_section = build_transcript_tool_call_section(
        &task_call,
        &AppState::default(),
        None,
        true,
        false,
        true,
        false,
        None,
    );
    let expanded_task_render =
        append_tool_call_section_lines(&expanded_task_section, &theme, 120, theme.surface.panel);
    let expanded_task_text = transcript_test_line_texts(expanded_task_render.lines).join("\n");
    assert!(!expanded_task_text.contains("Found the inline transcript path."));

    let mut fetch_call = transcript_section_model_test_tool_call("tc-fetch", "webfetch");
    fetch_call.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("webfetch".to_string()),
        effective_tool_id: Some("web.fetch".to_string()),
        canonical_tool_id: Some("web.fetch".to_string()),
        alias_source_tool_id: Some("webfetch".to_string()),
    });
    fetch_call.args_summary =
        r#"{"url":"https://example.test/report.pdf","format":"markdown"}"#.to_string();
    fetch_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    fetch_call.status = ToolCallDisplayStatus::Succeeded;
    fetch_call.output_summary =
        Some("report ready\npage count: 2\nformat: pdf\nstored inline artifact".to_string());
    fetch_call.truncated_output = fetch_call.output_summary.clone();
    fetch_call.artifact_refs = vec![crate::app::ToolArtifactEntry {
        path: "artifacts/toolcalls/tc-fetch/web.fetch.pdf".to_string(),
        digest: Some("digest-fetch-artifact".to_string()),
    }];
    fetch_call.timing_elapsed_ms = Some(2400);
    fetch_call.last_mono_ms = 2_400;
    fetch_call.last_timestamp = Some("2026-03-22T14:37:12Z".to_string());

    let fetch_section = build_transcript_tool_call_section(
        &fetch_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(
        fetch_section.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );
    let mut fetch_lines = Vec::new();
    {
        let render =
            append_tool_call_section_lines(&fetch_section, &theme, 120, theme.surface.panel);
        fetch_lines.extend(render.lines);
    }
    let fetch_text = transcript_test_line_texts(fetch_lines).join("\n");
    assert!(
        fetch_text.contains("WebFetch https://example.test/report.pdf")
            || fetch_text.contains("◆ WebFetch")
    );
    assert!(!fetch_text.contains("report ready"));
    assert!(!fetch_text.contains("stored inline artifact"));
    assert!(!fetch_text.contains("Click to expand"));
    assert!(!fetch_text.contains("Attachment ·"));
    assert!(!fetch_text.contains("web.fetch.pdf"));
    assert!(!fetch_text.contains("Compat alias ·"));

    let mut web_search_call =
        transcript_section_model_test_tool_call("tc-web-search", "search.web");
    web_search_call.args_summary = r#"{"query":"rust tui parity"}"#.to_string();
    web_search_call.status = ToolCallDisplayStatus::Succeeded;
    web_search_call.output_json = Some(serde_json::json!({
        "provider": "parallel",
        "numResults": 4,
    }));
    let web_search_section = build_transcript_tool_call_section(
        &web_search_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(web_search_section.header.icon, Some("◈"));
    let web_search_render =
        append_tool_call_section_lines(&web_search_section, &theme, 120, theme.surface.panel);
    let web_search_text = transcript_test_line_texts(web_search_render.lines).join("\n");
    assert!(
        web_search_text.contains("◆ Parallel Web Search \"rust tui parity\"")
            || web_search_text.contains("Parallel Web Search \"rust tui parity\"")
    );
    assert!(!web_search_text.contains("Exa Web Search \"rust tui parity\""));

    let mut code_search_call =
        transcript_section_model_test_tool_call("tc-code-search", "search.code");
    code_search_call.args_summary = r#"{"query":"append_reasoning_block"}"#.to_string();
    code_search_call.status = ToolCallDisplayStatus::Succeeded;
    code_search_call.output_json = Some(serde_json::json!({
        "results": 2,
    }));
    let code_search_section = build_transcript_tool_call_section(
        &code_search_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(code_search_section.header.icon, Some("◇"));
    let code_search_render =
        append_tool_call_section_lines(&code_search_section, &theme, 120, theme.surface.panel);
    let code_search_text = transcript_test_line_texts(code_search_render.lines).join("\n");
    assert!(
        code_search_text.contains("Exa Code Search \"append_reasoning_block\"")
            || code_search_text.contains("◆ Exa Code Search")
    );

    let mut generic_call = transcript_section_model_test_tool_call("tc-generic", "vendor.magic");
    generic_call.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("vendor.magic".to_string()),
        effective_tool_id: Some("vendor.magic".to_string()),
        canonical_tool_id: None,
        alias_source_tool_id: None,
    });
    generic_call.args_summary =
        r#"{"path":"notes.md","query":"child parity","limit":3}"#.to_string();
    generic_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    generic_call.status = ToolCallDisplayStatus::Succeeded;
    generic_call.timing_elapsed_ms = Some(800);
    let generic_section = build_transcript_tool_call_section(
        &generic_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(
        generic_section.header.title,
        "vendor.magic child parity [limit=3]"
    );
    assert_eq!(generic_section.header.subtitle, None);
}

#[cfg(test)]
pub(crate) fn exact_test_mcp_tool_transcript_rows_use_effective_identity_without_generic_fallback()
{
    let theme = Theme::default();

    let mut direct_call =
        transcript_section_model_test_tool_call("tc-mcp-direct", "mcp.fixture.echo");
    direct_call.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("mcp.fixture.echo".to_string()),
        effective_tool_id: Some("mcp.fixture.echo".to_string()),
        canonical_tool_id: None,
        alias_source_tool_id: None,
    });
    direct_call.args_summary = r#"{"text":"hello from direct"}"#.to_string();
    direct_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    direct_call.status = ToolCallDisplayStatus::Succeeded;
    direct_call.output_summary = Some("direct output summary".to_string());
    direct_call.output_json = Some(serde_json::json!({
        "server": {
            "id": "fixture",
            "transport": "stdio",
        },
        "protocolVersion": "2025-06-18",
        "serverInfo": {
            "name": "fixture",
            "version": "1.0.0",
        },
        "payload": {
            "tool": "echo",
            "arguments": { "text": "hello from direct" },
            "result": {
                "content": [{ "type": "text", "text": "direct output summary" }],
                "isError": false,
            },
        },
    }));
    direct_call.timing_elapsed_ms = Some(900);

    let mut wrapper_call =
        transcript_section_model_test_tool_call("tc-mcp-wrapper", "mcp.fixture.tool.call");
    wrapper_call.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("mcp.fixture.tool.call".to_string()),
        effective_tool_id: Some("mcp.fixture.echo".to_string()),
        canonical_tool_id: None,
        alias_source_tool_id: None,
    });
    wrapper_call.args_summary =
        r#"{"tool":"echo","arguments":{"text":"hello from wrapper"}}"#.to_string();
    wrapper_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    wrapper_call.status = ToolCallDisplayStatus::Succeeded;
    wrapper_call.output_summary = Some("wrapper output summary".to_string());
    wrapper_call.output_json = Some(serde_json::json!({
        "server": {
            "id": "fixture",
            "transport": "stdio",
        },
        "protocolVersion": "2025-06-18",
        "serverInfo": {
            "name": "fixture",
            "version": "1.0.0",
        },
        "payload": {
            "tool": "echo",
            "arguments": { "text": "hello from wrapper" },
            "result": {
                "content": [{ "type": "text", "text": "wrapper output summary" }],
                "isError": false,
            },
        },
    }));
    wrapper_call.timing_elapsed_ms = Some(900);

    let direct_section = build_transcript_tool_call_section(
        &direct_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    let wrapper_section = build_transcript_tool_call_section(
        &wrapper_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );

    assert_eq!(
        direct_section.header.title,
        "fixture_echo [text=hello from direct]"
    );
    assert_eq!(
        wrapper_section.header.title,
        "fixture_echo [text=hello from wrapper]"
    );
    assert_eq!(direct_section.header.icon, Some("⚙"));
    assert_eq!(wrapper_section.header.icon, Some("⚙"));
    assert_eq!(
        direct_section.header.visual_style,
        TranscriptToolCallVisualStyle::Inline
    );
    assert_eq!(
        wrapper_section.header.visual_style,
        direct_section.header.visual_style
    );
    assert_eq!(
        direct_section.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );
    assert_eq!(
        wrapper_section.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );

    let mut direct_lines = Vec::new();
    {
        let render =
            append_tool_call_section_lines(&direct_section, &theme, 120, theme.surface.panel);
        direct_lines.extend(render.lines);
    }
    let mut wrapper_lines = Vec::new();
    {
        let render =
            append_tool_call_section_lines(&wrapper_section, &theme, 120, theme.surface.panel);
        wrapper_lines.extend(render.lines);
    }

    let direct_text = transcript_test_line_texts(direct_lines).join("\n");
    let wrapper_text = transcript_test_line_texts(wrapper_lines).join("\n");
    assert!(direct_text.contains("fixture_echo"));
    assert!(direct_text.contains("[text=hello from direct]"));
    assert!(wrapper_text.contains("fixture_echo"));
    assert!(wrapper_text.contains("[text=hello from wrapper]"));
    assert!(!direct_text.contains("direct output summary"));
    assert!(!wrapper_text.contains("wrapper output summary"));
    assert!(!wrapper_text.contains("mcp.fixture.tool.call"));
    assert!(!wrapper_text.contains("Compat alias"));

    let expanded_wrapper = build_transcript_tool_call_section(
        &wrapper_call,
        &AppState::default(),
        None,
        true,
        false,
        true,
        false,
        None,
    );
    let expanded_text = transcript_test_line_texts({
        let mut lines = Vec::new();
        {
            let render =
                append_tool_call_section_lines(&expanded_wrapper, &theme, 120, theme.surface.panel);
            lines.extend(render.lines);
        }
        lines
    })
    .join("\n");
    assert!(expanded_text.contains("wrapper output summary"));
}

#[cfg(test)]
pub(crate) fn exact_test_generic_tool_successful_output_prefers_inline_background_rows() {
    let theme = Theme::default();
    let mut tool_call = transcript_section_model_test_tool_call("tc-vendor-magic", "vendor.magic");
    tool_call.args_summary = r#"{"path":"notes.md","query":"child parity","limit":3}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    tool_call.output_summary = Some("line 1\nline 2\nline 3\nline 4".to_string());
    tool_call.timing_elapsed_ms = Some(250);

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Inline
    );
    assert_eq!(
        section.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );

    let rendered = transcript_test_line_texts({
        let mut lines = Vec::new();
        {
            let render = append_tool_call_section_lines(&section, &theme, 96, theme.surface.panel);
            lines.extend(render.lines);
        }
        lines
    });

    assert!(rendered[0].contains("vendor.magic"));
    assert!(rendered[0].contains("child parity"));
    assert!(rendered[0].contains("[limit=3]"));
    assert!(rendered.iter().all(|line| !line.contains("line 1")));
    assert!(rendered
        .iter()
        .all(|line| !line.trim_start().starts_with('┃')));

    let visible = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        true,
        true,
        false,
        false,
        None,
    );
    assert_eq!(
        visible.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );
    assert_eq!(
        visible.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );

    let visible_rendered = transcript_test_line_texts({
        let mut lines = Vec::new();
        {
            let render = append_tool_call_section_lines(&visible, &theme, 96, theme.surface.panel);
            lines.extend(render.lines);
        }
        lines
    });
    let visible_text = visible_rendered.join("\n");
    assert!(visible_text.contains("line 1"));
    assert!(visible_text.contains("line 3"));
    assert!(!visible_text.contains("line 4"));
    assert!(visible_text.contains("Click to expand"));
    assert!(visible_text.contains('…'));

    let expanded = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        true,
        false,
        true,
        false,
        None,
    );
    let expanded_text = transcript_test_line_texts({
        let mut lines = Vec::new();
        {
            let render = append_tool_call_section_lines(&expanded, &theme, 96, theme.surface.panel);
            lines.extend(render.lines);
        }
        lines
    })
    .join("\n");
    assert!(expanded_text.contains("line 4"));
}

#[cfg(test)]
pub(crate) fn exact_test_lsp_tool_successful_output_stays_hidden_until_generic_output_enabled() {
    let theme = Theme::default();
    let mut tool_call = transcript_section_model_test_tool_call("tc-lsp", "code.lsp");
    tool_call.args_summary =
        r#"{"operation":"goto_definition","filePath":"src/main.rs","line":12,"character":4}"#
            .to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    tool_call.output_summary = Some("result 1\nresult 2\nresult 3\nresult 4".to_string());

    let hidden = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(
        hidden.header.visual_style,
        TranscriptToolCallVisualStyle::Inline
    );
    assert_eq!(
        hidden.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );

    let hidden_text = transcript_test_line_texts({
        let mut lines = Vec::new();
        {
            let render = append_tool_call_section_lines(&hidden, &theme, 96, theme.surface.panel);
            lines.extend(render.lines);
        }
        lines
    })
    .join("\n");
    assert!(
        hidden_text.contains("◆ LSP goto_definition")
            || hidden_text.contains("LSP goto_definition")
    );
    assert!(!hidden_text.contains("⌘"));
    assert!(!hidden_text.contains("[operation=goto_definition]"));
    assert!(!hidden_text.contains("result 1"));

    let visible = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        true,
        true,
        false,
        false,
        None,
    );
    assert_eq!(
        visible.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );

    let visible_text = transcript_test_line_texts({
        let mut lines = Vec::new();
        {
            let render = append_tool_call_section_lines(&visible, &theme, 96, theme.surface.panel);
            lines.extend(render.lines);
        }
        lines
    })
    .join("\n");
    assert!(visible_text.contains("result 1"));
    assert!(visible_text.contains("Click to expand"));
    assert!(visible_text.contains('…'));
    assert!(!visible_text.contains("result 4"));
}

#[cfg(test)]
pub(crate) fn exact_test_skill_tool_rows_match_reference_title_and_icon() {
    let theme = Theme::default();
    let mut tool_call = transcript_section_model_test_tool_call("tc-skill", "skill");
    tool_call.args_summary = r#"{"name":"rust-best-practices"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    tool_call.output_summary = Some("skill loaded".to_string());

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(section.header.icon, Some("→"));
    assert_eq!(section.header.title, "Skill \"rust-best-practices\"");
    assert_eq!(section.header.subtitle, None);
    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Inline
    );
    assert_eq!(section.header.disclosure_state, None);

    let rendered = transcript_test_line_texts({
        let render = append_tool_call_section_lines(&section, &theme, 96, theme.surface.panel);
        render.lines
    })
    .join("\n");
    assert!(
        rendered.contains("◆ Skill \"rust-best-practices\"")
            || rendered.contains("Skill \"rust-best-practices\"")
    );
    assert!(!rendered.contains("Load skill"));
    assert!(!rendered.contains('✦'));
    assert!(!rendered.contains("skill loaded"));
}

#[cfg(test)]
pub(crate) fn exact_test_file_search_rows_match_reference_title_description_shape() {
    let theme = Theme::default();

    let mut glob_call = transcript_section_model_test_tool_call("tc-glob", "fs.glob");
    glob_call.args_summary = r#"{"pattern":"**/*.rs","path":"crates/harness-tui"}"#.to_string();
    glob_call.status = ToolCallDisplayStatus::Succeeded;
    glob_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    glob_call.output_json = Some(serde_json::json!({ "count": 3 }));

    let mut grep_call = transcript_section_model_test_tool_call("tc-grep", "fs.grep");
    grep_call.args_summary =
        r#"{"pattern":"HARNESS_SPLIT_RAIL_GLYPH","path":"crates/harness-tui/src"}"#.to_string();
    grep_call.status = ToolCallDisplayStatus::Succeeded;
    grep_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    grep_call.output_json = Some(serde_json::json!({ "total_count": 2 }));

    let mut list_call = transcript_section_model_test_tool_call("tc-list", "list");
    list_call.args_summary = r#"{"path":"crates/harness-tui/src"}"#.to_string();
    list_call.status = ToolCallDisplayStatus::Succeeded;
    list_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);

    let glob_section = build_transcript_tool_call_section(
        &glob_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    let grep_section = build_transcript_tool_call_section(
        &grep_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    let list_section = build_transcript_tool_call_section(
        &list_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );

    assert_eq!(glob_section.header.icon, Some("✱"));
    assert_eq!(glob_section.header.title, "Glob \"**/*.rs\"");
    assert_eq!(
        glob_section.header.subtitle.as_deref(),
        Some("in crates/harness-tui · 3 matches")
    );
    assert_eq!(grep_section.header.icon, Some("✱"));
    assert_eq!(
        grep_section.header.title,
        "Grep \"HARNESS_SPLIT_RAIL_GLYPH\""
    );
    assert_eq!(
        grep_section.header.subtitle.as_deref(),
        Some("in crates/harness-tui/src · 2 matches")
    );
    assert_eq!(list_section.header.icon, Some("→"));
    assert_eq!(list_section.header.title, "Listed 1 dir");
    assert_eq!(list_section.header.subtitle, None);

    let rendered = transcript_test_line_texts({
        let mut lines = Vec::new();
        lines.extend(
            append_tool_call_section_lines(&glob_section, &theme, 96, theme.surface.panel).lines,
        );
        lines.extend(
            append_tool_call_section_lines(&grep_section, &theme, 96, theme.surface.panel).lines,
        );
        lines.extend(
            append_tool_call_section_lines(&list_section, &theme, 96, theme.surface.panel).lines,
        );
        lines
    })
    .join("\n");
    assert!(
        rendered.contains("◈ Glob \"**/*.rs\"")
            || rendered.contains("◆ Glob \"**/*.rs\"")
            || rendered.contains("Glob \"**/*.rs\"")
    );
    assert!(rendered.contains("3 matches") || rendered.contains("crates/harness-tui"));
    assert!(rendered.contains("Grep \"HARNESS_SPLIT_RAIL_GLYPH\""));
    assert!(
        rendered.contains("◈ Listed 1 dir")
            || rendered.contains("Listed 1 dir")
            || rendered.contains("List crates/harness-tui/src")
    );
    assert!(!rendered.contains("Glob \"**/*.rs\" in crates/harness-tui"));
    assert!(!rendered.contains("(3 matches)"));
    assert!(!rendered.contains("(2 matches)"));
}

#[cfg(test)]
pub(crate) fn exact_test_todo_write_rows_render_open_checklist() {
    let app = AppState::new_live(None, false, None);
    let theme = Theme::default();

    let mut write_call = transcript_section_model_test_tool_call("tc-todo-write", "todo.write");
    write_call.status = ToolCallDisplayStatus::Succeeded;
    write_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    write_call.output_summary = Some("todo list updated".to_string());
    write_call.output_json = Some(serde_json::json!({
        "todos": [
            {"content": "Plan work", "status": "completed", "priority": "high"},
            {"content": "Implement UI", "status": "in_progress", "priority": "high"},
            {"content": "Verify tests", "status": "pending", "priority": "medium"}
        ]
    }));

    let mut read_call = transcript_section_model_test_tool_call("tc-todo-read", "todo.read");
    read_call.status = ToolCallDisplayStatus::Succeeded;
    read_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    read_call.output_summary = Some("[]".to_string());

    let mut compat_read_call = transcript_section_model_test_tool_call("tc-todoread", "todoread");
    compat_read_call.status = ToolCallDisplayStatus::Succeeded;
    compat_read_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    compat_read_call.output_summary = Some("[]".to_string());

    let write_section =
        build_tool_call_section(&write_call, &app, false, false, false, false, false, None)
            .unwrap_or_abort();
    assert_eq!(
        write_section.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );
    assert_eq!(
        (
            write_section.header.icon,
            write_section.header.title.as_str()
        ),
        (None, "# Todos")
    );
    assert_eq!(
        write_section.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );

    let expanded_write_section =
        build_tool_call_section(&write_call, &app, false, false, false, true, false, None)
            .unwrap_or_abort();
    let todo_lines = {
        let render = append_tool_call_section_lines(
            &expanded_write_section,
            &theme,
            96,
            theme.surface.panel,
        );
        render.lines
    };
    let rendered = transcript_test_line_texts(todo_lines.clone()).join("\n");
    let active_marker = format!("[{}] Implement UI", theme.live_shell.glyphs.streaming);
    let completed_marker = format!(
        "[{}] Plan work",
        theme.live_shell.transcript_glyphs.choice_checked
    );
    assert!(rendered.contains("Todos"));
    assert!(rendered.contains(&active_marker));
    assert!(rendered.contains(&completed_marker));
    assert!(rendered.contains("[ ] Verify tests"));

    assert!(
        !rendered.contains('┃'),
        "todo block must not paint legacy nested ┃ rails\n{rendered}"
    );
    let title_line = rendered
        .lines()
        .find(|line| line.contains("# Todos"))
        .expect("title line should be present");
    assert!(
        title_line.contains("◆ # Todos") || title_line.contains("# Todos"),
        "title should include todos header after tool marker: {title_line}"
    );
    let active_line = rendered
        .lines()
        .find(|line| line.contains(&active_marker))
        .expect("active todo line should be present");
    assert!(
        active_line.contains(&active_marker),
        "todo items should remain structured: {active_line}"
    );
    let completed = rendered.find(&completed_marker).unwrap_or_abort();
    let active = rendered.find(&active_marker).unwrap_or_abort();
    assert!(
        completed < active,
        "todo rows should preserve tool-provided order\n{rendered}"
    );

    let mut cancelled_then_pending =
        transcript_section_model_test_tool_call("tc-todo-cancelled-pending", "todowrite");
    cancelled_then_pending.status = ToolCallDisplayStatus::Succeeded;
    cancelled_then_pending.lifecycle_state =
        Some(harness_core::event::ToolCallLifecycleState::Completed);
    cancelled_then_pending.output_json = Some(serde_json::json!([
        {"content": "Skip stale path", "status": "cancelled", "priority": "low"},
        {"content": "Pick next path", "status": "pending", "priority": "high"}
    ]));
    let cancelled_then_pending_section = build_tool_call_section(
        &cancelled_then_pending,
        &app,
        false,
        false,
        false,
        true,
        false,
        None,
    )
    .unwrap_or_abort();
    let cancelled_then_pending_lines = {
        let render = append_tool_call_section_lines(
            &cancelled_then_pending_section,
            &theme,
            96,
            theme.surface.panel,
        );
        render.lines
    };
    let cancelled_then_pending_rendered =
        transcript_test_line_texts(cancelled_then_pending_lines.clone()).join("\n");
    let cancelled = cancelled_then_pending_rendered
        .find("[ ] Skip stale path")
        .unwrap_or_abort();
    let pending = cancelled_then_pending_rendered
        .find("[ ] Pick next path")
        .unwrap_or_abort();
    assert!(
        cancelled < pending,
        "todo rows should preserve cancelled/pending order\n{cancelled_then_pending_rendered}"
    );
    let cancelled_line = cancelled_then_pending_lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("Skip stale path")
        })
        .unwrap_or_abort();
    assert!(
        !cancelled_line
            .spans
            .iter()
            .any(|span| (span.content == "[" || span.content == "]")
                && span
                    .style
                    .add_modifier
                    .contains(ratatui::style::Modifier::CROSSED_OUT)),
        "cancelled todo marker should not be crossed out"
    );
    assert!(
        !cancelled_line
            .spans
            .iter()
            .any(|span| span.content.contains("Skip")
                && span
                    .style
                    .add_modifier
                    .contains(ratatui::style::Modifier::CROSSED_OUT)),
        "cancelled todo content should not be crossed out"
    );

    if std::env::var_os("HARNESS_TUI_TODO_RENDER_CAPTURE").is_some() {
        println!("# Todo render\n{rendered}\n\n# Cancelled todo render\n{cancelled_then_pending_rendered}");
    }

    let mut structured_output =
        transcript_section_model_test_tool_call("tc-todo-structured-output", "todo.write");
    structured_output.status = ToolCallDisplayStatus::Succeeded;
    structured_output.lifecycle_state =
        Some(harness_core::event::ToolCallLifecycleState::Completed);
    structured_output.output_json = Some(serde_json::json!({
        "structured_output": {
            "todos": [
                {"content": "Render nested todos", "status": "pending", "priority": "medium"}
            ]
        }
    }));
    let structured_output_section = build_tool_call_section(
        &structured_output,
        &app,
        false,
        false,
        false,
        false,
        false,
        None,
    )
    .unwrap_or_abort();
    let structured_output_rendered = transcript_test_line_texts({
        let render = append_tool_call_section_lines(
            &structured_output_section,
            &theme,
            96,
            theme.surface.panel,
        );
        render.lines
    })
    .join("\n");
    assert!(structured_output_rendered.contains("[ ] Render nested todos"));

    assert!(
        build_tool_call_section(&read_call, &app, true, false, false, false, false, None).is_none(),
        "todo reads should stay hidden because they do not update visible state"
    );
    assert!(build_tool_call_section(
        &compat_read_call,
        &app,
        true,
        false,
        false,
        false,
        false,
        None,
    )
    .is_none());
}

#[cfg(test)]
pub(crate) fn exact_test_todo_write_running_renders_inline_updating_indicator() {
    let app = AppState::new_live(None, false, None);

    let mut running_call = transcript_section_model_test_tool_call("tc-todo-running", "todo.write");
    running_call.status = ToolCallDisplayStatus::Running;
    running_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Running);
    running_call.output_summary = Some("updating".to_string());

    let section =
        build_tool_call_section(&running_call, &app, false, false, false, false, false, None)
            .unwrap_or_abort();
    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Inline
    );
    assert_eq!(
        (section.header.icon, section.header.title.as_str()),
        (Some("⚙"), "Updating todos...")
    );
    assert!(section.detail_blocks.is_empty());
}
