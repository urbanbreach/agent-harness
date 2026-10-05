use super::*;

pub(in crate::ui::ui_transcript) fn set_diff_highlight_phase(
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

pub(super) fn push_tool_call_diff_blocks(
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

        if tool_call.effective_tool_id() == "apply_patch"
            && let Some(rows) = tool_call_apply_patch_file_rows(tool_call)
        {
            for row in rows {
                detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                    text: row,
                    tone: TranscriptToolCallDetailTone::Secondary,
                });
            }
            return true;
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

pub(super) fn attach_recorded_diff_sources(
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
