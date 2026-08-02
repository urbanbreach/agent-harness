// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use super::ui_tool_delegation::agent_spawn_is_background;
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

    if tool_call.status == ToolCallDisplayStatus::Succeeded
        && !show_tool_details
        && !tool_call_should_remain_visible_without_tool_details(tool_call)
    {
        return None;
    }

    let task_row = app.transcript_task_row_for_tool_call(tool_call);

    Some(build_transcript_tool_call_section(
        tool_call,
        app,
        task_row.as_ref(),
        timestamps_visible,
        show_generic_tool_output,
        tool_output_expanded,
        stacked_diffs,
        session_path,
    ))
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
    let struck_out = tool_call_denied(tool_call);
    let mut detail_blocks = Vec::new();
    let expanded = tool_output_expanded;
    let generic_output_visible = show_generic_tool_output || tool_output_expanded;
    let display_tool_id = tool_call.effective_tool_id();
    let child_session_id = task_tool_child_session_id(tool_call)
        .map(str::to_string)
        .or_else(|| {
            task_row
                .and_then(crate::app::OrchestrationTaskRow::effective_child_session_id)
                .map(str::to_string)
        });
    let error_subtitle = tool_error_subtitle(tool_call);
    let error_body = tool_error_text(tool_call);
    let question_answers = resolved_question_answer_items(tool_call);
    let todo_items = todo_items_from_tool_call(tool_call, session_path);
    let mut header_path_metadata = None;

    let animation_phase = app.transcript_animation_phase();

    let (title, icon, visual_style, uses_generic_output_visibility) = match display_tool_id {
        "fs.read" | "read" => {
            let path = tool_path_display(tool_call);
            let title = match tool_call.status {
                ToolCallDisplayStatus::Succeeded => {
                    completed_read_tool_title(tool_call, path.as_deref())
                }
                ToolCallDisplayStatus::Running
                | ToolCallDisplayStatus::PendingPermission
                | ToolCallDisplayStatus::Queued
                | ToolCallDisplayStatus::Failed => path.as_ref().map_or_else(
                    || format!("{display_tool_id} · Reading file..."),
                    |path| format!("Read {path}{}", read_tool_input_suffix(tool_call)),
                ),
            };
            let icon = match tool_call.status {
                ToolCallDisplayStatus::Running => {
                    Some(transcript_streaming_spinner_frame(animation_phase))
                }
                ToolCallDisplayStatus::PendingPermission | ToolCallDisplayStatus::Queued
                    if path.is_none() =>
                {
                    Some("~")
                }
                ToolCallDisplayStatus::PendingPermission
                | ToolCallDisplayStatus::Queued
                | ToolCallDisplayStatus::Succeeded
                | ToolCallDisplayStatus::Failed => Some("→"),
            };
            (title, icon, TranscriptToolCallVisualStyle::Inline, false)
        }
        "fs.glob" | "glob" => (
            format!(
                "Glob \"{}\"",
                tool_summary_string(&tool_call.args_summary, &["pattern"])
                    .unwrap_or_else(|| "*".to_string()),
            ),
            Some("✱"),
            TranscriptToolCallVisualStyle::Inline,
            false,
        ),
        "fs.grep" | "grep" => (
            format!(
                "Grep \"{}\"",
                tool_summary_string(&tool_call.args_summary, &["pattern"])
                    .unwrap_or_else(|| "pattern".to_string()),
            ),
            Some("✱"),
            TranscriptToolCallVisualStyle::Inline,
            false,
        ),
        "fs.ls" | "list" => (
            completed_list_tool_title(tool_call),
            Some("→"),
            TranscriptToolCallVisualStyle::Inline,
            false,
        ),
        "shell.run" | "bash" => {
            let cmd = shell_tool_command(tool_call).unwrap_or_else(|| "Shell".to_string());
            let shell_output = shell_tool_output(tool_call);
            if shell_output.is_some() {
                if let Some(output) = shell_output {
                    push_collapsible_bash_panel_block(
                        &mut detail_blocks,
                        &cmd,
                        &output,
                        shell_tool_title_description(tool_call, session_path),
                        HARNESS_BASH_OUTPUT_LINE_CLAMP,
                        expanded,
                        TranscriptToolCallDetailTone::Primary,
                    );
                }
                (
                    "Shell".to_string(),
                    None,
                    TranscriptToolCallVisualStyle::Block,
                    false,
                )
            } else {
                (
                    cmd.clone(),
                    Some("$"),
                    TranscriptToolCallVisualStyle::Inline,
                    false,
                )
            }
        }
        "edit.hashline_apply" => {
            let path = tool_call.edit_path_display();
            let title = match tool_call.edit.as_ref().map(|edit| edit.status) {
                Some(crate::app::EditDisplayStatus::Applied) => "Patch".to_string(),
                Some(crate::app::EditDisplayStatus::Rejected)
                | Some(crate::app::EditDisplayStatus::Proposed) => path
                    .as_ref()
                    .map(|_| "Edit".to_string())
                    .unwrap_or_else(|| "Preparing edit...".to_string()),
                None => path
                    .as_ref()
                    .map(|_| "Edit".to_string())
                    .unwrap_or_else(|| "Preparing edit...".to_string()),
            };

            if let Some(edit) = &tool_call.edit {
                if edit.status == crate::app::EditDisplayStatus::Applied {
                    push_tool_call_diff_blocks(
                        &mut detail_blocks,
                        tool_call,
                        app,
                        session_path,
                        stacked_diffs,
                    );
                }

                if edit.status == crate::app::EditDisplayStatus::Rejected {
                    if let Some(reason) = edit.rejection_reason.as_deref() {
                        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                            text: reason.to_string(),
                            tone: TranscriptToolCallDetailTone::Error,
                        });
                    }
                }
            }

            let icon = if title == "Preparing edit..." {
                Some("~")
            } else {
                Some("←")
            };

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
            build_agent_spawn_tool_row(tool_call, task_row, &mut detail_blocks, animation_phase)
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
            (
                write_tool_title(tool_call),
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
        "web.fetch" => (
            format!(
                "WebFetch {}",
                tool_summary_string(&tool_call.args_summary, &["url"])
                    .unwrap_or_else(|| "url".to_string())
            ),
            Some("%"),
            TranscriptToolCallVisualStyle::Inline,
            true,
        ),
        "search.web" | "search.code" => {
            let title = if display_tool_id == "search.web" {
                web_search_provider_label(tool_call)
            } else {
                "Exa Code Search"
            };
            (
                format!(
                    "{} \"{}\"{}",
                    title,
                    tool_summary_string(&tool_call.args_summary, &["query"])
                        .unwrap_or_else(|| "query".to_string()),
                    search_result_count_suffix(tool_call, display_tool_id)
                ),
                Some(if display_tool_id == "search.web" {
                    "◈"
                } else {
                    "◇"
                }),
                TranscriptToolCallVisualStyle::Inline,
                true,
            )
        }
        "user.question" => {
            let _ = &question_answers;
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
            let generic_output = if tool_call.status == ToolCallDisplayStatus::Failed {
                error_body
                    .as_deref()
                    .or(tool_call.output_summary.as_deref())
            } else {
                tool_call.output_summary.as_deref()
            };
            if generic_output_visible && generic_output.is_some() {
                push_collapsible_output_block(
                    &mut detail_blocks,
                    generic_output.unwrap_or_default(),
                    if tool_call.status == ToolCallDisplayStatus::Failed {
                        TranscriptToolCallDetailTone::Error
                    } else {
                        TranscriptToolCallDetailTone::Primary
                    },
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

    push_failed_tool_error_block(&mut detail_blocks, tool_call);
    push_truncated_output_artifact_block(&mut detail_blocks, tool_call);

    let disclosure_state = if matches!(
        display_tool_id,
        "agent.spawn" | "task" | "todo.write" | "todowrite"
    ) || uses_generic_output_visibility
        && tool_call.status == ToolCallDisplayStatus::Succeeded
        && !generic_output_visible
    {
        None
    } else {
        tool_disclosure_state(tool_call, tool_output_expanded)
    };
    let default_subtitle = match display_tool_id {
        "shell.run" | "bash" => None,
        "fs.glob" | "glob" | "fs.grep" | "grep" => join_tool_subtitles(
            tool_in_path_description(tool_call),
            tool_match_count_description(tool_call),
        ),
        "edit.hashline_apply" => tool_call_path_metadata(tool_call.edit_path_display().as_deref())
            .map(|metadata| {
                header_path_metadata = metadata.parent.clone();
                metadata.leaf
            }),
        "fs.write" | "write" | "edit" => None,
        "background_output" => background_output_tool_subtitle(tool_call),
        "agent.spawn" | "task" => agent_spawn_subtitle(tool_call),
        "apply_patch" => None,
        _ => None,
    };

    TranscriptToolCallSection {
        tool_call_id: tool_call.tool_call_id.clone(),
        child_session_id,
        hovered_target: app.hovered_transcript_target().cloned(),
        header: TranscriptToolCallHeader {
            tool_id: if matches!(display_tool_id, "shell.run" | "bash") {
                display_tool_id.to_string()
            } else {
                tool_call.tool_id.clone()
            },
            title,
            subtitle: if matches!(display_tool_id, "shell.run" | "bash")
                && !tool_call_denied(tool_call)
            {
                default_subtitle
            } else if tool_call.status == ToolCallDisplayStatus::Failed {
                join_tool_subtitles(default_subtitle, error_subtitle)
            } else {
                default_subtitle
            },
            path_metadata: header_path_metadata,
            icon,
            status: tool_call.status,
            visual_style,
            struck_out,
            disclosure_state,
        },
        detail_blocks,
        expanded,
    }
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

fn push_applied_edit_fallback_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    edit: &crate::app::EditEntry,
) {
    let summary = edit.summary.as_deref().map(collapse_inline_whitespace);
    let diff_rel_path = edit
        .diff_rel_path
        .as_deref()
        .map(collapse_inline_whitespace);
    let text = match (summary.as_deref(), diff_rel_path.as_deref()) {
        (Some(summary), Some(diff_rel_path)) => {
            format!("{summary} · Diff preview unavailable ({diff_rel_path})")
        }
        (Some(summary), None) => format!("{summary} · Diff preview unavailable"),
        (None, Some(diff_rel_path)) => format!("Diff preview unavailable · {diff_rel_path}"),
        (None, None) => "Diff preview unavailable".to_string(),
    };
    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
        text,
        tone: TranscriptToolCallDetailTone::Secondary,
    });
}

fn push_tool_call_diff_blocks(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
    app: &AppState,
    session_path: Option<&Path>,
    stacked_diffs: bool,
) -> bool {
    let Some(session_path) = session_path else {
        if let Some(edit) = tool_call.edit.as_ref() {
            if edit.status == crate::app::EditDisplayStatus::Applied {
                push_applied_edit_fallback_block(detail_blocks, edit);
                return true;
            }
        }
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
            session_path,
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
                diff_content,
                fallback_path,
                force_stacked,
                plain_numbered,
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

        if let Some(edit) = tool_call.edit.as_ref() {
            if edit.status == crate::app::EditDisplayStatus::Applied {
                push_applied_edit_fallback_block(detail_blocks, edit);
                return true;
            }
        }
        if tool_call_has_diff_preview(tool_call) {
            detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                text: "Diff preview unavailable".to_string(),
                tone: TranscriptToolCallDetailTone::Secondary,
            });
            return true;
        }
    }

    rendered
}

