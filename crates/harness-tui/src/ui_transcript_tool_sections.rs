// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use super::ui_tool_delegation::agent_spawn_is_background;
use super::ui_tool_paths::tool_id_matches;
use super::ui_tool_visibility::{
    tool_call_has_transcript_disclosure, tool_error_hidden_inline, tool_output_is_viewer_only,
};
use super::ui_transcript_block_grammar::{tool_family, TranscriptToolFamily};
use super::*;

const HARNESS_GENERIC_OUTPUT_LINE_CLAMP: usize = 3;

#[expect(
    clippy::too_many_arguments,
    reason = "transcript tool-row assembly keeps the rendering toggles explicit at the call site"
)]
pub(super) fn build_tool_call_section(
    tool_call: &crate::app::ToolCallEntry,
    app: &AppState,
    show_tool_details: bool,
    timestamps_visible: bool,
    show_generic_tool_output: bool,
    tool_output_expanded: bool,
    stacked_diffs: bool,
    session_path: Option<&Path>,
) -> Option<TranscriptToolCallSection> {
    if tool_hidden_from_transcript(tool_call) {
        return None;
    }

    let task_row = app.transcript_task_row_for_tool_call(tool_call);

    let mut section = build_transcript_tool_call_section(
        tool_call,
        app,
        task_row.as_ref(),
        timestamps_visible,
        show_generic_tool_output,
        tool_output_expanded,
        stacked_diffs,
        session_path,
    );
    super::ui_transcript_subagent::refresh_started_status(
        &mut section,
        tool_call,
        task_row.as_ref(),
        app,
    );
    section.group.expanded = app.tool_group_expanded(&section.tool_call_id);
    if app.tool_output_previewed(&tool_call.tool_call_id) {
        section.expanded = false;
        section.header.disclosure_state = Some(TranscriptToolCallDisclosureState::Collapsed);
    }
    if let Some(child) = section
        .child_session_id
        .as_ref()
        .filter(|_| tool_family(&section) == TranscriptToolFamily::Task)
    {
        section.group.sources.insert(child.clone());
    } else if matches!(tool_call.effective_tool_id(), "search.web" | "websearch")
        && tool_call.status == ToolCallDisplayStatus::Succeeded
    {
        section.group.sources = super::super::ui_recorded_tool_output::web_sources(tool_call)
            .into_iter()
            .collect();
    }
    section.details_preview_visible |= show_tool_details
        && !section.detail_blocks.is_empty()
        && (app.tool_output_previewed(&tool_call.tool_call_id)
            || matches!(
                tool_family(&section),
                TranscriptToolFamily::Task | TranscriptToolFamily::Question
            )
            || matches!(
                tool_call.effective_tool_id(),
                "edit.hashline_apply" | "apply_patch" | "todo.write" | "todowrite"
            ));
    // These reference blocks have no animated accent; only commands and task
    // lifecycle rows signal execution with a wave.
    if matches!(section.rail_motion, ToolRailMotion::Running { .. })
        && matches!(
            tool_family(&section),
            TranscriptToolFamily::Read
                | TranscriptToolFamily::Edit
                | TranscriptToolFamily::Search
                | TranscriptToolFamily::List
        )
    {
        section.rail_motion = ToolRailMotion::Settled;
    }
    if !section.details_visible()
        && matches!(section.rail_motion, ToolRailMotion::FinishFlash { .. })
    {
        section.rail_motion = ToolRailMotion::Settled;
    }
    if (is_mcp_tool_id(&section.header.tool_id)
        || super::ui_tool_titles::generic_tool_id(&section.header.tool_id))
        && !section.expanded
        && !section.details_preview_visible
    {
        section.rail_motion = ToolRailMotion::Settled;
    }
    Some(section)
}

fn read_output_block(
    tool_call: &crate::app::ToolCallEntry,
) -> Option<TranscriptToolCallDetailBlock> {
    let display = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.pointer("/metadata/display"));
    let content = display
        .and_then(|value| value.get("text"))
        .and_then(serde_json::Value::as_str);
    let text = content
        .or_else(|| {
            display
                .and_then(|value| value.get("preview"))
                .and_then(serde_json::Value::as_str)
        })
        .or(tool_call.output_summary.as_deref())?;
    if text.is_empty() {
        return Some(TranscriptToolCallDetailBlock::Message {
            text: "(empty file)".to_string(),
            tone: TranscriptToolCallDetailTone::Secondary,
        });
    }
    let start_line = content
        .and_then(|_| display.and_then(|value| value.get("lineStart")))
        .and_then(serde_json::Value::as_u64);
    Some(TranscriptToolCallDetailBlock::ReadOutput {
        text: text.to_string(),
        start_line,
    })
}

