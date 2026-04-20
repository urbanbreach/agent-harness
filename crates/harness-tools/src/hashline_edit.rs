use async_trait::async_trait;
use harness_core::edit::hashline::{
    compute_line_hash, HashlineOp, HashlinePatch, HashlineWorkspaceOp, LineAnchor,
};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};

use crate::hashline_apply::{
    apply_hashline_patch_to_workspace, apply_hashline_workspace_op_to_workspace,
    resolve_workspace_target_path,
};
use crate::workspace_edit::recent_hashline_anchors;

pub(crate) struct HashlineEditTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HashlineEditArgs {
    #[serde(rename = "filePath", alias = "file", alias = "path")]
    file_path: String,
    #[serde(default, rename = "editId", alias = "edit_id")]
    edit_id: Option<String>,
    #[serde(default)]
    edits: Vec<RawHashlineEdit>,
    #[serde(default)]
    delete: bool,
    #[serde(default)]
    rename: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHashlineEdit {
    #[serde(default)]
    op: Option<String>,
    #[serde(default, alias = "start")]
    pos: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present_value")]
    lines: MaybePresent<Value>,
}

#[derive(Debug, Default)]
enum MaybePresent<T> {
    #[default]
    Missing,
    Present(T),
}

impl<T> MaybePresent<T> {
    fn as_ref(&self) -> Option<&T> {
        match self {
            Self::Missing => None,
            Self::Present(value) => Some(value),
        }
    }
}

fn deserialize_present_value<'de, D>(deserializer: D) -> Result<MaybePresent<Value>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(MaybePresent::Present(Value::deserialize(deserializer)?))
}

enum TranslatedHashlinePlan {
    Patch(HashlinePatch),
    RewriteFile { content: String },
}

#[async_trait]
impl Tool for HashlineEditTool {
    fn id(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Edit files using LINE#HASH anchors for precise, safe modifications. Use pos/end anchors for each edit, read the file first, batch related edits in one call, and re-read before editing the same file again."
    }

    fn parameters_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["filePath", "edits"],
            "properties": {
                "filePath": { "type": "string" },
                "editId": { "type": "string" },
                "delete": { "type": "boolean", "default": false },
                "rename": { "type": "string" },
                "edits": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["op"],
                        "properties": {
                            "op": {
                                "type": "string",
                                "enum": ["replace", "append", "prepend"]
                            },
                            "pos": { "type": "string" },
                            "end": { "type": "string" },
                            "lines": {
                                "oneOf": [
                                    { "type": "string" },
                                    {
                                        "type": "array",
                                        "items": { "type": "string" }
                                    },
                                    { "type": "null" }
                                ]
                            }
                        }
                    }
                }
            }
        })
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: HashlineEditArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        execute_hashline_edit(&ctx, args)
    }
}

fn execute_hashline_edit(
    ctx: &ToolContext,
    args: HashlineEditArgs,
) -> Result<ToolResult, ToolError> {
    let edit_id = args
        .edit_id
        .clone()
        .unwrap_or_else(|| format!("edit-{}", ctx.tool_call_id));

    if args.delete {
        if !args.edits.is_empty() {
            return Err(ToolError::InvalidArguments(
                "delete=true requires edits=[]".to_string(),
            ));
        }
        if args.rename.is_some() {
            return Err(ToolError::InvalidArguments(
                "delete=true cannot be combined with rename".to_string(),
            ));
        }
        return apply_hashline_workspace_op_to_workspace(
            ctx,
            HashlineWorkspaceOp::DeleteFile {
                edit_id,
                path: args.file_path,
            },
        );
    }

    let resolved_path = resolve_workspace_target_path(ctx, &args.file_path)?;
    let source = match std::fs::read_to_string(&resolved_path) {
        Ok(source) => Some(source),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            return Err(ToolError::Execution(format!(
                "failed to read target file for hashline edit: {err}"
            )))
        }
    };

    let translated = translate_hashline_edits(
        &args.file_path,
        &edit_id,
        source.as_deref(),
        &args.edits,
        recent_hashline_anchors(ctx, &resolved_path).as_deref(),
    )?;
    let mut result = match translated {
        TranslatedHashlinePlan::Patch(patch) => apply_hashline_patch_to_workspace(ctx, patch)?,
        TranslatedHashlinePlan::RewriteFile { content } => {
            apply_hashline_workspace_op_to_workspace(
                ctx,
                HashlineWorkspaceOp::RewriteFile {
                    edit_id: edit_id.clone(),
                    path: args.file_path.clone(),
                    content,
                },
            )?
        }
    };
    result.display_text = "Edit applied successfully.".to_string();

    if let Some(rename) = args.rename {
        let move_result = apply_hashline_workspace_op_to_workspace(
            ctx,
            HashlineWorkspaceOp::MoveFile {
                edit_id,
                from_path: args.file_path,
                to_path: rename,
            },
        )?;
        result.display_text = move_result.display_text;
        result.artifacts.extend(move_result.artifacts);
        result.structured_json = move_result.structured_json;
    }

    Ok(result)
}

