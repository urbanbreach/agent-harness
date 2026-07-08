use harness_core::edit::hashline::HashlineWorkspaceOp;
use harness_core::tool::{ToolContext, ToolError, ToolResult};
use harness_core::ToolResultExt;
use serde_json::json;

use crate::exact_edit_match::select_replacement_plan;
use crate::file_write::{
    append_lsp_diagnostics, diagnostics_json, run_file_formatter, run_lsp_diagnostics,
};
use crate::fs_walk::workspace_relative_display;
use crate::hashline_apply::{
    apply_hashline_workspace_op_to_workspace, resolve_workspace_target_path,
};
use crate::workspace_paths::canonical_workspace_root;

pub(crate) struct ExactEditRequest {
    pub(crate) edit_id: String,
    pub(crate) file_path: String,
    pub(crate) old_string: String,
    pub(crate) new_string: String,
    pub(crate) replace_all: bool,
}

pub(crate) async fn execute_exact_edit(
    ctx: &ToolContext,
    request: ExactEditRequest,
) -> Result<ToolResult, ToolError> {
    if request.old_string == request.new_string {
        return Err(ToolError::InvalidArguments(
            "No changes to apply: oldString and newString are identical.".to_string(),
        ));
    }
    let resolved_path = resolve_workspace_target_path(ctx, &request.file_path)?;
    let existed = resolved_path.exists();
    let (next, replacements) = if request.old_string.is_empty() {
        if existed {
            return Err(ToolError::InvalidArguments(
                "oldString cannot be empty when editing an existing file. Provide the exact text to replace, or use write for an intentional full-file replacement."
                    .to_string(),
            ));
        }
        (request.new_string, 0)
    } else {
        let decoded = decode_utf8_preserving_bom(&resolved_path)?;
        let ending = detect_line_ending(&decoded.text);
        let old_string = convert_line_endings(&request.old_string, ending);
        let new_string = convert_line_endings(&request.new_string, ending);
        let plan = select_replacement_plan(&decoded.text, &old_string, request.replace_all)?;

        let replaced = if request.replace_all {
            decoded.text.replace(&plan.matched_text, &new_string)
        } else {
            decoded.text.replacen(&plan.matched_text, &new_string, 1)
        };
        (join_bom(&replaced, decoded.bom), plan.replacements)
    };
    let mut result = apply_hashline_workspace_op_to_workspace(
        ctx,
        HashlineWorkspaceOp::RewriteFile {
            edit_id: request.edit_id,
            path: request.file_path,
            content: next,
        },
    )?;
    let workspace = canonical_workspace_root(ctx)?;
    let resource = workspace_relative_display(&workspace, &resolved_path)?;
    let format_warning = run_file_formatter(&workspace, &resolved_path).await;
    let diagnostics = run_lsp_diagnostics(workspace.clone(), resolved_path.clone()).await;
    result.display_text = format!(
        "Edited file successfully: {resource}\nReplacements: {}",
        replacements
    );
    if let Some(warning) = &format_warning {
        result.display_text.push_str("\n\nFormatter warning:\n");
        result.display_text.push_str(warning);
    }
    append_lsp_diagnostics(&mut result.display_text, &diagnostics, &resolved_path);
    if let Some(structured) = result.structured_json.as_mut() {
        structured["operation"] = json!("edit");
        structured["resource"] = json!(resource);
        structured["replacements"] = json!(replacements);
        structured["created"] = json!(!existed);
        structured["format_warning"] = match format_warning {
            Some(warning) => json!(warning),
            None => serde_json::Value::Null,
        };
        structured["diagnostics"] = diagnostics_json(&diagnostics);
    }
    Ok(result)
}

struct DecodedText {
    bom: bool,
    text: String,
}

fn decode_utf8_preserving_bom(path: &std::path::Path) -> Result<DecodedText, ToolError> {
    let bytes = std::fs::read(path)
        .tool_err(format!("Unable to edit {}", path.display()))?;
    let bom = bytes.starts_with(&[0xef, 0xbb, 0xbf]);
    let content = if bom { bytes[3..].to_vec() } else { bytes };
    let text = String::from_utf8(content).map_err(|_| {
        ToolError::Execution(format!("File is not valid UTF-8: {}", path.display()))
    })?;
    Ok(DecodedText { bom, text })
}

fn detect_line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn convert_line_endings(text: &str, ending: &str) -> String {
    let normalized = normalize_line_endings(text);
    if ending == "\r\n" {
        normalized.replace('\n', "\r\n")
    } else {
        normalized
    }
}

fn join_bom(text: &str, bom: bool) -> String {
    if bom {
        format!("\u{feff}{text}")
    } else {
        text.to_string()
    }
}
