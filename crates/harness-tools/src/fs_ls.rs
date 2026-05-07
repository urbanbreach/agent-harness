use std::path::Path;

use async_trait::async_trait;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::limit_summary::summarize_limit;

const DEFAULT_LIMIT: usize = 2000;

pub(crate) struct FsLsTool;

#[derive(Debug, Deserialize, JsonSchema)]
struct FsLsArgs {
    path: String,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListResult {
    entries: Vec<String>,
    total_count: usize,
    returned_count: usize,
    truncated_count: usize,
    is_truncated: bool,
}

#[async_trait]
impl Tool for FsLsTool {
    fn id(&self) -> &str {
        "fs.ls"
    }

    fn description(&self) -> &str {
        "Lists immediate children of a workspace directory in deterministic order."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        super::json_schema_for::<FsLsArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: FsLsArgs = crate::parse_tool_args(args_json)?;

        let path = Path::new(&args.path);
        if path.is_absolute() {
            return Err(ToolError::InvalidArguments(
                "path must be relative to workspace root".to_string(),
            ));
        }

        let resolved = ctx.resolve_workspace_path(path)?;
        let limit = args.limit.map_or(DEFAULT_LIMIT, |value| value as usize);
        let list_result = list_directory_entries(&resolved, limit)?;

        Ok(crate::text_json_tool_result(
            list_result.entries.join("\n"),
            json!({
                "path": args.path,
                "resolved_path": resolved.display().to_string(),
                "entries": list_result.entries,
                "total_count": list_result.total_count,
                "returned_count": list_result.returned_count,
                "truncated_count": list_result.truncated_count,
                "truncated": list_result.is_truncated,
            }),
        ))
    }
}

fn list_directory_entries(directory: &Path, limit: usize) -> Result<ListResult, ToolError> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(|err| ToolError::Execution(format!("failed to list directory: {err}")))?
        .map(|entry| {
            entry
                .map_err(|err| {
                    ToolError::Execution(format!("failed to read directory entry: {err}"))
                })
                .and_then(|entry| {
                    let file_type = entry.file_type().map_err(|err| {
                        ToolError::Execution(format!("failed to read directory entry type: {err}"))
                    })?;

                    let mut name = entry.file_name().to_string_lossy().to_string();
                    if file_type.is_dir() {
                        name.push('/');
                    }
                    Ok(name)
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    entries.sort();

    let total_count = entries.len();
    let limit_summary = summarize_limit(total_count, limit);
    let mut returned_entries = entries
        .into_iter()
        .take(limit_summary.returned_count)
        .collect::<Vec<_>>();
    if limit_summary.is_truncated {
        returned_entries.push(format!(
            "... (truncated, {} more)",
            limit_summary.truncated_count
        ));
    }

    Ok(ListResult {
        total_count,
        returned_count: limit_summary.returned_count,
        truncated_count: limit_summary.truncated_count,
        is_truncated: limit_summary.is_truncated,
        entries: returned_entries,
    })
}

#[cfg(test)]
mod tests {
    use super::list_directory_entries;

    #[test]
    fn list_directory_entries_sorts_deterministically_and_marks_directories() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path();

        std::fs::write(root.join("zeta.txt"), "z").expect("write zeta");
        std::fs::create_dir(root.join("alpha")).expect("create alpha dir");
        std::fs::write(root.join("beta.txt"), "b").expect("write beta");

        let result = list_directory_entries(root, 2000).expect("list directory");

        assert_eq!(result.entries, vec!["alpha/", "beta.txt", "zeta.txt"]);
        assert_eq!(result.total_count, 3);
        assert_eq!(result.returned_count, 3);
        assert!(!result.is_truncated);
    }

    #[test]
    fn list_directory_entries_adds_truncation_marker_when_limit_hit() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path();

        std::fs::write(root.join("c.txt"), "c").expect("write c");
        std::fs::write(root.join("a.txt"), "a").expect("write a");
        std::fs::write(root.join("b.txt"), "b").expect("write b");

        let result = list_directory_entries(root, 2).expect("list directory");

        assert_eq!(
            result.entries,
            vec!["a.txt", "b.txt", "... (truncated, 1 more)"]
        );
        assert_eq!(result.total_count, 3);
        assert_eq!(result.returned_count, 2);
        assert_eq!(result.truncated_count, 1);
        assert!(result.is_truncated);
    }
}