fn push_apply_patch_file_sections(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
    app: &AppState,
    session_path: &Path,
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
                session_path,
                diff_rel_path,
                Some(&entry.file_path),
                stacked_diffs,
                false,
                false,
            );
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
    session_path: &Path,
    diff_rel_path: &str,
    fallback_path: Option<&str>,
    force_stacked: bool,
    plain_numbered: bool,
    show_file_header: bool,
) -> bool {
    let Ok(diff_content) = std::fs::read_to_string(session_path.join(diff_rel_path)) else {
        return false;
    };
    detail_blocks.push(TranscriptToolCallDetailBlock::StructuredDiff {
        diff_content,
        fallback_path: fallback_path.map(str::to_string),
        force_stacked,
        plain_numbered,
        show_file_header,
    });
    true
}

pub(super) fn build_agent_spawn_tool_row(
    tool_call: &crate::app::ToolCallEntry,
    task_row: Option<&crate::app::OrchestrationTaskRow>,
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    animation_phase: usize,
) -> (
    String,
    Option<&'static str>,
    TranscriptToolCallVisualStyle,
    bool,
) {
    let description = agent_spawn_description(tool_call).or_else(|| {
        task_row
            .and_then(|row| row.result_summary.as_deref())
            .map(collapse_inline_whitespace)
            .filter(|value| !value.is_empty())
    });
    let title = agent_spawn_title(tool_call, description);
    let background_child_running = tool_call.status == ToolCallDisplayStatus::Succeeded
        && agent_spawn_is_background(tool_call)
        && task_row.is_some_and(|row| !row.state.is_terminal());
    let active_task_row =
        tool_call.status == ToolCallDisplayStatus::Running || background_child_running;
    if matches!(
        tool_call.status,
        ToolCallDisplayStatus::Succeeded | ToolCallDisplayStatus::Running
    ) {
        if active_task_row {
            push_running_subagent_detail(task_row, detail_blocks);
        } else if tool_call.status == ToolCallDisplayStatus::Succeeded {
            if let Some(row) = task_row {
                push_completed_subagent_detail(row, detail_blocks);
            } else if let Some(result) = task_result_markdown(tool_call) {
                detail_blocks.push(TranscriptToolCallDetailBlock::Markdown { text: result });
            }
        }
    }
    let icon = match tool_call.status {
        ToolCallDisplayStatus::Failed => Some("│"),
        ToolCallDisplayStatus::Succeeded => Some("✓"),
        ToolCallDisplayStatus::Running
        | ToolCallDisplayStatus::PendingPermission
        | ToolCallDisplayStatus::Queued => Some("│"),
    };
    let icon = if active_task_row {
        Some(transcript_streaming_spinner_frame(animation_phase))
    } else {
        icon
    };
    (
        title,
        icon,
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
        text: preview.output,
        tone,
    });
    if let Some(hint) = preview.expand_hint {
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: hint.to_string(),
            tone: TranscriptToolCallDetailTone::Secondary,
        });
    }
}