fn translate_hashline_edits(
    path: &str,
    edit_id: &str,
    source: Option<&str>,
    edits: &[RawHashlineEdit],
    recent_anchors: Option<&[LineAnchor]>,
) -> Result<TranslatedHashlinePlan, ToolError> {
    if edits.is_empty() {
        return Err(ToolError::InvalidArguments(
            "edits must contain at least one operation".to_string(),
        ));
    }

    let Some(source) = source else {
        return translate_hashline_edits_for_missing_file(path, edit_id, edits);
    };

    let source_lines = split_source_lines(source);
    let mut ops = Vec::with_capacity(edits.len());
    for (index, edit) in edits.iter().enumerate() {
        ops.push(translate_hashline_edit(
            index,
            edit,
            &source_lines,
            recent_anchors,
        )?);
    }

    Ok(TranslatedHashlinePlan::Patch(HashlinePatch {
        edit_id: edit_id.to_string(),
        path: path.to_string(),
        ops,
    }))
}

fn translate_hashline_edits_for_missing_file(
    path: &str,
    _edit_id: &str,
    edits: &[RawHashlineEdit],
) -> Result<TranslatedHashlinePlan, ToolError> {
    let mut lines = Vec::new();
    for (index, edit) in edits.iter().enumerate() {
        let op = normalize_edit_op(index, edit)?;
        if edit.pos.is_some() || edit.end.is_some() {
            return Err(ToolError::Execution(format!(
                "file {path} does not exist; only anchorless append/prepend edits can create new files"
            )));
        }
        let next_lines = normalize_edit_lines(edit, index)?;
        match op.as_str() {
            "append" => lines.extend(next_lines),
            "prepend" => {
                let mut combined = next_lines;
                combined.extend(lines);
                lines = combined;
            }
            "replace" => {
                return Err(ToolError::Execution(format!(
                    "file {path} does not exist; replace requires an existing LINE#HASH anchor"
                )));
            }
            _ => unreachable!(),
        }
    }

    Ok(TranslatedHashlinePlan::RewriteFile {
        content: join_lines_with_trailing_newline(&lines),
    })
}

fn translate_hashline_edit(
    index: usize,
    edit: &RawHashlineEdit,
    source_lines: &[String],
    recent_anchors: Option<&[LineAnchor]>,
) -> Result<HashlineOp, ToolError> {
    let op = normalize_edit_op(index, edit)?;
    match op.as_str() {
        "replace" => translate_replace_edit(index, edit, source_lines, recent_anchors),
        "append" => translate_append_edit(index, edit, source_lines, recent_anchors),
        "prepend" => translate_prepend_edit(index, edit, source_lines, recent_anchors),
        _ => unreachable!(),
    }
}

fn translate_replace_edit(
    index: usize,
    edit: &RawHashlineEdit,
    source_lines: &[String],
    recent_anchors: Option<&[LineAnchor]>,
) -> Result<HashlineOp, ToolError> {
    let start = parse_anchor_reference(
        edit.pos.as_deref().or(edit.end.as_deref()),
        index,
        "replace",
        source_lines,
        recent_anchors,
    )?;
    let end = match edit.end.as_deref() {
        Some(end) => {
            parse_anchor_reference(Some(end), index, "replace", source_lines, recent_anchors)?
        }
        None => start.clone(),
    };
    let expected = anchors_for_range(source_lines, &start, &end, index)?;
    let lines = normalize_edit_lines(edit, index)?;
    if lines.is_empty() {
        Ok(HashlineOp::Delete { expected })
    } else {
        Ok(HashlineOp::Replace { expected, lines })
    }
}