fn suppress_unexecuted_edit_proposals(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
) {
    // Input-derived replacements are proposals, not output from a completed
    // edit. Keep recorded diff artifacts, including partial failure results.
    if matches!(tool_call.effective_tool_id(), "write" | "fs.write" | "edit")
        && tool_call.status != ToolCallDisplayStatus::Succeeded
        && tool_call_diff_artifacts(tool_call).is_empty()
    {
        detail_blocks
            .retain(|block| !matches!(block, TranscriptToolCallDetailBlock::StructuredDiff { .. }));
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "tool-row assembly keeps transcript toggles and state inputs explicit at the call site"
)]
pub(super) fn build_transcript_tool_call_section(
    tool_call: &crate::app::ToolCallEntry,
    app: &AppState,
    task_row: Option<&crate::app::OrchestrationTaskRow>,
    _timestamps_visible: bool,
    show_generic_tool_output: bool,
    tool_output_expanded: bool,
    stacked_diffs: bool,
    session_path: Option<&Path>,
) -> TranscriptToolCallSection {
    let viewer_only = tool_output_is_viewer_only(tool_call);
    let error_hidden_inline = tool_error_hidden_inline(tool_call);
    let tool_output_expanded = tool_output_expanded && !viewer_only;
    fn initial_tool_row(
        tool_call: &crate::app::ToolCallEntry,
        app: &AppState,
        task_row: Option<&crate::app::OrchestrationTaskRow>,
        show_generic_tool_output: bool,
        tool_output_expanded: bool,
        stacked_diffs: bool,
        session_path: Option<&Path>,
    ) -> (
        String,
        Option<&'static str>,
        TranscriptToolCallVisualStyle,
        bool,
        Vec<TranscriptToolCallDetailBlock>,
        Option<String>,
    ) {
        let mut detail_blocks = Vec::new();
        let display_tool_id = tool_call.effective_tool_id();
        let expanded = tool_output_expanded;
        let generic_output_visible = show_generic_tool_output || tool_output_expanded;
        let error_body = tool_error_text(tool_call);
        let output_tone = if tool_call.status == ToolCallDisplayStatus::Failed {
            TranscriptToolCallDetailTone::Error
        } else {
            TranscriptToolCallDetailTone::Primary
        };
        let question_answers = resolved_question_answer_items(tool_call);
        let todo_items = todo_items_from_tool_call(tool_call, session_path);
        let mut header_path_metadata = None;

        let theme = app.theme();

        let (title, icon, visual_style, uses_generic_output_visibility) = match display_tool_id {
            "fs.read" | "read" => {
                let path = tool_path_display(tool_call);
                let (title, icon) = read_tool_row_header(tool_call, app, path.as_deref());
                header_path_metadata = if title == "Skill" {
                    path.as_deref()
                        .and_then(|path| Path::new(path).parent()?.file_name()?.to_str())
                        .map(str::to_owned)
                } else {
                    path
                };
                if generic_output_visible && tool_call.status != ToolCallDisplayStatus::Failed {
                    detail_blocks.extend(read_output_block(tool_call));
                }
                (
                    title,
                    icon,
                    generic_tool_visual_style(tool_call, generic_output_visible),
                    true,
                )
            }
            "fs.glob" | "glob" | "fs.grep" | "grep" => {
                header_path_metadata = tool_path_display(tool_call).filter(|path| path != ".");
                (
                    super::super::ui_tool_titles_harness::local_search_tool_title(tool_call),
                    Some("✱"),
                    generic_tool_visual_style(tool_call, generic_output_visible),
                    true,
                )
            }
            "fs.ls" | "list" => {
                header_path_metadata = tool_path_display(tool_call);
                (
                    "List".to_string(),
                    Some("→"),
                    generic_tool_visual_style(tool_call, generic_output_visible),
                    true,
                )
            }
            "shell.run" | "bash" => {
                let cmd = shell_tool_command(tool_call).unwrap_or_else(|| "Shell".to_string());
                let description = shell_tool_title_description(tool_call, session_path);
                let title = format!("Run {}", description.as_deref().unwrap_or(&cmd));
                let shell_output =
                    shell_tool_output(tool_call).or_else(|| expanded.then(String::new));
                let has_output = shell_output.is_some();
                if let Some(output) = shell_output {
                    push_bash_panel_block(&mut detail_blocks, &cmd, &output, description);
                }
                (
                    title,
                    None,
                    TranscriptToolCallVisualStyle::Block,
                    has_output,
                )
            }
            "edit.hashline_apply" => {
                let (title, icon) = hashline_tool_row_header(
                    tool_call,
                    app,
                    &mut detail_blocks,
                    session_path,
                    stacked_diffs,
                );
                (title, icon, TranscriptToolCallVisualStyle::Block, false)
            }
            "edit.hashline_scan" => (
                format!(
                    "Scan {}",
                    tool_path_display(tool_call).unwrap_or_else(|| "file".to_string())
                ),
                Some("→"),
                TranscriptToolCallVisualStyle::Inline,
                false,
            ),
            "agent.spawn" | "task" => {
                build_agent_spawn_tool_row(tool_call, task_row, &mut detail_blocks)
            }
            "background_output" => (
                background_output_tool_title(tool_call),
                Some("↻"),
                TranscriptToolCallVisualStyle::Inline,
                false,
            ),
            "background_cancel" => (
                background_cancel_tool_title(tool_call),
                Some("✕"),
                TranscriptToolCallVisualStyle::Inline,
                false,
            ),
            "plan_enter" => (
                plan_enter_tool_title(tool_call),
                Some("⊕"),
                TranscriptToolCallVisualStyle::Inline,
                false,
            ),
            "plan_exit" => (
                plan_exit_tool_title(tool_call),
                Some("⊖"),
                TranscriptToolCallVisualStyle::Inline,
                false,
            ),
            "invalid" => (
                invalid_tool_title(tool_call),
                Some("!"),
                generic_tool_visual_style(tool_call, generic_output_visible),
                true,
            ),
            "session_list" => (
                session_tool_title(tool_call, "List"),
                Some("≡"),
                generic_tool_visual_style(tool_call, generic_output_visible),
                true,
            ),
            "session_read" => (
                session_tool_title(tool_call, "Read"),
                Some("→"),
                generic_tool_visual_style(tool_call, generic_output_visible),
                true,
            ),
            "session_search" => (
                session_tool_title(tool_call, "Search"),
                Some("✱"),
                generic_tool_visual_style(tool_call, generic_output_visible),
                true,
            ),
            "session_info" => (
                session_tool_title(tool_call, "Inspect"),
                Some("ⓘ"),
                generic_tool_visual_style(tool_call, generic_output_visible),
                true,
            ),
            "ast_grep_search" => (
                ast_grep_tool_title(tool_call, "AST Search"),
                Some("✱"),
                generic_tool_visual_style(tool_call, generic_output_visible),
                true,
            ),
            "ast_grep_replace" => {
                let rendered_diff = push_tool_call_diff_blocks(
                    &mut detail_blocks,
                    tool_call,
                    app,
                    session_path,
                    stacked_diffs,
                );
                (
                    ast_grep_tool_title(tool_call, "AST Replace"),
                    Some("←"),
                    if rendered_diff {
                        TranscriptToolCallVisualStyle::Block
                    } else {
                        generic_tool_visual_style(tool_call, generic_output_visible)
                    },
                    true,
                )
            }
            "lsp" => (
                lsp_tool_title(tool_call),
                Some("→"),
                generic_tool_visual_style(tool_call, generic_output_visible),
                true,
            ),
            "lsp.rename" => {
                let rendered_diff = push_tool_call_diff_blocks(
                    &mut detail_blocks,
                    tool_call,
                    app,
                    session_path,
                    stacked_diffs,
                );
                (
                    lsp_tool_title(tool_call),
                    Some("→"),
                    if rendered_diff {
                        TranscriptToolCallVisualStyle::Block
                    } else {
                        generic_tool_visual_style(tool_call, generic_output_visible)
                    },
                    true,
                )
            }
            "skill" | "skill.load" => (
                skill_tool_title(tool_call),
                Some("→"),
                TranscriptToolCallVisualStyle::Inline,
                false,
            ),
            "todo.read" | "todoread" => (
                "Read todos".to_string(),
                Some("☑"),
                TranscriptToolCallVisualStyle::Inline,
                false,
            ),
            "todo.write" | "todowrite" => {
                let still_running = matches!(
                    tool_call.status,
                    ToolCallDisplayStatus::Running
                        | ToolCallDisplayStatus::Queued
                        | ToolCallDisplayStatus::PendingPermission
                );
                if still_running && todo_items.is_empty() {
                    (
                        "Updating todos...".to_string(),
                        Some("⚙"),
                        TranscriptToolCallVisualStyle::Inline,
                        false,
                    )
                } else {
                    if !todo_items.is_empty() {
                        detail_blocks.push(TranscriptToolCallDetailBlock::TodoList {
                            items: todo_items.clone(),
                        });
                    }
                    (
                        "# Todos".to_string(),
                        None,
                        TranscriptToolCallVisualStyle::Block,
                        false,
                    )
                }
            }
            "fs.write" | "write" => {
                let rendered_diff = push_tool_call_diff_blocks(
                    &mut detail_blocks,
                    tool_call,
                    app,
                    session_path,
                    stacked_diffs,
                );
                let title = write_tool_title(tool_call);
                (
                    title,
                    Some("←"),
                    if rendered_diff {
                        TranscriptToolCallVisualStyle::Block
                    } else {
                        TranscriptToolCallVisualStyle::Inline
                    },
                    false,
                )
            }
            "edit" => {
                let rendered_diff = push_tool_call_diff_blocks(
                    &mut detail_blocks,
                    tool_call,
                    app,
                    session_path,
                    stacked_diffs,
                );
                let visual_style = if rendered_diff {
                    TranscriptToolCallVisualStyle::Block
                } else {
                    TranscriptToolCallVisualStyle::Inline
                };
                (edit_tool_title(tool_call), Some("←"), visual_style, false)
            }
            "apply_patch" => {
                let rendered_diff = push_tool_call_diff_blocks(
                    &mut detail_blocks,
                    tool_call,
                    app,
                    session_path,
                    stacked_diffs,
                );
                (
                    apply_patch_tool_title(tool_call),
                    Some("%"),
                    if rendered_diff {
                        TranscriptToolCallVisualStyle::Block
                    } else {
                        TranscriptToolCallVisualStyle::Inline
                    },
                    false,
                )
            }
            "web.fetch" | "webfetch" => (
                format!(
                    "Fetch {}",
                    tool_summary_string(&tool_call.args_summary, &["url"])
                        .unwrap_or_else(|| "url".to_string())
                ),
                Some("%"),
                TranscriptToolCallVisualStyle::Inline,
                true,
            ),
            "search.web" | "websearch" | "search.code" => {
                let is_web = matches!(display_tool_id, "search.web" | "websearch");
                (
                    search_tool_title(tool_call, expanded),
                    Some(if is_web {
                        theme.live_shell.transcript_glyphs.group_marker
                    } else {
                        theme.live_shell.transcript_glyphs.thought_marker
                    }),
                    TranscriptToolCallVisualStyle::Inline,
                    true,
                )
            }
            "user.question" | "question" => {
                if !question_answers.is_empty() {
                    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                        text: question_answers
                            .iter()
                            .enumerate()
                            .map(|(index, item)| {
                                format!(
                                    "  {}. {}\n     → {}",
                                    index.saturating_add(1),
                                    item.question,
                                    item.answer
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                        tone: TranscriptToolCallDetailTone::Primary,
                    });
                }
                (
                    question_tool_title(tool_call, &question_answers),
                    Some("→"),
                    TranscriptToolCallVisualStyle::Inline,
                    false,
                )
            }
            "tool.batch" | "batch" => (
                batch_tool_title(tool_call),
                Some("#"),
                TranscriptToolCallVisualStyle::Block,
                true,
            ),
            "code.lsp" => (
                lsp_tool_title(tool_call),
                Some("→"),
                generic_tool_visual_style(tool_call, generic_output_visible),
                true,
            ),
            _ if is_mcp_tool_id(display_tool_id) => (
                mcp_tool_title(tool_call, display_tool_id),
                Some("⚙"),
                generic_tool_visual_style(tool_call, generic_output_visible),
                true,
            ),
            _ => {
                let title = generic_tool_title(tool_call, display_tool_id);
                let generic_output = error_body
                    .as_deref()
                    .filter(|_| tool_call.status == ToolCallDisplayStatus::Failed)
                    .or(tool_call.output_summary.as_deref());
                if generic_output_visible && generic_output.is_some() {
                    push_collapsible_output_block(
                        &mut detail_blocks,
                        generic_output.unwrap_or_default(),
                        output_tone,
                        HARNESS_GENERIC_OUTPUT_LINE_CLAMP,
                        expanded,
                    );
                    (title, None, TranscriptToolCallVisualStyle::Block, true)
                } else {
                    (
                        title,
                        Some("⚙"),
                        generic_tool_visual_style(tool_call, generic_output_visible),
                        true,
                    )
                }
            }
        };

        (
            title,
            icon,
            visual_style,
            uses_generic_output_visibility,
            detail_blocks,
            header_path_metadata,
        )
    }
    let (
        mut title,
        icon,
        visual_style,
        uses_generic_output_visibility,
        mut detail_blocks,
        mut header_path_metadata,
    ) = initial_tool_row(
        tool_call,
        app,
        task_row,
        show_generic_tool_output,
        tool_output_expanded,
        stacked_diffs,
        session_path,
    );
    let struck_out = tool_call_denied(tool_call);
    let display_tool_id = tool_call.effective_tool_id();
    let expanded = tool_output_expanded;
    let generic_output_visible = show_generic_tool_output || expanded;
    let animation_phase = app.transcript_animation_phase();
    let error_body = tool_error_text(tool_call);
    let error_subtitle = tool_error_subtitle(tool_call);
    let child_session_id = task_tool_child_session_id(tool_call)
        .map(str::to_string)
        .or_else(|| {
            task_row
                .and_then(|row| row.child_session_id.as_deref())
                .map(str::to_string)
        });

    suppress_unexecuted_edit_proposals(&mut detail_blocks, tool_call);
    attach_recorded_diff_sources(&mut detail_blocks, tool_call, app);
    set_diff_highlight_phase(
        &mut detail_blocks,
        tool_call.status == ToolCallDisplayStatus::Succeeded,
    );

    if tool_call.status == ToolCallDisplayStatus::Succeeded
        && matches!(
            display_tool_id,
            "edit.hashline_apply" | "edit" | "write" | "fs.write"
        )
    {
        title = edit_tool_action(tool_call).to_string();
    }

    replace_recorded_output(&mut detail_blocks, tool_call, generic_output_visible);
    if detail_blocks.is_empty()
        && uses_generic_output_visibility
        && if tool_call.status == ToolCallDisplayStatus::Failed {
            error_body.is_some() || tool_call.output_summary.is_some()
        } else {
            tool_call.output_summary.is_some()
        }
        && generic_output_visible
    {
        push_collapsible_output_block(
            &mut detail_blocks,
            if tool_call.status == ToolCallDisplayStatus::Failed {
                error_body
                    .as_deref()
                    .or(tool_call.output_summary.as_deref())
            } else {
                tool_call.output_summary.as_deref()
            }
            .unwrap_or_default(),
            if tool_call.status == ToolCallDisplayStatus::Failed {
                TranscriptToolCallDetailTone::Error
            } else {
                TranscriptToolCallDetailTone::Primary
            },
            HARNESS_GENERIC_OUTPUT_LINE_CLAMP,
            expanded,
        );
    }

    if !matches!(
        display_tool_id,
        "user.question" | "question" | "agent.spawn" | "task"
    ) {
        push_failed_tool_error_block(&mut detail_blocks, tool_call);
    }
    push_truncated_output_artifact_block(&mut detail_blocks, tool_call);
    prepare_failed_tool_details(&mut detail_blocks, tool_call);

    let details_collapsed_by_default =
        viewer_only || tool_call_has_transcript_disclosure(tool_call);
    let details_preview_visible =
        show_generic_tool_output && uses_generic_output_visibility && !viewer_only;
    let disclosure_state = if details_collapsed_by_default && !viewer_only {
        Some(if expanded {
            TranscriptToolCallDisclosureState::Expanded
        } else {
            TranscriptToolCallDisclosureState::Collapsed
        })
    } else {
        None
    };
    let default_subtitle = match display_tool_id {
        "shell.run" | "bash" => {
            super::super::ui_transcript_bash::shell_tool_workdir_display(tool_call, session_path)
                .map(|path| format!("in {path}"))
        }
        "fs.read" | "read" => read_tool_subtitle(tool_call),
        "fs.ls" | "list" => (tool_call.status == ToolCallDisplayStatus::Succeeded)
            .then(|| tool_entry_count(tool_call))
            .flatten()
            .map(|count| format!("({count} {})", if count == 1 { "entry" } else { "entries" })),
        "fs.glob" | "glob" | "fs.grep" | "grep" => tool_match_count_description(tool_call),
        "search.web" | "websearch" if !expanded => {
            let sources = super::super::ui_recorded_tool_output::web_sources(tool_call);
            let count = super::super::ui_recorded_tool_output::source_domains(&sources).len();
            (count > 0).then(|| format!("({count} site{})", if count == 1 { "" } else { "s" }))
        }
        "edit.hashline_apply" | "fs.write" | "write" | "edit" => {
            header_path_metadata = tool_call
                .edit_path_display()
                .or_else(|| tool_path_display(tool_call));
            // Pending/failed edit and write titles may already contain the
            // path. Paint it only in the dedicated path span, as on success.
            let action = header_path_metadata
                .as_ref()
                .and_then(|path| title.strip_suffix(&format!(" {path}")));
            if let Some(action) = action {
                title = action.to_string();
            }
            None
        }
        "background_output" => background_output_tool_subtitle(tool_call),
        "agent.spawn" | "task" => agent_spawn_subtitle(tool_call),
        "apply_patch" => None,
        _ => None,
    };
    let rail_motion = tool_rail_motion(tool_call, app, !detail_blocks.is_empty());

    TranscriptToolCallSection {
        group: Default::default(),
        hook_executions: tool_call.hook_executions.clone(),
        tool_call_id: tool_call.tool_call_id.clone(),
        coalesced_tool_call_ids: vec![tool_call.tool_call_id.clone()],
        child_session_id,
        subagent_background: matches!(display_tool_id, "agent.spawn" | "task")
            && agent_spawn_is_background(tool_call),
        output_truncated: tool_call.truncated_output.is_some(),
        replay_read_only: app.replay_mode,
        hovered_target: app.hovered_transcript_target().cloned(),
        header: TranscriptToolCallHeader {
            selected: tool_header_selected(app, &tool_call.tool_call_id),
            tool_id: if matches!(display_tool_id, "shell.run" | "bash") {
                display_tool_id.to_string()
            } else {
                tool_call.tool_id.clone()
            },
            title,
            subtitle: if tool_call.status == ToolCallDisplayStatus::Failed
                && !error_hidden_inline
                && !is_mcp_tool_id(display_tool_id)
                && !matches!(
                    display_tool_id,
                    "user.question"
                        | "question"
                        | "bash"
                        | "shell.run"
                        | "edit"
                        | "write"
                        | "fs.write"
                        | "edit.hashline_apply"
                ) {
                join_tool_subtitles(default_subtitle, error_subtitle)
            } else {
                default_subtitle
            },
            path_metadata: header_path_metadata,
            icon,
            presentation: tool_call.presentation(),
            visual_style,
            struck_out,
            disclosure_state,
        },
        detail_blocks,
        details_collapsed_by_default,
        details_preview_visible,
        animation_phase,
        expanded,
        rail_motion,
    }
}

fn tool_rail_motion(
    tool: &crate::app::ToolCallEntry,
    app: &AppState,
    has_details: bool,
) -> ToolRailMotion {
    if app.replay_mode || !app.transcript_motion_enabled() {
        return ToolRailMotion::Settled;
    }
    if tool.has_execution_motion() {
        return ToolRailMotion::Running {
            elapsed: std::time::Duration::from_millis(
                u64::try_from(app.transcript_animation_phase())
                    .unwrap_or(u64::MAX)
                    .saturating_mul(crate::scheduling::active_animation_period_ms()),
            ),
            sampled_phase: app.transcript_animation_phase(),
        };
    }
    match tool.status {
        ToolCallDisplayStatus::PendingPermission => ToolRailMotion::Waiting,
        ToolCallDisplayStatus::Queued => ToolRailMotion::Queued,
        ToolCallDisplayStatus::Succeeded | ToolCallDisplayStatus::Failed => app
            .tool_finish_elapsed(&tool.tool_call_id)
            .filter(|_| has_details)
            .map_or(ToolRailMotion::Settled, |elapsed| {
                ToolRailMotion::FinishFlash {
                    elapsed,
                    sampled_phase: app.transcript_animation_phase(),
                }
            }),
        ToolCallDisplayStatus::Running => ToolRailMotion::Settled,
    }
}

fn prepare_failed_tool_details(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
) {
    if tool_error_hidden_inline(tool_call) {
        detail_blocks.clear();
        return;
    }

    // Failed edits reveal their error below the header, using the same muted
    // decoration as Grok's edit block. Keep the full recorded error text.
    if tool_call.status == ToolCallDisplayStatus::Failed
        && matches!(
            tool_call.effective_tool_id(),
            "edit" | "write" | "fs.write" | "edit.hashline_apply"
        )
    {
        for block in detail_blocks {
            if let TranscriptToolCallDetailBlock::Message { text, tone } = block {
                if *tone == TranscriptToolCallDetailTone::Error {
                    *text = format!("\n{text}");
                    *tone = TranscriptToolCallDetailTone::Secondary;
                }
            }
        }
    }
}

fn read_tool_subtitle(tool: &crate::app::ToolCallEntry) -> Option<String> {
    if let Some(mime) = super::super::ui_tool_metadata::read_media_mime(tool) {
        return Some(
            if mime == "application/pdf" {
                "(PDF)"
            } else {
                "(image)"
            }
            .into(),
        );
    }
    let range = read_tool_input_suffix(tool);
    (!range.is_empty() && tool.status != ToolCallDisplayStatus::Failed).then_some(range)
}

fn read_tool_row_header(
    tool_call: &crate::app::ToolCallEntry,
    app: &AppState,
    path: Option<&str>,
) -> (String, Option<&'static str>) {
    let title =
        if path.is_some_and(|path| path.ends_with("/SKILL.md") && path.contains("/skills/")) {
            "Skill"
        } else {
            "Read"
        }
        .to_string();
    let icon = match tool_call.status {
        ToolCallDisplayStatus::Running => glyph_routed_streaming_spinner_frame(
            app.theme(),
            app.transcript_animation_phase(),
            app.transcript_motion_enabled(),
        ),
        ToolCallDisplayStatus::PendingPermission | ToolCallDisplayStatus::Queued
            if path.is_none() =>
        {
            "~"
        }
        ToolCallDisplayStatus::PendingPermission
        | ToolCallDisplayStatus::Queued
        | ToolCallDisplayStatus::Succeeded
        | ToolCallDisplayStatus::Failed => "→",
    };
    (title, Some(icon))
}

fn hashline_tool_row_header(
    tool_call: &crate::app::ToolCallEntry,
    app: &AppState,
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    session_path: Option<&Path>,
    stacked_diffs: bool,
) -> (String, Option<&'static str>) {
    let path = tool_call.edit_path_display();
    let title = match tool_call.edit.as_ref().map(|edit| edit.status) {
        Some(crate::app::EditDisplayStatus::Applied) => "Patch".to_string(),
        Some(crate::app::EditDisplayStatus::Rejected | crate::app::EditDisplayStatus::Proposed)
        | None => path
            .as_ref()
            .map_or_else(|| "Preparing edit...".to_string(), |_| "Edit".to_string()),
    };
    if let Some(edit) = &tool_call.edit {
        match edit.status {
            crate::app::EditDisplayStatus::Applied => {
                push_tool_call_diff_blocks(
                    detail_blocks,
                    tool_call,
                    app,
                    session_path,
                    stacked_diffs,
                );
            }
            crate::app::EditDisplayStatus::Rejected => {
                if let Some(reason) = edit.rejection_reason.as_deref() {
                    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                        text: reason.to_string(),
                        tone: TranscriptToolCallDetailTone::Error,
                    });
                }
            }
            crate::app::EditDisplayStatus::Proposed => {}
        }
    }
    let icon = if title == "Preparing edit..." {
        "~"
    } else {
        "←"
    };
    (title, Some(icon))
}

