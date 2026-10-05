use super::*;
use TranscriptToolCallVisualStyle::{Block, Inline, TaskInline};

pub(super) fn populate(
    row: &mut TranscriptToolCallSection,
    tool: &ToolCallEntry,
    app: &AppState,
    visible: bool,
    stacked: bool,
    session_path: Option<&Path>,
) -> bool {
    let id = tool.effective_tool_id();
    let mut generic = !matches!(
        id,
        "edit.hashline_apply"
            | "edit.hashline_scan"
            | "spawn_subagent"
            | "agent.spawn"
            | "task"
            | "background_output"
            | "background_cancel"
            | "plan_enter"
            | "plan_exit"
            | "skill"
            | "skill.load"
            | "fs.write"
            | "write"
            | "edit"
            | "apply_patch"
            | "user.question"
            | "question"
            | "eval"
    );
    let header = &mut row.header;
    let blocks = &mut row.detail_blocks;
    if !generic {
        header.visual_style = Inline;
    }
    (header.title, header.icon) = match id {
        "fs.read" | "read" => {
            let path = tool_path_display(tool);
            let title = read_tool_row_header(tool, app, path.as_deref());
            header.path_metadata = if title.0 == "Skill" {
                path.as_deref()
                    .and_then(|path| Path::new(path).parent()?.file_name()?.to_str())
                    .map(str::to_owned)
            } else {
                path
            };
            if visible && tool.status != ToolCallDisplayStatus::Failed {
                blocks.extend(read_output_block(tool));
            }
            title
        }
        "fs.glob" | "glob" | "fs.grep" | "grep" => {
            header.path_metadata = tool_path_display(tool).filter(|path| path != ".");
            (
                crate::ui::ui_tool_titles_harness::local_search_tool_title(tool),
                Some("✱"),
            )
        }
        "fs.ls" | "list" => {
            header.path_metadata = tool_path_display(tool);
            ("List".into(), Some("→"))
        }
        "shell.run" | "bash" => {
            let command = shell_tool_command(tool).unwrap_or_else(|| "Shell".into());
            let description = shell_tool_title_description(tool, session_path);
            let title = format!("Run {}", description.as_deref().unwrap_or(&command));
            let output = if tool.status == ToolCallDisplayStatus::Failed {
                shell_tool_structured_output(tool.output_json.as_ref())
            } else {
                shell_tool_output(tool)
            }
            .or_else(|| row.expanded.then(String::new));
            generic = output.is_some();
            if let Some(output) = output {
                // Terminal interpretation handles SGR, CR overwrite and redaction.
                blocks.push(TranscriptToolCallDetailBlock::BashPanel {
                    command,
                    output,
                    description,
                });
            }
            header.visual_style = Block;
            (title, None)
        }
        "edit.hashline_apply" => {
            header.visual_style = Block;
            hashline_tool_row_header(tool, app, blocks, session_path, stacked)
        }
        "edit.hashline_scan" => (
            format!(
                "Scan {}",
                tool_path_display(tool).unwrap_or_else(|| "file".into())
            ),
            Some("→"),
        ),
        "spawn_subagent" | "agent.spawn" | "task" => {
            header.visual_style = TaskInline;
            (String::new(), None)
        }
        "background_output" => (background_output_tool_title(tool), Some("↻")),
        "background_cancel" => (background_cancel_tool_title(tool), Some("✕")),
        "plan_enter" => (plan_enter_tool_title(tool), Some("⊕")),
        "plan_exit" => (plan_exit_tool_title(tool), Some("⊖")),
        "invalid" => (invalid_tool_title(tool), Some("!")),
        "session_list" => (session_tool_title(tool, "List"), Some("≡")),
        "session_read" => (session_tool_title(tool, "Read"), Some("→")),
        "session_search" => (session_tool_title(tool, "Search"), Some("✱")),
        "session_info" => (session_tool_title(tool, "Inspect"), Some("ⓘ")),
        "ast_grep_search" => (ast_grep_tool_title(tool, "AST Search"), Some("✱")),
        "ast_grep_replace" | "lsp.rename" | "fs.write" | "write" | "edit" | "apply_patch" => {
            if diffs::push_tool_call_diff_blocks(blocks, tool, app, session_path, stacked) {
                header.visual_style = Block;
            }
            match id {
                "ast_grep_replace" => (ast_grep_tool_title(tool, "AST Replace"), Some("←")),
                "lsp.rename" => (lsp_tool_title(tool), Some("→")),
                "fs.write" | "write" => (write_tool_title(tool), Some("←")),
                "edit" => (edit_tool_title(tool), Some("←")),
                _ => (apply_patch_tool_title(tool), Some("%")),
            }
        }
        "lsp" | "code.lsp" => (lsp_tool_title(tool), Some("→")),
        "skill" | "skill.load" => (skill_tool_title(tool), Some("→")),
        "web.fetch" | "webfetch" => {
            header.visual_style = Inline;
            (
                format!(
                    "Fetch {}",
                    tool_summary_string(&tool.args_summary, &["url"])
                        .unwrap_or_else(|| "url".into())
                ),
                Some("%"),
            )
        }
        "search.web" | "websearch" | "search.code" => {
            header.visual_style = Inline;
            let glyphs = &app.theme().live_shell.transcript_glyphs;
            (
                search_tool_title(tool, row.expanded),
                Some(if matches!(id, "search.web" | "websearch") {
                    glyphs.group_marker
                } else {
                    glyphs.thought_marker
                }),
            )
        }
        "user.question" | "question" => {
            let answers = resolved_question_answer_items(tool);
            if !answers.is_empty() {
                blocks.push(TranscriptToolCallDetailBlock::Message {
                    text: answers
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
            (question_tool_title(tool, &answers), Some("→"))
        }
        "eval" => {
            header.visual_style = Block;
            let action = tool_summary_string(&tool.args_summary, &["action"])
                .unwrap_or_else(|| "run".into());
            let title = tool_summary_string(&tool.args_summary, &["summary", "title"])
                .unwrap_or_else(|| {
                    match action.as_str() {
                        "peek" => "Inspect eval cell",
                        "stop" => "Stop eval cell",
                        "list" => "List eval cells",
                        _ => "code",
                    }
                    .into()
                });
            blocks.push(TranscriptToolCallDetailBlock::EvalPanel {
                code: serde_json::from_str::<serde_json::Value>(&tool.args_summary)
                    .ok()
                    .and_then(|v| v["code"].as_str().map(str::to_owned))
                    .unwrap_or_default(),
                language: tool_summary_string(&tool.args_summary, &["language"])
                    .unwrap_or_default(),
                output: tool
                    .output_json
                    .as_ref()
                    .filter(|_| action == "run")
                    .and_then(|data| data["cells"][0]["output"].as_str())
                    .map(str::to_owned)
                    .or_else(|| tool.output_summary.clone())
                    .unwrap_or_default(),
                failed: tool.status == ToolCallDisplayStatus::Failed,
            });
            if action == "run" {
                eval_notices(blocks, tool, row.expanded);
            }
            (
                if action == "run" {
                    format!("Eval {title}")
                } else {
                    title
                },
                None,
            )
        }
        _ if is_mcp_tool_id(id) => (mcp_tool_title(tool, id), Some("⚙")),
        _ => {
            let output_visible = visible
                && (tool.status == ToolCallDisplayStatus::Failed || tool.output_summary.is_some());
            (
                generic_tool_title(tool, id),
                (!output_visible).then_some("⚙"),
            )
        }
    };
    generic
}

fn eval_notices(
    blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool: &ToolCallEntry,
    expanded: bool,
) {
    let Some(data) = &tool.output_json else {
        return;
    };
    let mut notices = Vec::new();
    if data["truncated"] == true {
        notices.push("Output truncated · Enter for details".to_owned());
        if expanded {
            if let Some(notice) = data["notice"].as_str() {
                notices.push(notice.to_owned());
            }
        }
    }
    if let Some(notice) = data["memory_notice"].as_str() {
        notices.push(notice.to_owned());
    }
    if !notices.is_empty() {
        blocks.push(TranscriptToolCallDetailBlock::Message {
            text: notices.join("\n"),
            tone: TranscriptToolCallDetailTone::Primary,
        });
    }
}

pub(super) fn subtitle(
    header: &mut TranscriptToolCallHeader,
    tool: &ToolCallEntry,
    app: &AppState,
    expanded: bool,
    session_path: Option<&Path>,
) {
    let id = tool.effective_tool_id();
    let subtitle = match id {
        "eval" => {
            let language =
                tool_summary_string(&tool.args_summary, &["language"]).unwrap_or_default();
            let language = match language.as_str() {
                "js" => "JavaScript",
                "py" => "Python",
                "rb" => "Ruby",
                "jl" => "Julia",
                _ => "Eval",
            };
            let data = tool.output_json.as_ref();
            let state = data
                .filter(|v| v["detached"] == true)
                .map(|_| "detached")
                .or_else(|| data.and_then(|v| v["cells"][0]["status"].as_str()))
                .unwrap_or(match tool.status {
                    ToolCallDisplayStatus::Running => "running",
                    ToolCallDisplayStatus::Queued => "queued",
                    ToolCallDisplayStatus::PendingPermission => "approval needed",
                    ToolCallDisplayStatus::Succeeded => "complete",
                    ToolCallDisplayStatus::Failed => "failed",
                });
            let mut parts = vec![language.to_owned()];
            if state != "complete" && tool.status != ToolCallDisplayStatus::Failed {
                parts.push(state.to_owned());
            }
            if let Some(count) = data
                .and_then(|v| v["toolCallCount"].as_u64())
                .filter(|n| *n > 0)
            {
                parts.push(format!(
                    "{count} tool call{}",
                    if count == 1 { "" } else { "s" }
                ));
            }
            if let Some(duration) = data
                .and_then(|v| v["durationMs"].as_u64())
                .filter(|_| !matches!(state, "running" | "queued" | "detached"))
            {
                parts.push(format_duration_ms(duration));
            }
            Some(parts.join(" · "))
        }
        "shell.run" | "bash" => {
            crate::ui::ui_transcript_bash::shell_tool_workdir_display(tool, session_path)
                .map(|path| format!("in {path}"))
        }
        "fs.read" | "read" => read_tool_subtitle(tool),
        "fs.ls" | "list" => (tool.status == ToolCallDisplayStatus::Succeeded)
            .then(|| tool_entry_count(tool))
            .flatten()
            .map(|count| format!("({count} {})", if count == 1 { "entry" } else { "entries" })),
        "fs.glob" | "glob" | "fs.grep" | "grep" => tool_match_count_description(tool),
        "search.web" | "websearch" if !expanded => {
            let sources = crate::ui::ui_recorded_tool_output::web_sources(tool);
            let count = crate::ui::ui_recorded_tool_output::source_domains(&sources).len();
            (count > 0).then(|| format!("({count} site{})", if count == 1 { "" } else { "s" }))
        }
        "edit.hashline_apply" | "fs.write" | "write" | "edit" => {
            header.path_metadata = tool.edit_path_display().or_else(|| tool_path_display(tool));
            if let Some(action) = header
                .path_metadata
                .as_ref()
                .and_then(|path| header.title.strip_suffix(&format!(" {path}")))
            {
                header.title = action.to_owned();
            }
            None
        }
        "background_output" => background_output_tool_subtitle(tool),
        "spawn_subagent" | "agent.spawn" | "task" => Some(agent_spawn_subtitle(tool, app)),
        _ => None,
    };
    header.subtitle = if tool.status == ToolCallDisplayStatus::Failed
        && !tool_error_hidden_inline(tool)
        && !is_mcp_tool_id(id)
        && !matches!(
            id,
            "user.question"
                | "question"
                | "bash"
                | "shell.run"
                | "edit"
                | "write"
                | "fs.write"
                | "edit.hashline_apply"
        ) {
        join_tool_subtitles(subtitle, tool_error_subtitle(tool))
    } else {
        subtitle
    };
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

fn read_tool_subtitle(tool: &crate::app::ToolCallEntry) -> Option<String> {
    if let Some(mime) = crate::ui::ui_tool_metadata::read_media_mime(tool) {
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
                diffs::push_tool_call_diff_blocks(
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