fn translate_append_edit(
    index: usize,
    edit: &RawHashlineEdit,
    source_lines: &[String],
    recent_anchors: Option<&[LineAnchor]>,
) -> Result<HashlineOp, ToolError> {
    let lines = normalize_edit_lines(edit, index)?;
    if lines.is_empty() {
        return Err(ToolError::InvalidArguments(format!(
            "edit {index}: append requires non-empty lines"
        )));
    }
    let anchor = match edit.pos.as_deref().or(edit.end.as_deref()) {
        Some(anchor) => {
            parse_anchor_reference(Some(anchor), index, "append", source_lines, recent_anchors)?
        }
        None => anchor_for_line(source_lines, source_lines.len() as u32, index, "append")?,
    };
    Ok(HashlineOp::InsertAfter { anchor, lines })
}

fn translate_prepend_edit(
    index: usize,
    edit: &RawHashlineEdit,
    source_lines: &[String],
    recent_anchors: Option<&[LineAnchor]>,
) -> Result<HashlineOp, ToolError> {
    let lines = normalize_edit_lines(edit, index)?;
    if lines.is_empty() {
        return Err(ToolError::InvalidArguments(format!(
            "edit {index}: prepend requires non-empty lines"
        )));
    }
    let anchor = match edit.pos.as_deref().or(edit.end.as_deref()) {
        Some(anchor) => {
            parse_anchor_reference(Some(anchor), index, "prepend", source_lines, recent_anchors)?
        }
        None => anchor_for_line(source_lines, 1, index, "prepend")?,
    };
    Ok(HashlineOp::InsertBefore { anchor, lines })
}

fn normalize_edit_op(index: usize, edit: &RawHashlineEdit) -> Result<String, ToolError> {
    match edit.op.as_deref().map(str::trim) {
        Some("replace") | Some("append") | Some("prepend") => {
            Ok(edit.op.as_deref().unwrap().trim().to_string())
        }
        Some(other) => Err(ToolError::InvalidArguments(format!(
            "edit {index}: unsupported op \"{other}\". Use replace, append, or prepend."
        ))),
        None => infer_missing_edit_op(index, edit),
    }
}

fn infer_missing_edit_op(index: usize, edit: &RawHashlineEdit) -> Result<String, ToolError> {
    let has_anchor = edit.pos.is_some() || edit.end.is_some();
    match edit.lines.as_ref() {
        Some(Value::Null) => {
            if has_anchor {
                Ok("replace".to_string())
            } else {
                Err(missing_op_error(index, MissingOpReason::AnchorlessDelete))
            }
        }
        Some(Value::Array(lines)) if lines.is_empty() => {
            if has_anchor {
                Ok("replace".to_string())
            } else {
                Err(missing_op_error(index, MissingOpReason::AnchorlessDelete))
            }
        }
        Some(_) if has_anchor => Err(missing_op_error(index, MissingOpReason::AnchoredEdit)),
        Some(_) => Err(missing_op_error(index, MissingOpReason::AnchorlessEdit)),
        None => Err(missing_op_error(index, MissingOpReason::MissingLines)),
    }
}

enum MissingOpReason {
    AnchoredEdit,
    AnchorlessDelete,
    AnchorlessEdit,
    MissingLines,
}

fn missing_op_error(index: usize, reason: MissingOpReason) -> ToolError {
    let detail = match reason {
        MissingOpReason::AnchoredEdit => {
            "Anchored edits are ambiguous without op because replace, append, and prepend can all use pos/end anchors."
        }
        MissingOpReason::AnchorlessDelete => {
            "Deleting without anchors is unsupported because append/prepend without anchors target BOF/EOF instead of removing content."
        }
        MissingOpReason::AnchorlessEdit => {
            "Anchorless edits are ambiguous without op because append inserts at EOF and prepend inserts at BOF."
        }
        MissingOpReason::MissingLines => "lines is also required when op is omitted.",
    };

    ToolError::InvalidArguments(format!(
        "edit {index}: missing op. Use replace for anchored rewrite/delete, append to insert after an anchor or EOF, or prepend to insert before an anchor or BOF. {detail}"
    ))
}

fn normalize_edit_lines(edit: &RawHashlineEdit, index: usize) -> Result<Vec<String>, ToolError> {
    match edit.lines.as_ref() {
        Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(line)) => Ok(split_multiline_argument(line)),
        Some(Value::Array(lines)) => lines
            .iter()
            .map(|line| match line {
                Value::String(line) => Ok(strip_model_prefixes(line)),
                _ => Err(ToolError::InvalidArguments(format!(
                    "edit {index}: lines arrays must contain only strings"
                ))),
            })
            .collect(),
        Some(_) => Err(ToolError::InvalidArguments(format!(
            "edit {index}: lines must be a string, string[], or null"
        ))),
        None => Err(ToolError::InvalidArguments(format!(
            "edit {index}: lines is required"
        ))),
    }
}