pub(super) fn tool_header_selected(app: &AppState, tool_call_id: &str) -> bool {
    app.focus == Focus::Details
        && !app.todo_pane_focused()
        && matches!(
            app.transcript_view.selected_entry,
            Some(TranscriptVisualEntryId::Part { semantic_key, .. }
                | TranscriptVisualEntryId::ToolGroup { semantic_key, .. })
                if semantic_key == super::ui_transcript_entry::semantic_key([tool_call_id])
        )
}

fn search_tool_title(tool: &crate::app::ToolCallEntry, expanded: bool) -> String {
    let id = tool.effective_tool_id();
    let is_web = matches!(id, "search.web" | "websearch");
    let label = if is_web {
        web_search_provider_label(tool)
    } else {
        "Exa Code Search"
    };
    let query =
        tool_summary_string(&tool.args_summary, &["query"]).unwrap_or_else(|| "query".into());
    let suffix = if expanded || is_web {
        String::new()
    } else {
        search_result_count_suffix(tool, id)
    };
    format!("{label} {query}{suffix}")
}

fn web_search_provider_label(tool_call: &crate::app::ToolCallEntry) -> &'static str {
    match tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("provider"))
        .and_then(serde_json::Value::as_str)
    {
        Some("parallel") => "Parallel Web Search",
        Some("exa") => "Exa Web Search",
        _ => "Web Search",
    }
}

