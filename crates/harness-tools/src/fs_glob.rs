use std::collections::BTreeSet;
use std::path::Path;

use async_trait::async_trait;
use globset::Glob;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::fs_walk::{collect_workspace_files, resolve_search_base, SKIPPED_WORKSPACE_DIRS};
use crate::limit_summary::summarize_limit;

pub(crate) const DEFAULT_GLOB_LIMIT: usize = 100;

pub(crate) struct FsGlobTool;

#[derive(Debug, Deserialize, JsonSchema)]
struct FsGlobArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GlobMatches {
    paths: Vec<String>,
    total_count: usize,
    returned_count: usize,
    truncated_count: usize,
    is_truncated: bool,
}

#[async_trait]
impl Tool for FsGlobTool {
    fn id(&self) -> &str {
        "fs.glob"
    }

    fn description(&self) -> &str {
        "Finds workspace files matching a globset pattern (supports ** recursive globs) with deterministic sorted output."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        super::json_schema_for::<FsGlobArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: FsGlobArgs = crate::parse_tool_args(args_json)?;

        let (workspace_root, resolved_base, display_path) =
            resolve_search_base(&ctx, args.path.as_deref())?;
        if !resolved_base.is_dir() {
            return Err(ToolError::InvalidArguments(
                "path must resolve to a directory".to_string(),
            ));
        }
        let limit = args
            .limit
            .map_or(DEFAULT_GLOB_LIMIT, |value| value as usize);

        let matches = collect_glob_matches(&workspace_root, &resolved_base, &args.pattern, limit)?;

        Ok(crate::text_json_tool_result(
            matches.paths.join("\n"),
            json!({
                "pattern": args.pattern,
                "path": display_path,
                "resolved_path": resolved_base.display().to_string(),
                "paths": matches.paths,
                "total_count": matches.total_count,
                "returned_count": matches.returned_count,
                "truncated_count": matches.truncated_count,
                "truncated": matches.is_truncated,
                "skipped_dirs": SKIPPED_WORKSPACE_DIRS,
            }),
        ))
    }
}

fn collect_glob_matches(
    workspace_root: &Path,
    base_dir: &Path,
    pattern: &str,
    limit: usize,
) -> Result<GlobMatches, ToolError> {
    let matcher = Glob::new(pattern)
        .map_err(|err| ToolError::InvalidArguments(format!("invalid glob pattern: {err}")))?
        .compile_matcher();

    let mut matched_paths = BTreeSet::new();
    for file in collect_workspace_files(workspace_root, base_dir)? {
        if matcher.is_match(&file.relative_path) {
            matched_paths.insert(file.relative_path);
        }
    }

    let total_count = matched_paths.len();
    let limit_summary = summarize_limit(total_count, limit);
    let paths = matched_paths
        .into_iter()
        .take(limit_summary.returned_count)
        .collect::<Vec<_>>();

    Ok(GlobMatches {
        returned_count: limit_summary.returned_count,
        is_truncated: limit_summary.is_truncated,
        paths,
        total_count,
        truncated_count: limit_summary.truncated_count,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;

    use harness_core::clock::RealClock;
    use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
    use harness_core::event::{ActorKind, EventActor};
    use harness_core::redact::DefaultRedactor;
    use harness_core::tool::{Tool, ToolContext};
    use serde_json::json;

    use super::{collect_glob_matches, FsGlobTool};

    fn test_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
        let coordinator = spawn_coordinator(
            CoordinatorConfig::default(),
            Arc::new(RealClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        ToolContext {
            run_id: "run-fs-glob-tests".to_string(),
            workspace_root: workspace_root.to_path_buf(),
            artifacts_dir: workspace_root.join("artifacts"),
            actor: EventActor::new(ActorKind::Worker, Some("worker-1".to_string())),
            category: Some("deep".to_string()),
            tool_call_id: tool_call_id.to_string(),
            coordinator,
        }
    }

    #[test]
    fn collect_glob_matches_supports_recursive_double_star_patterns() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path();

        fs::create_dir_all(root.join("src/nested")).expect("create src tree");
        fs::create_dir_all(root.join("tests")).expect("create tests dir");
        fs::create_dir_all(root.join("target/build")).expect("create target dir");
        fs::create_dir_all(root.join(".git/objects")).expect("create .git dir");
        fs::create_dir_all(root.join(".agent-harness/sessions/run-1"))
            .expect("create sessions dir");

        fs::write(root.join("src/lib.rs"), "pub fn lib() {}").expect("write src/lib.rs");
        fs::write(root.join("src/nested/mod.rs"), "pub fn nested() {}")
            .expect("write src/nested/mod.rs");
        fs::write(root.join("tests/glob.rs"), "#[test] fn t() {}").expect("write tests/glob.rs");
        fs::write(root.join("src/readme.md"), "# docs").expect("write src/readme.md");
        fs::write(root.join("target/build/generated.rs"), "ignored")
            .expect("write target/build/generated.rs");
        fs::write(root.join(".git/objects/cache.rs"), "ignored")
            .expect("write .git/objects/cache.rs");
        fs::write(
            root.join(".agent-harness/sessions/run-1/session.rs"),
            "ignored",
        )
        .expect("write session.rs");

        let result = collect_glob_matches(root, root, "**/*.rs", 100).expect("collect matches");

        assert_eq!(
            result.paths,
            vec!["src/lib.rs", "src/nested/mod.rs", "tests/glob.rs"]
        );
        assert_eq!(result.total_count, 3);
        assert_eq!(result.returned_count, 3);
        assert_eq!(result.truncated_count, 0);
        assert!(!result.is_truncated);
    }

    #[test]
    fn collect_glob_matches_applies_limit_after_deterministic_sort() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path();

        fs::create_dir_all(root.join("src")).expect("create src dir");
        fs::write(root.join("src/c.rs"), "").expect("write c");
        fs::write(root.join("src/a.rs"), "").expect("write a");
        fs::write(root.join("src/b.rs"), "").expect("write b");

        let result = collect_glob_matches(root, root, "**/*.rs", 2).expect("collect matches");

        assert_eq!(result.paths, vec!["src/a.rs", "src/b.rs"]);
        assert_eq!(result.total_count, 3);
        assert_eq!(result.returned_count, 2);
        assert_eq!(result.truncated_count, 1);
        assert!(result.is_truncated);
    }

    #[tokio::test]
    async fn fs_glob_accepts_absolute_workspace_paths() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workspace = tempdir.path().join("workspace");
        fs::create_dir_all(workspace.join("src")).expect("create workspace/src");
        fs::write(workspace.join("src/lib.rs"), "pub fn lib() {}\n").expect("write src/lib.rs");

        let result = FsGlobTool
            .call(
                test_context(&workspace, "glob-absolute-path"),
                json!({
                    "pattern": "**/*.rs",
                    "path": workspace.display().to_string(),
                }),
            )
            .await
            .expect("glob tool should accept absolute workspace paths");

        let structured = result.structured_json.expect("structured json");
        assert_eq!(structured.get("path"), Some(&json!(".")));
        assert_eq!(structured.get("paths"), Some(&json!(["src/lib.rs"])));
    }
}
