use std::collections::BTreeSet;

use crate::app::ToolCallEntry;
use crate::text::non_empty_trimmed;

#[derive(Debug, Clone)]
pub(super) struct ApplyPatchFileRenderEntry {
    pub(super) file_path: String,
    pub(super) diff_rel_path: Option<String>,
}

pub(super) fn tool_call_has_diff_preview(tool_call: &ToolCallEntry) -> bool {
    !tool_call_diff_artifacts(tool_call).is_empty()
        || tool_call_inline_diff_block(tool_call).is_some()
}

pub(super) fn tool_call_has_preview_content(tool_call: &ToolCallEntry) -> bool {
    tool_call_has_diff_preview(tool_call)
        || tool_call_apply_patch_file_rows(tool_call).is_some_and(|rows| !rows.is_empty())
}

pub(super) fn apply_patch_tool_title(tool_call: &ToolCallEntry) -> String {
    match apply_patch_file_count(tool_call) {
        Some(0) | None => "Patch".to_string(),
        Some(count) => format!("Patch {count} file{}", if count == 1 { "" } else { "s" }),
    }
}

pub(super) fn collect_apply_patch_file_render_entries(
    tool_call: &ToolCallEntry,
) -> Vec<ApplyPatchFileRenderEntry> {
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();

    if let Some(edits) = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("edits"))
        .and_then(serde_json::Value::as_array)
    {
        for edit in edits {
            let Some(file_path) = edit
                .get("path")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(str::to_string)
            else {
                continue;
            };
            if !seen.insert(file_path.clone()) {
                continue;
            }
            let diff_rel_path = edit
                .get("diff_rel_path")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(str::to_string);
            entries.push(ApplyPatchFileRenderEntry {
                file_path,
                diff_rel_path,
            });
        }
    }

    if entries.is_empty() {
        let diff_artifacts = tool_call_diff_artifacts(tool_call);
        let diff_paths = diff_artifacts
            .into_iter()
            .filter_map(|(diff_rel_path, fallback_path)| {
                fallback_path.map(|file_path| ApplyPatchFileRenderEntry {
                    file_path,
                    diff_rel_path: Some(diff_rel_path),
                })
            })
            .collect::<Vec<_>>();
        for entry in diff_paths {
            if seen.insert(entry.file_path.clone()) {
                entries.push(entry);
            }
        }
    }

    if entries.is_empty() {
        let diff_rel_paths = tool_call_diff_artifacts(tool_call)
            .into_iter()
            .map(|(diff_rel_path, _)| diff_rel_path)
            .collect::<Vec<_>>();
        if let Some(rows) = tool_call_apply_patch_file_rows(tool_call) {
            for (index, row) in rows.into_iter().enumerate() {
                let Some(file_path) = normalize_apply_patch_file_row(&row) else {
                    continue;
                };
                if !seen.insert(file_path.clone()) {
                    continue;
                }
                entries.push(ApplyPatchFileRenderEntry {
                    file_path,
                    diff_rel_path: diff_rel_paths.get(index).cloned(),
                });
            }
        }
    }

    entries
}

fn apply_patch_file_count(tool_call: &ToolCallEntry) -> Option<usize> {
    tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("files"))
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .or_else(|| {
            let count = collect_apply_patch_file_render_entries(tool_call).len();
            (count > 0).then_some(count)
        })
}

fn normalize_apply_patch_file_row(row: &str) -> Option<String> {
    let trimmed = row.trim();
    if trimmed.is_empty() {
        return None;
    }

    let remainder = trimmed
        .split_once(' ')
        .map(|(_, rest)| rest.trim())
        .filter(|rest| !rest.is_empty())
        .unwrap_or(trimmed);
    Some(remainder.to_string())
}

pub(super) fn tool_call_inline_diff_block(
    tool_call: &ToolCallEntry,
) -> Option<(String, Option<String>)> {
    let args = serde_json::from_str::<serde_json::Value>(&tool_call.args_summary).ok()?;
    let object = args.as_object()?;

    match tool_call.effective_tool_id() {
        "edit" => {
            let path = object
                .get("filePath")
                .or_else(|| object.get("path"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|path| !path.is_empty())?
                .to_string();
            let before = object
                .get("oldString")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let after = object
                .get("newString")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            Some((basic_unified_diff(&path, before, after), Some(path)))
        }
        "fs.write" => {
            let path = object
                .get("filePath")
                .or_else(|| object.get("path"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|path| !path.is_empty())?
                .to_string();
            let after = object
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            Some((basic_unified_diff(&path, "", after), Some(path)))
        }
        _ => None,
    }
}

pub(super) fn tool_call_apply_patch_file_rows(tool_call: &ToolCallEntry) -> Option<Vec<String>> {
    let files = tool_call
        .output_json
        .as_ref()?
        .get("files")
        .and_then(serde_json::Value::as_array)?;
    let rows = files
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    (!rows.is_empty()).then_some(rows)
}

fn basic_unified_diff(path: &str, before: &str, after: &str) -> String {
    let before_lines = before.lines().collect::<Vec<_>>();
    let after_lines = after.lines().collect::<Vec<_>>();
    let before_count = before_lines.len();
    let after_count = after_lines.len();
    let before_start = if before_count == 0 { 0 } else { 1 };
    let after_start = if after_count == 0 { 0 } else { 1 };

    let mut diff = String::new();
    diff.push_str(&format!("--- {path}\n"));
    diff.push_str(&format!("+++ {path}\n"));
    diff.push_str(&format!(
        "@@ -{before_start},{before_count} +{after_start},{after_count} @@\n"
    ));
    for line in before_lines {
        diff.push_str(&format!("-{line}\n"));
    }
    for line in after_lines {
        diff.push_str(&format!("+{line}\n"));
    }
    diff
}

pub(super) fn tool_call_diff_artifacts(tool_call: &ToolCallEntry) -> Vec<(String, Option<String>)> {
    let mut seen = BTreeSet::new();
    let mut diffs = Vec::new();

    if let Some(edit) = tool_call.edit.as_ref() {
        if let Some(diff_rel_path) = edit.diff_rel_path.as_deref() {
            if seen.insert(diff_rel_path.to_string()) {
                diffs.push((diff_rel_path.to_string(), Some(edit.path.clone())));
            }
        }
    }

    if let Some(edits) = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("edits"))
        .and_then(serde_json::Value::as_array)
    {
        for edit in edits {
            let Some(diff_rel_path) = edit
                .get("diff_rel_path")
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let diff_rel_path = diff_rel_path.trim();
            if diff_rel_path.is_empty() || !seen.insert(diff_rel_path.to_string()) {
                continue;
            }
            let fallback_path = edit
                .get("path")
                .and_then(serde_json::Value::as_str)
                .and_then(non_empty_trimmed)
                .map(str::to_string);
            diffs.push((diff_rel_path.to_string(), fallback_path));
        }
    }

    let default_fallback_path = tool_call.edit_path_display();
    for artifact in &tool_call.artifact_refs {
        if !artifact.path.ends_with(".diff") || !seen.insert(artifact.path.clone()) {
            continue;
        }
        diffs.push((artifact.path.clone(), default_fallback_path.clone()));
    }

    diffs
}