pub(super) fn set_diff_highlight_phase(
    blocks: &mut [TranscriptToolCallDetailBlock],
    full_file_ready: bool,
) {
    for block in blocks {
        match block {
            TranscriptToolCallDetailBlock::StructuredDiff {
                highlight_syntax, ..
            } => *highlight_syntax = full_file_ready,
            TranscriptToolCallDetailBlock::FileSection(section) => {
                set_diff_highlight_phase(&mut section.detail_blocks, full_file_ready);
            }
            _ => {}
        }
    }
}

pub(super) fn edit_tool_action(tool_call: &crate::app::ToolCallEntry) -> &'static str {
    if matches!(tool_call.effective_tool_id(), "write" | "fs.write") {
        "Creating"
    } else {
        "Edit"
    }
}

fn push_tool_call_diff_blocks(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
    app: &AppState,
    session_path: Option<&Path>,
    stacked_diffs: bool,
) -> bool {
    let Some(session_path) = session_path else {
        return false;
    };

    if tool_call.effective_tool_id() == "apply_patch" {
        let file_entries = collect_apply_patch_file_render_entries(tool_call);
        if file_entries.len() > 1 {
            return push_apply_patch_file_sections(
                detail_blocks,
                tool_call,
                app,
                session_path,
                stacked_diffs,
                &file_entries,
            );
        }
    }

    let diff_artifacts = tool_call_diff_artifacts(tool_call);
    let path_already_in_title = tool_call.edit.is_some()
        || matches!(tool_call.effective_tool_id(), "edit" | "write" | "fs.write");
    let show_file_header = !path_already_in_title || diff_artifacts.len() > 1;
    let force_stacked =
        stacked_diffs || matches!(tool_call.effective_tool_id(), "write" | "fs.write");
    let plain_numbered = matches!(tool_call.effective_tool_id(), "write" | "fs.write");
    let mut rendered = false;
    for (diff_rel_path, fallback_path) in diff_artifacts {
        rendered |= push_structured_diff_artifact_block(
            detail_blocks,
            app,
            &diff_rel_path,
            fallback_path.as_deref(),
            force_stacked,
            plain_numbered,
            show_file_header,
        );
    }

    if !rendered {
        if let Some((diff_content, fallback_path)) = tool_call_inline_diff_block(tool_call) {
            detail_blocks.push(TranscriptToolCallDetailBlock::StructuredDiff {
                before_source: None,
                diff_content,
                fallback_path,
                force_stacked,
                plain_numbered,
                highlight_syntax: false,
                show_file_header: !matches!(
                    tool_call.effective_tool_id(),
                    "edit" | "write" | "fs.write"
                ),
            });
            return true;
        }

        if tool_call.effective_tool_id() == "apply_patch" {
            let file_entries = collect_apply_patch_file_render_entries(tool_call);
            if file_entries.len() > 1 {
                return push_apply_patch_file_sections(
                    detail_blocks,
                    tool_call,
                    app,
                    session_path,
                    stacked_diffs,
                    &file_entries,
                );
            }
            if let Some(rows) = tool_call_apply_patch_file_rows(tool_call) {
                for row in rows {
                    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                        text: row,
                        tone: TranscriptToolCallDetailTone::Secondary,
                    });
                }
                return true;
            }
        }

        // Optional preview artifacts are not user-facing errors. The header already
        // carries the truthful file summary when an artifact is absent or unreadable.
    }

    rendered
}