fn split_multiline_argument(text: &str) -> Vec<String> {
    text.split('\n').map(strip_model_prefixes).collect()
}

fn strip_model_prefixes(line: &str) -> String {
    let trimmed = line.trim_start_matches(">>>").trim_start();
    let without_hashline = match trimmed.split_once('|') {
        Some((prefix, rest))
            if prefix.contains('#')
                && prefix
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '#')) =>
        {
            rest
        }
        _ => trimmed,
    };
    without_hashline
        .strip_prefix('+')
        .or_else(|| without_hashline.strip_prefix('-'))
        .unwrap_or(without_hashline)
        .to_string()
}

fn parse_anchor_reference(
    value: Option<&str>,
    index: usize,
    op: &str,
    source_lines: &[String],
    recent_anchors: Option<&[LineAnchor]>,
) -> Result<LineAnchor, ToolError> {
    let value = value.ok_or_else(|| {
        ToolError::InvalidArguments(format!(
            "edit {index}: {op} requires at least one LINE#HASH anchor (pos or end)"
        ))
    })?;
    let token = value
        .trim()
        .trim_start_matches(">>>")
        .trim()
        .split('|')
        .next()
        .unwrap_or(value)
        .trim();
    if let Some(hash) = token.strip_prefix('#') {
        return resolve_hash_only_anchor(hash, value, index, source_lines, recent_anchors);
    }
    let (line, hash) = token.split_once('#').ok_or_else(|| {
        ToolError::InvalidArguments(format!(
            "edit {index}: anchor \"{value}\" must use LINE#HASH format"
        ))
    })?;
    let line = line.parse::<u32>().map_err(|_| {
        ToolError::InvalidArguments(format!(
            "edit {index}: anchor \"{value}\" must use a numeric line number"
        ))
    })?;
    if hash.trim().is_empty() {
        return Err(ToolError::InvalidArguments(format!(
            "edit {index}: anchor \"{value}\" must include a hash after #"
        )));
    }
    Ok(LineAnchor {
        line,
        hash: hash.trim().to_string(),
    })
}

fn resolve_hash_only_anchor(
    hash: &str,
    value: &str,
    index: usize,
    source_lines: &[String],
    recent_anchors: Option<&[LineAnchor]>,
) -> Result<LineAnchor, ToolError> {
    let hash = hash.trim();
    if hash.is_empty() {
        return Err(ToolError::InvalidArguments(format!(
            "edit {index}: anchor \"{value}\" must include a hash after #"
        )));
    }

    let matches = source_lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            let line_hash = compute_line_hash(line);
            (line_hash == hash).then_some((line_index, line_hash))
        })
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] => Err(ToolError::InvalidArguments(format!(
            "edit {index}: anchor \"{value}\" omitted its line number and does not match any current line. Re-read with read(hashlineAnchors=true) or edit.hashline_scan and copy the full LINE#HASH tag."
        ))),
        [(line_index, line_hash)] => Ok(LineAnchor {
            line: *line_index as u32 + 1,
            hash: line_hash.clone(),
        }),
        _ => resolve_ambiguous_hash_only_anchor(hash, value, index, source_lines, recent_anchors),
    }
}

fn resolve_ambiguous_hash_only_anchor(
    hash: &str,
    value: &str,
    index: usize,
    source_lines: &[String],
    recent_anchors: Option<&[LineAnchor]>,
) -> Result<LineAnchor, ToolError> {
    if let Some(anchor) =
        resolve_hash_only_anchor_from_recent_read(hash, source_lines, recent_anchors)
    {
        return Ok(anchor);
    }

    let snippet = format_hash_only_anchor_candidates(hash, source_lines);
    Err(ToolError::InvalidArguments(format!(
        "edit {index}: anchor \"{value}\" omitted its line number and matches multiple current lines. Re-read with read(hashlineAnchors=true) or edit.hashline_scan and copy the full LINE#HASH tag. Matching tags:\n{}",
        snippet
    )))
}

