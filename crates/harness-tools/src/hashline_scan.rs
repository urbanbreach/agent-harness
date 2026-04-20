use std::path::Path;

use async_trait::async_trait;
use harness_core::edit::hashline::{compute_line_hash, LineAnchor};
use harness_core::redact::{redact_value, DefaultRedactor};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::workspace_edit::record_file_hashline_read;

const DEFAULT_START_LINE: u32 = 1;
const DEFAULT_LIMIT: u32 = 2000;

pub(crate) struct HashlineScanTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct HashlineScanArgs {
    path: String,
    #[serde(default)]
    start_line: Option<u32>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct HashlineAnchor {
    line: u32,
    hash: String,
    text: String,
}

#[async_trait]
impl Tool for HashlineScanTool {
    fn id(&self) -> &str {
        "edit.hashline_scan"
    }

    fn description(&self) -> &str {
        "Scans workspace file lines and returns hashline anchors for patch authoring. Prefer this or read(hashlineAnchors=true) before edit.hashline_apply when precise edits might be stale."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        super::json_schema_for::<HashlineScanArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: HashlineScanArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;

        let path = Path::new(&args.path);
        if path.is_absolute() {
            return Err(ToolError::InvalidArguments(
                "path must be relative to workspace root".to_string(),
            ));
        }

        let resolved_path = ctx.resolve_workspace_path(path)?;
        let source = std::fs::read_to_string(&resolved_path)
            .map_err(|err| ToolError::Execution(format!("failed to read target file: {err}")))?;

        let start_line = args.start_line.unwrap_or(DEFAULT_START_LINE).max(1);
        let limit = args.limit.unwrap_or(DEFAULT_LIMIT);
        let anchors = scan_line_anchors(&source, start_line, limit);

        let structured_json = json!({
            "path": args.path,
            "resolved_path": resolved_path.display().to_string(),
            "start_line": start_line,
            "limit": limit,
            "anchors": anchors,
        });

        let redacted_artifact = redact_value(&DefaultRedactor::default(), &structured_json);
        let redacted_artifact_text =
            serde_json::to_string_pretty(&redacted_artifact).map_err(|err| {
                ToolError::Execution(format!("failed to serialize hashline scan artifact: {err}"))
            })?;

        record_file_hashline_read(
            &ctx,
            &resolved_path,
            anchors
                .iter()
                .map(|anchor| LineAnchor {
                    line: anchor.line,
                    hash: anchor.hash.clone(),
                })
                .collect(),
        )?;

        let artifact_name = format!("hashline_scan/{}.json", sanitize_artifact_name(&args.path));
        let artifact = ctx
            .artifact_store()
            .map_err(|err| ToolError::Execution(format!("failed to access artifact store: {err}")))?
            .write_text(&artifact_name, &redacted_artifact_text)
            .map_err(|err| {
                ToolError::Execution(format!("failed to write hashline scan artifact: {err}"))
            })?;

        Ok(ToolResult {
            display_text: render_display_text(&anchors),
            structured_json: Some(structured_json),
            artifacts: vec![artifact],
        })
    }
}

fn scan_line_anchors(content: &str, start_line: u32, limit: u32) -> Vec<HashlineAnchor> {
    if limit == 0 {
        return Vec::new();
    }

    let body = content.strip_suffix('\n').unwrap_or(content);
    if body.is_empty() {
        return Vec::new();
    }

    let start_index = start_line.saturating_sub(1) as usize;
    body.split('\n')
        .enumerate()
        .skip(start_index)
        .take(limit as usize)
        .map(|(index, line)| HashlineAnchor {
            line: u32::try_from(index + 1).unwrap_or(u32::MAX),
            hash: compute_line_hash(line),
            text: line.strip_suffix('\r').unwrap_or(line).to_string(),
        })
        .collect()
}

fn render_display_text(anchors: &[HashlineAnchor]) -> String {
    anchors
        .iter()
        .map(|anchor| format!("{}#{}|{}", anchor.line, anchor.hash, anchor.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn sanitize_artifact_name(path: &str) -> String {
    let sanitized = path
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();

    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() {
        "workspace_root".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::HashlineScanTool;
    use harness_core::clock::RealClock;
    use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
    use harness_core::edit::hashline::compute_line_hash;
    use harness_core::event::{ActorKind, EventActor};
    use harness_core::redact::DefaultRedactor;
    use harness_core::tool::{Tool, ToolContext};
    use serde_json::json;
    use std::sync::Arc;

    fn test_context(
        workspace_root: &std::path::Path,
        artifacts_dir: &std::path::Path,
    ) -> ToolContext {
        let coordinator = spawn_coordinator(
            CoordinatorConfig::default(),
            Arc::new(RealClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        ToolContext {
            run_id: "run-1".to_string(),
            workspace_root: workspace_root.to_path_buf(),
            artifacts_dir: artifacts_dir.to_path_buf(),
            actor: EventActor::new(ActorKind::Worker, Some("worker-1".to_string())),
            category: Some("deep".to_string()),
            plan_mode: false,
            plan_exit_target_profile: None,
            tool_call_id: "tool-call-1".to_string(),
            coordinator,
        }
    }

    #[tokio::test]
    async fn hashline_scan_anchor_hashes_match_compute_line_hash() {
        let workspace = tempfile::tempdir().expect("workspace tempdir");
        let artifacts = tempfile::tempdir().expect("artifacts tempdir");
        let file_path = workspace.path().join("demo.txt");
        std::fs::write(&file_path, "alpha\nbeta\r\ngamma\n").expect("write demo file");

        let tool = HashlineScanTool;
        let result = tool
            .call(
                test_context(workspace.path(), artifacts.path()),
                json!({ "path": "demo.txt" }),
            )
            .await
            .expect("hashline_scan should succeed");

        let payload = result
            .structured_json
            .expect("hashline_scan should include structured_json");
        let anchors = payload["anchors"]
            .as_array()
            .expect("anchors should be an array");
        assert_eq!(anchors.len(), 3);

        assert_eq!(
            result.display_text,
            format!(
                "1#{}|alpha\n2#{}|beta\n3#{}|gamma",
                compute_line_hash("alpha"),
                compute_line_hash("beta"),
                compute_line_hash("gamma")
            )
        );

        assert_eq!(anchors[0]["line"], json!(1));
        assert_eq!(anchors[0]["hash"], json!(compute_line_hash("alpha")));
        assert_eq!(anchors[0]["text"], json!("alpha"));

        assert_eq!(anchors[1]["line"], json!(2));
        assert_eq!(anchors[1]["hash"], json!(compute_line_hash("beta")));
        assert_eq!(anchors[1]["text"], json!("beta"));

        assert_eq!(anchors[2]["line"], json!(3));
        assert_eq!(anchors[2]["hash"], json!(compute_line_hash("gamma")));
        assert_eq!(anchors[2]["text"], json!("gamma"));
    }

    #[tokio::test]
    async fn hashline_scan_out_of_range_start_line_returns_empty_anchors() {
        let workspace = tempfile::tempdir().expect("workspace tempdir");
        let artifacts = tempfile::tempdir().expect("artifacts tempdir");
        let file_path = workspace.path().join("demo.txt");
        std::fs::write(&file_path, "alpha\nbeta\n").expect("write demo file");

        let tool = HashlineScanTool;
        let result = tool
            .call(
                test_context(workspace.path(), artifacts.path()),
                json!({ "path": "demo.txt", "start_line": 99, "limit": 10 }),
            )
            .await
            .expect("out-of-range start_line should be handled gracefully");

        let payload = result
            .structured_json
            .expect("hashline_scan should include structured_json");
        let anchors = payload["anchors"]
            .as_array()
            .expect("anchors should be an array");

        assert!(anchors.is_empty());
        assert_eq!(payload["start_line"], json!(99));
        assert_eq!(payload["limit"], json!(10));
        assert!(result.display_text.is_empty());
    }
}