fn push_apply_patch_file_sections(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
    app: &AppState,
    _session_path: &Path,
    stacked_diffs: bool,
    file_entries: &[ApplyPatchFileRenderEntry],
) -> bool {
    if file_entries.is_empty() {
        return false;
    }

    for entry in file_entries {
        let mut file_detail_blocks = Vec::new();
        if let Some(diff_rel_path) = entry.diff_rel_path.as_deref() {
            let _ = push_structured_diff_artifact_block(
                &mut file_detail_blocks,
                app,
                diff_rel_path,
                Some(&entry.file_path),
                stacked_diffs,
                false,
                false,
            );
        }
        if file_detail_blocks.is_empty() {
            detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                text: entry.file_path.clone(),
                tone: TranscriptToolCallDetailTone::Secondary,
            });
            continue;
        }
        let metadata =
            tool_call_path_metadata(Some(&entry.file_path)).unwrap_or(TranscriptPathMetadata {
                leaf: entry.file_path.clone(),
                parent: None,
            });
        detail_blocks.push(TranscriptToolCallDetailBlock::FileSection(
            TranscriptToolCallFileSection {
                tool_call_id: tool_call.tool_call_id.clone(),
                file_path: entry.file_path.clone(),
                title: metadata.leaf,
                subtitle: metadata.parent,
                disclosure_state: if app
                    .patch_file_output_expanded(&tool_call.tool_call_id, &entry.file_path)
                {
                    TranscriptToolCallDisclosureState::Expanded
                } else {
                    TranscriptToolCallDisclosureState::Collapsed
                },
                detail_blocks: file_detail_blocks,
            },
        ));
    }

    true
}