fn resolve_hash_only_anchor_from_recent_read(
    hash: &str,
    source_lines: &[String],
    recent_anchors: Option<&[LineAnchor]>,
) -> Option<LineAnchor> {
    let matches = recent_anchors?
        .iter()
        .filter(|anchor| anchor.hash == hash)
        .cloned()
        .collect::<Vec<_>>();
    let [anchor] = matches.as_slice() else {
        return None;
    };
    let source = source_lines.get(anchor.line.saturating_sub(1) as usize)?;
    (compute_line_hash(source) == anchor.hash).then_some(anchor.clone())
}

fn format_hash_only_anchor_candidates(hash: &str, source_lines: &[String]) -> String {
    const MAX_CANDIDATES: usize = 8;

    let matches = source_lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            (compute_line_hash(line) == hash).then_some((line_index + 1, line))
        })
        .collect::<Vec<_>>();

    let mut lines = matches
        .iter()
        .take(MAX_CANDIDATES)
        .map(|(line_number, line)| format!(">>> {}#{}|{}", line_number, hash, line))
        .collect::<Vec<_>>();
    if matches.len() > MAX_CANDIDATES {
        lines.push(format!(
            ">>> ... and {} more matching lines",
            matches.len() - MAX_CANDIDATES
        ));
    }
    lines.join("\n")
}

fn anchors_for_range(
    source_lines: &[String],
    start: &LineAnchor,
    end: &LineAnchor,
    index: usize,
) -> Result<Vec<LineAnchor>, ToolError> {
    if start.line == 0 || end.line == 0 {
        return Err(ToolError::InvalidArguments(format!(
            "edit {index}: anchors use 1-based line numbers"
        )));
    }
    if start.line > end.line {
        return Err(ToolError::InvalidArguments(format!(
            "edit {index}: end anchor must be on or after pos"
        )));
    }

    validate_anchor(source_lines, start, index)?;
    validate_anchor(source_lines, end, index)?;

    let start_index = (start.line - 1) as usize;
    let end_index = (end.line - 1) as usize;
    Ok(source_lines[start_index..=end_index]
        .iter()
        .enumerate()
        .map(|(offset, line)| LineAnchor {
            line: start.line + offset as u32,
            hash: compute_line_hash(line),
        })
        .collect())
}

fn anchor_for_line(
    source_lines: &[String],
    line: u32,
    index: usize,
    op: &str,
) -> Result<LineAnchor, ToolError> {
    if source_lines.is_empty() {
        return Err(ToolError::Execution(format!(
            "edit {index}: {op} without anchors can only target an existing file when there is at least one line; use a single anchorless append/prepend call to create a new file"
        )));
    }
    let line_index = line.saturating_sub(1) as usize;
    let source = source_lines.get(line_index).ok_or_else(|| {
        ToolError::Execution(format!(
            "edit {index}: {op} anchor line {line} is out of range for the current file"
        ))
    })?;
    Ok(LineAnchor {
        line,
        hash: compute_line_hash(source),
    })
}

fn validate_anchor(
    source_lines: &[String],
    anchor: &LineAnchor,
    index: usize,
) -> Result<(), ToolError> {
    let source = source_lines
        .get(anchor.line.saturating_sub(1) as usize)
        .ok_or_else(|| {
            ToolError::Execution(format!(
                "edit {index}: anchor {}#{} is out of range for the current file",
                anchor.line, anchor.hash
            ))
        })?;

    let actual_hash = compute_line_hash(source);
    if actual_hash != anchor.hash {
        let snippet = format_anchor_refresh_snippet(source_lines, anchor.line);
        return Err(ToolError::Execution(format!(
            "edit {index}: anchor {}#{} no longer matches the current file (current hash: {}). Copy updated tags from this snippet and retry, or re-read with read(hashlineAnchors=true) / edit.hashline_scan if you need a wider view.\n{}",
            anchor.line, anchor.hash, actual_hash, snippet
        )));
    }
    Ok(())
}

fn format_anchor_refresh_snippet(source_lines: &[String], line: u32) -> String {
    if source_lines.is_empty() {
        return ">>> file is currently empty".to_string();
    }

    let center = line.saturating_sub(1) as usize;
    let start = center.saturating_sub(1);
    let end = usize::min(center + 1, source_lines.len().saturating_sub(1));

    (start..=end)
        .map(|index| {
            let content = &source_lines[index];
            format!(
                ">>> {}#{}|{}",
                index + 1,
                compute_line_hash(content),
                content
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn split_source_lines(source: &str) -> Vec<String> {
    source
        .strip_suffix('\n')
        .unwrap_or(source)
        .split('\n')
        .map(|line| line.to_string())
        .collect()
}

fn join_lines_with_trailing_newline(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}
