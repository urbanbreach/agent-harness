use std::path::Path;

use async_trait::async_trait;
use globset::Glob;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use walkdir::{DirEntry, WalkDir};

use crate::hashline_apply::resolve_workspace_target_path;

const DEFAULT_LIMIT: usize = 100;
const SKIPPED_DIR_NAMES: &[&str] = &[".git", "target"];
const SKIPPED_RELATIVE_DIRS: &[&str] = &[".agent-harness/sessions"];

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
        let args: FsGlobArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;

        let base_path = args.path.as_deref().unwrap_or(".");
        let workspace_root = ctx.resolve_workspace_path(Path::new("."))?;
        let resolved_base = resolve_workspace_target_path(&ctx, base_path)?;
        if !resolved_base.is_dir() {
            return Err(ToolError::InvalidArguments(
                "path must resolve to a directory".to_string(),
            ));
        }
        let display_path = workspace_relative_display(&workspace_root, &resolved_base)?;
        let limit = args.limit.map_or(DEFAULT_LIMIT, |value| value as usize);

        let matches = collect_glob_matches(&workspace_root, &resolved_base, &args.pattern, limit)?;

        Ok(ToolResult {
            display_text: matches.paths.join("\n"),
            structured_json: Some(json!({
                "pattern": args.pattern,
                "path": display_path,
                "resolved_path": resolved_base.display().to_string(),
                "paths": matches.paths,
                "total_count": matches.total_count,
                "returned_count": matches.returned_count,
                "truncated_count": matches.truncated_count,
                "truncated": matches.is_truncated,
                "skipped_dirs": [".git", "target", ".agent-harness/sessions"],
            })),
            artifacts: Vec::new(),
        })
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

    let mut matched_paths = WalkDir::new(base_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !should_skip_entry(workspace_root, entry))
        .map(|entry| {
            entry.map_err(|err| {
                ToolError::Execution(format!("failed to traverse directory tree: {err}"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| {
            let relative = entry.path().strip_prefix(workspace_root).map_err(|_| {
                ToolError::Execution(format!(
                    "failed to compute workspace-relative path for {}",
                    entry.path().display()
                ))
            })?;

            Ok(normalize_relative_path(relative))
        })
        .collect::<Result<Vec<_>, ToolError>>()?
        .into_iter()
        .filter(|relative| matcher.is_match(relative))
        .collect::<Vec<_>>();

    matched_paths.sort();

    let total_count = matched_paths.len();
    let capped_limit = limit.min(total_count);
    let paths = matched_paths
        .into_iter()
        .take(capped_limit)
        .collect::<Vec<_>>();
    let truncated_count = total_count.saturating_sub(capped_limit);

    Ok(GlobMatches {
        returned_count: capped_limit,
        is_truncated: truncated_count > 0,
        paths,
        total_count,
        truncated_count,
    })
}

fn should_skip_entry(workspace_root: &Path, entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }

    let path = entry.path();
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    if entry.file_type().is_dir() && SKIPPED_DIR_NAMES.contains(&name) {
        return true;
    }

    let Ok(relative) = path.strip_prefix(workspace_root) else {
        return false;
    };
    let relative = normalize_relative_path(relative);

    SKIPPED_RELATIVE_DIRS
        .iter()
        .any(|prefix| relative == *prefix || relative.starts_with(&format!("{prefix}/")))
}

fn normalize_relative_path(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        return ".".to_string();
    }

    path.iter()
        .map(|segment| segment.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn workspace_relative_display(
    workspace_root: &Path,
    resolved_path: &Path,
) -> Result<String, ToolError> {
    let relative = resolved_path.strip_prefix(workspace_root).map_err(|_| {
        ToolError::PathEscapesWorkspace {
            workspace_root: workspace_root.display().to_string(),
            path: resolved_path.display().to_string(),
        }
    })?;
    Ok(normalize_relative_path(relative))
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