fn push_structured_diff_artifact_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    app: &AppState,
    diff_rel_path: &str,
    fallback_path: Option<&str>,
    force_stacked: bool,
    plain_numbered: bool,
    show_file_header: bool,
) -> bool {
    let Some(diff_content) = app.recorded_artifacts.get(diff_rel_path).cloned() else {
        return false;
    };
    detail_blocks.push(TranscriptToolCallDetailBlock::StructuredDiff {
        before_source: None,
        diff_content,
        fallback_path: fallback_path.map(str::to_string),
        force_stacked,
        plain_numbered,
        highlight_syntax: false,
        show_file_header,
    });
    true
}

pub(super) fn build_agent_spawn_tool_row(
    tool_call: &crate::app::ToolCallEntry,
    task_row: Option<&crate::app::OrchestrationTaskRow>,
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
) -> (
    String,
    Option<&'static str>,
    TranscriptToolCallVisualStyle,
    bool,
) {
    let activity = task_row
        .filter(|row| !row.state.is_terminal())
        .and_then(|row| row.current_child_tool_title.as_deref());
    let title = agent_spawn_title(tool_call, agent_spawn_description(tool_call), activity);
    detail_blocks.clear();
    (
        title,
        None,
        TranscriptToolCallVisualStyle::TaskInline,
        false,
    )
}