fn push_collapsible_bash_panel_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    command: &str,
    output: &str,
    description: Option<String>,
    max_lines: usize,
    expanded: bool,
    tone: TranscriptToolCallDetailTone,
) {
    let preview = collapsible_bash_panel_preview(output, max_lines, expanded);

    detail_blocks.push(TranscriptToolCallDetailBlock::BashPanel {
        command: command.to_string(),
        output: preview.output,
        description,
        expand_hint: preview.expand_hint.map(str::to_string),
        tone,
    });
}

fn completed_list_tool_title(tool_call: &crate::app::ToolCallEntry) -> String {
    if tool_call.status != ToolCallDisplayStatus::Succeeded {
        return tool_summary_string(&tool_call.args_summary, &["path"])
            .filter(|path| !path.is_empty())
            .map(|path| format!("List {path}"))
            .unwrap_or_else(|| "List".to_string());
    }
    let count = tool_entry_count(tool_call).unwrap_or(1);
    let noun = if count == 1 { "dir" } else { "dirs" };
    format!("Listed {count} {noun}")
}

fn completed_read_tool_title(tool_call: &crate::app::ToolCallEntry, _path: Option<&str>) -> String {
    // Waiting-state packing: completed reads use count form ("Read 1 file"),
    // not path form. Running/failed keep path via the caller match arm.
    let count = tool_file_count(tool_call).unwrap_or(1);
    let noun = if count == 1 { "file" } else { "files" };
    format!("Read {count} {noun}")
}

fn tool_entry_count(tool_call: &crate::app::ToolCallEntry) -> Option<u64> {
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
    tool_call.output_summary.as_deref().and_then(|summary| {
        let lines = summary
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        (lines > 0).then_some(lines as u64)
    })
}

fn tool_file_count(tool_call: &crate::app::ToolCallEntry) -> Option<u64> {
    tool_call
        .output_json
        .as_ref()
        .and_then(|value| {
            value
                .get("file_count")
                .or_else(|| value.get("files_read"))
                .or_else(|| value.get("count"))
        })
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            tool_call.output_summary.as_deref().and_then(|summary| {
                let lower = summary.to_ascii_lowercase();
                if lower.contains("file") {
                    Some(1)
                } else {
                    None
                }
            })
        })
}