fn push_running_subagent_detail(
    task_row: Option<&crate::app::OrchestrationTaskRow>,
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
) {
    let Some(row) = task_row else {
        return;
    };
    let detail = row
        .warning
        .as_deref()
        .map(|warning| format!("↳ {warning}"))
        .or_else(|| {
            row.current_child_tool_title
                .as_deref()
                .map(|title| format!("↳ {title}"))
        })
        .or_else(|| {
            (row.child_tool_call_count > 0)
                .then(|| format!("↳ {}", format_subagent_toolcalls(row.child_tool_call_count)))
        });
    if let Some(detail) = detail {
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: detail,
            tone: TranscriptToolCallDetailTone::Primary,
        });
    }
}

fn push_completed_subagent_detail(
    row: &crate::app::OrchestrationTaskRow,
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
) {
    let detail = format_completed_subagent_detail(
        row.child_tool_call_count,
        row.duration_ms()
            .map(format_duration_ms)
            .unwrap_or_default(),
    );
    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
        text: format!("↳ {detail}"),
        tone: TranscriptToolCallDetailTone::Primary,
    });
}

fn format_subagent_toolcalls(count: usize) -> String {
    format!("{count} toolcall{}", if count == 1 { "" } else { "s" })
}

fn format_completed_subagent_detail(toolcalls: usize, duration: String) -> String {
    if toolcalls == 0 {
        return duration;
    }
    format!("{} · {duration}", format_subagent_toolcalls(toolcalls))
}

fn task_result_markdown(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    let output = tool_call.output_summary.as_deref()?.trim();
    if output.is_empty() {
        return None;
    }

    if let Some(result) = tagged_task_result(output) {
        return Some(result.to_string());
    }

    let result = output
        .lines()
        .filter(|line| !line.starts_with("task_id:"))
        .collect::<Vec<_>>()
        .join("\n");
    let trimmed = result.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn tagged_task_result(output: &str) -> Option<&str> {
    let start = output.find("<task_result>")? + "<task_result>".len();
    let end = output[start..].find("</task_result>")? + start;
    let result = output[start..end].trim();
    (!result.is_empty()).then_some(result)
}

fn push_truncated_output_artifact_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
) {
    if tool_call.truncated_output.is_none() || tool_call.artifact_refs.is_empty() {
        return;
    }
    if !detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message {
                tone: TranscriptToolCallDetailTone::Primary,
                ..
            } | TranscriptToolCallDetailBlock::BashPanel { .. }
        )
    }) {
        return;
    }
    let artifact = &tool_call.artifact_refs[0];
    let text = format!(
        "Output truncated · full output at {artifact}",
        artifact = artifact.path
    );
    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
        text,
        tone: TranscriptToolCallDetailTone::Secondary,
    });
}

fn push_collapsible_output_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    output: &str,
    tone: TranscriptToolCallDetailTone,
    max_lines: usize,
    expanded: bool,
) {
    let preview = collapsible_output_preview(output, max_lines, expanded);
    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
        text: if tone == TranscriptToolCallDetailTone::Primary {
            format!("\n{preview}")
        } else {
            preview
        },
        tone: if tone == TranscriptToolCallDetailTone::Primary {
            TranscriptToolCallDetailTone::Secondary
        } else {
            tone
        },
    });
}

fn push_bash_panel_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    command: &str,
    output: &str,
    description: Option<String>,
) {
    detail_blocks.push(TranscriptToolCallDetailBlock::BashPanel {
        command: command.to_string(),
        // The terminal-cell interpreter consumes controls and redacts the final
        // text before paint. Stripping here would destroy SGR and CR overwrite.
        output: output.to_string(),
        description,
        expand_hint: None,
    });
}

fn tool_entry_count(tool_call: &crate::app::ToolCallEntry) -> Option<u64> {
    if let Some(summary) = tool_call.output_summary.as_deref() {
        let count = summary
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        if count > 0 {
            return Some(u64::try_from(count).unwrap_or(u64::MAX));
        }
    }
    if let Some(value) = tool_call.output_json.as_ref() {
        if let Some(count) = value
            .get("entry_count")
            .or_else(|| value.get("count"))
            .or_else(|| value.get("total_count"))
            .and_then(serde_json::Value::as_u64)
        {
            return Some(count);
        }
        if let Some(entries) = value.get("entries").and_then(serde_json::Value::as_array) {
            return Some(entries.len() as u64);
        }
    }
    None
}

fn attach_recorded_diff_sources(
    blocks: &mut [TranscriptToolCallDetailBlock],
    tool: &crate::app::ToolCallEntry,
    app: &AppState,
) {
    for block in blocks {
        match block {
            TranscriptToolCallDetailBlock::StructuredDiff {
                before_source,
                fallback_path,
                ..
            } => {
                let Some(value) = tool.output_json.as_ref() else {
                    continue;
                };
                let object = value
                    .get("edits")
                    .and_then(serde_json::Value::as_array)
                    .and_then(|edits| {
                        edits.iter().find(|edit| {
                            edit.get("path").and_then(serde_json::Value::as_str)
                                == fallback_path.as_deref()
                        })
                    })
                    .unwrap_or(value);
                *before_source = object
                    .get("before_rel_path")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|path| app.recorded_artifacts.get(path))
                    .cloned()
                    .or_else(|| {
                        object
                            .get("before_text")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string)
                    });
            }
            TranscriptToolCallDetailBlock::FileSection(file) => {
                attach_recorded_diff_sources(&mut file.detail_blocks, tool, app)
            }
            _ => {}
        }
    }
}

fn replace_recorded_output(
    blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool: &crate::app::ToolCallEntry,
    visible: bool,
) {
    if !visible {
        return;
    }
    if let Some(output) = super::super::ui_recorded_tool_output::project(tool) {
        blocks.clear();
        blocks.push(TranscriptToolCallDetailBlock::Recorded(output));
    }
}

#[cfg(test)]
mod presentation_section_tests {
    use super::*;
    use crate::app::{PermissionEntry, ToolCallDisplayStatus, ToolCallPresentationStatus};
    use crate::ui::ui_transcript_test_helpers::transcript_section_model_test_tool_call;

    fn section(tool_call: &crate::app::ToolCallEntry) -> TranscriptToolCallSection {
        build_transcript_tool_call_section(
            tool_call,
            &AppState::default(),
            None,
            false,
            false,
            false,
            false,
            None,
        )
    }

    #[test]
    fn static_tool_headers_preserve_recorded_identity_and_ranges() {
        // Given: native and MCP calls with recorded, displayable metadata.
        let cases = [
            (
                "read",
                r#"{"path":"src/main.rs","offset":42,"limit":20}"#,
                "Read",
                Some("src/main.rs"),
                Some("(42-61)"),
            ),
            (
                "list",
                r#"{"path":"src"}"#,
                "List",
                Some("src"),
                Some("(3 entries)"),
            ),
            (
                "bash",
                r#"{"command":"cargo test","description":"unit tests"}"#,
                "Run unit tests",
                None,
                None,
            ),
            (
                "mcp.database.query",
                r#"{"sql":"select 1"}"#,
                "Database Query",
                None,
                None,
            ),
        ];
        for (id, args, title, path, subtitle) in cases {
            let mut tool = transcript_section_model_test_tool_call(id, id);
            tool.status = ToolCallDisplayStatus::Succeeded;
            tool.args_summary = args.to_string();
            if id == "list" {
                tool.output_json = Some(serde_json::json!({"entry_count": 3}));
            }

            // When: the stored call is projected without performing tool work.
            let header = section(&tool).header;

            // Then: identity and metadata occupy separate header fields.
            assert_eq!(
                (
                    header.title.as_str(),
                    header.path_metadata.as_deref(),
                    header.subtitle.as_deref()
                ),
                (title, path, subtitle),
                "{id}"
            );
        }
    }

    #[test]
    fn search_markers_use_ascii_catalog_when_requested() {
        // Given: an ASCII-mode app and search rows that normally use Unicode diamonds.
        let mut app = AppState::default();
        app.set_glyph_mode(crate::theme::GlyphMode::Ascii);
        let web = transcript_section_model_test_tool_call("web", "search.web");
        let code = transcript_section_model_test_tool_call("code", "search.code");

        // When: the search markers are projected.
        let markers = [&web, &code].map(|tool_call| {
            build_transcript_tool_call_section(
                tool_call, &app, None, false, false, false, false, None,
            )
            .header
            .icon
        });

        // Then: both use the catalog's ASCII-safe marker.
        assert_eq!(markers, [Some("*"), Some("*")]);
    }

    #[test]
    fn section_projects_waiting_and_correlated_cancelled_states() {
        // arrange
        let mut waiting = transcript_section_model_test_tool_call("waiting", "bash");
        waiting.status = ToolCallDisplayStatus::PendingPermission;
        assert_eq!(
            section(&waiting).header.presentation.status,
            ToolCallPresentationStatus::Waiting
        );

        // act
        let mut cancelled =
            transcript_section_model_test_tool_call("cancelled", "background_cancel");
        cancelled.status = ToolCallDisplayStatus::Succeeded;
        cancelled.args_summary = r#"{"request_id":"req-child"}"#.to_string();
        cancelled.output_json = Some(serde_json::json!({
            "request_id": "req-child",
            "status": "cancelled"
        }));
        // assert
        assert_eq!(
            section(&cancelled).header.presentation.status,
            ToolCallPresentationStatus::Cancelled
        );
    }

    #[test]
    fn section_preserves_terminal_metadata_and_disclosure_modes() {
        // arrange
        let mut tool_call = transcript_section_model_test_tool_call("generic", "custom.tool");
        tool_call.status = ToolCallDisplayStatus::Succeeded;
        tool_call.output_summary = Some("result body".to_string());
        tool_call.output_json = Some(serde_json::json!({ "result_count": 3 }));
        tool_call.timing_elapsed_ms = Some(850);

        // act
        let collapsed = section(&tool_call);
        let preview = build_transcript_tool_call_section(
            &tool_call,
            &AppState::default(),
            None,
            false,
            true,
            false,
            false,
            None,
        );
        let expanded = build_transcript_tool_call_section(
            &tool_call,
            &AppState::default(),
            None,
            false,
            false,
            true,
            false,
            None,
        );

        // assert
        assert_eq!(collapsed.header.presentation.duration_ms, Some(850));
        assert_eq!(collapsed.header.presentation.result_count, Some(3));
        assert_eq!(
            collapsed.header.disclosure_state,
            Some(TranscriptToolCallDisclosureState::Collapsed)
        );
        assert!(preview.details_preview_visible);
        assert_eq!(
            expanded.header.disclosure_state,
            Some(TranscriptToolCallDisclosureState::Expanded)
        );
    }

    #[test]
    fn resolved_question_renders_numbered_question_and_answer_pairs() {
        // arrange
        // Given: a completed native question call with one answer and one omission.
        let mut tool_call = transcript_section_model_test_tool_call("question", "question");
        tool_call.status = ToolCallDisplayStatus::Succeeded;
        tool_call.args_summary =
            r#"{"questions":[{"question":"Pick one"},{"question":"Pick two"}]}"#.to_string();
        tool_call.permissions.push(PermissionEntry {
            permission_id: "permission".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some(tool_call.tool_call_id.clone()),
            summary: tool_call.args_summary.clone(),
            request_digest: "digest".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
            resolved_decision: Some(harness_core::event::PermissionDecision::Allow),
            resolution_reason: Some(r#"[["Alpha"],[]]"#.to_string()),
            first_seq: 2,
            last_seq: 3,
        });

        // When: the transcript section is projected.
        let rendered = section(&tool_call);

        // act
        // assert
        assert_eq!(rendered.header.title, "Asked 2 questions");
        assert_eq!(
            rendered.detail_blocks,
            vec![TranscriptToolCallDetailBlock::Message {
                text: "  1. Pick one\n     → Alpha\n  2. Pick two\n     → (no answer)".to_string(),
                tone: TranscriptToolCallDetailTone::Primary,
            }]
        );
    }
}
