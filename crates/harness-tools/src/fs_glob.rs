use crate::UnwrapOrAbort;
use std::path::Path;
use std::time::SystemTime;

use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use harness_core::tool_metadata;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::fs_walk::{
    collect_workspace_files, normalize_base_relative_path, resolve_search_base,
    SKIPPED_WORKSPACE_DIRS,
};
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

#[derive(Debug, Clone, Copy)]
struct GlobSearch<'a> {
    pattern: &'a str,
    limit: usize,
}

impl FsGlobArgs {
    fn search(&self) -> GlobSearch<'_> {
        GlobSearch {
            pattern: &self.pattern,
            limit: self
                .limit
                .map_or(DEFAULT_GLOB_LIMIT, |value| value as usize),
        }
    }
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
    tool_metadata!(
        "fs.glob",
        "Finds workspace files matching a globset pattern (supports ** recursive globs), sorted by modification time (newest first).",
        ToolCapability::ReadFs,
        super::json_schema_for::<FsGlobArgs>()
    );

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
        let matches = collect_glob_matches(&workspace_root, &resolved_base, args.search())?;

        Ok(crate::text_json_tool_result(
            render_glob_display(&workspace_root, &matches),
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
    search: GlobSearch<'_>,
) -> Result<GlobMatches, ToolError> {
    let matcher = compile_glob_matcher(search.pattern)?;
    let matched_paths = collect_matching_glob_paths(workspace_root, base_dir, &matcher)?;

    Ok(limit_glob_matches(matched_paths, search.limit))
}

fn collect_matching_glob_paths(
    workspace_root: &Path,
    base_dir: &Path,
    matcher: &GlobMatcher,
) -> Result<Vec<String>, ToolError> {
    let mut matched_paths = Vec::new();
    for file in collect_workspace_files(workspace_root, base_dir)? {
        let path_relative_to_base = normalize_base_relative_path(base_dir, &file.path);
        if matcher.is_match(path_relative_to_base) {
            matched_paths.push(file.relative_path);
        }
    }
    sort_paths_by_mtime_desc(workspace_root, &mut matched_paths);
    Ok(matched_paths)
}

fn sort_paths_by_mtime_desc(workspace_root: &Path, paths: &mut [String]) {
    paths.sort_by(|a, b| {
        let mtime_a = path_mtime(workspace_root, a);
        let mtime_b = path_mtime(workspace_root, b);
        mtime_b.cmp(&mtime_a)
    });
}

fn path_mtime(workspace_root: &Path, relative_path: &str) -> SystemTime {
    std::fs::metadata(workspace_root.join(relative_path))
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn limit_glob_matches(matched_paths: Vec<String>, limit: usize) -> GlobMatches {
    let total_count = matched_paths.len();
    let limit_summary = summarize_limit(total_count, limit);
    let paths = matched_paths
        .into_iter()
        .take(limit_summary.returned_count)
        .collect::<Vec<_>>();

    GlobMatches {
        returned_count: limit_summary.returned_count,
        is_truncated: limit_summary.is_truncated,
        paths,
        total_count,
        truncated_count: limit_summary.truncated_count,
    }
}

fn render_glob_display(workspace_root: &Path, matches: &GlobMatches) -> String {
    if matches.paths.is_empty() {
        return "No files found".to_string();
    }

    let mut output = matches
        .paths
        .iter()
        .map(|path| workspace_root.join(path).display().to_string())
        .collect::<Vec<_>>();

    if matches.is_truncated {
        output.push(String::new());
        output.push(format!(
            "(Results are truncated: showing first {} results. Consider using a more specific path or pattern.)",
            matches.returned_count
        ));
    }

    output.join("\n")
}

fn compile_glob_matcher(pattern: &str) -> Result<GlobMatcher, ToolError> {
    Glob::new(pattern)
        .map_err(|err| ToolError::InvalidArguments(format!("invalid glob pattern: {err}")))
        .map(|glob| glob.compile_matcher())
}

#[cfg(test)]
mod tests {
    use crate::UnwrapOrAbort;
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use harness_core::clock::RealClock;
    use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
    use harness_core::event::{ActorKind, EventActor};
    use harness_core::redact::DefaultRedactor;
    use harness_core::tool::{Tool, ToolContext, ToolRunState};
    use serde_json::json;

    use super::{collect_glob_matches, FsGlobTool, GlobSearch};

    fn glob_search(pattern: &str, limit: usize) -> GlobSearch<'_> {
        GlobSearch { pattern, limit }
    }

    fn create_dir(root: &Path, path: &str) {
        fs::create_dir_all(root.join(path)).unwrap_or_abort();
    }

    fn write_file(root: &Path, path: &str, contents: &str) {
        fs::write(root.join(path), contents).unwrap_or_abort();
    }

    fn set_mtime(root: &Path, path: &str, mtime: SystemTime) {
        let file = fs::OpenOptions::new()
            .write(true)
            .open(root.join(path))
            .unwrap_or_abort();
        file.set_times(fs::FileTimes::new().set_modified(mtime))
            .unwrap_or_abort();
    }

    fn test_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
        let coordinator = spawn_coordinator(
            CoordinatorConfig::default(),
            Arc::new(RealClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        ToolContext {
            run_id: "run-fs-glob-tests".into(),
            workspace_root: workspace_root.to_path_buf(),
            artifacts_dir: workspace_root.join("artifacts"),
            actor: EventActor::new(ActorKind::Worker, Some("worker-1".to_string())),
            category: Some("deep".to_string()),
            tool_call_id: tool_call_id.into(),
            current_model_ref: None,
            current_model_settings: None,
            tool_state: ToolRunState::default(),
            external_directory_allow_prefixes: Vec::new(),
            coordinator,
        }
    }

    #[test]
    fn collect_glob_matches_supports_recursive_double_star_patterns() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();

        create_dir(root, "src/nested");
        create_dir(root, "tests");
        create_dir(root, "target/build");
        create_dir(root, ".git/objects");
        create_dir(root, ".agent-harness/sessions/run-1");

        write_file(root, "src/lib.rs", "pub fn lib() {}");
        write_file(root, "src/nested/mod.rs", "pub fn nested() {}");
        write_file(root, "tests/glob.rs", "#[test] fn t() {}");
        write_file(root, "src/readme.md", "# docs");
        write_file(root, "target/build/generated.rs", "ignored");
        write_file(root, ".git/objects/cache.rs", "ignored");
        write_file(root, ".agent-harness/sessions/run-1/session.rs", "ignored");

        set_mtime(
            root,
            "src/lib.rs",
            SystemTime::UNIX_EPOCH + Duration::from_secs(100),
        );
        set_mtime(
            root,
            "src/nested/mod.rs",
            SystemTime::UNIX_EPOCH + Duration::from_secs(200),
        );
        set_mtime(
            root,
            "tests/glob.rs",
            SystemTime::UNIX_EPOCH + Duration::from_secs(300),
        );

        let result =
            collect_glob_matches(root, root, glob_search("**/*.rs", 100)).unwrap_or_abort();

        assert_eq!(
            result.paths,
            vec!["tests/glob.rs", "src/nested/mod.rs", "src/lib.rs"]
        );
        assert_eq!(result.total_count, 3);
        assert_eq!(result.returned_count, 3);
        assert_eq!(result.truncated_count, 0);
        assert!(!result.is_truncated);
    }

    #[test]
    fn collect_glob_matches_applies_limit_after_mtime_sort() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();

        create_dir(root, "src");
        write_file(root, "src/c.rs", "");
        write_file(root, "src/a.rs", "");
        write_file(root, "src/b.rs", "");

        set_mtime(
            root,
            "src/c.rs",
            SystemTime::UNIX_EPOCH + Duration::from_secs(300),
        );
        set_mtime(
            root,
            "src/a.rs",
            SystemTime::UNIX_EPOCH + Duration::from_secs(200),
        );
        set_mtime(
            root,
            "src/b.rs",
            SystemTime::UNIX_EPOCH + Duration::from_secs(100),
        );

        let result = collect_glob_matches(root, root, glob_search("**/*.rs", 2)).unwrap_or_abort();

        assert_eq!(result.paths, vec!["src/c.rs", "src/a.rs"]);
        assert_eq!(result.total_count, 3);
        assert_eq!(result.returned_count, 2);
        assert_eq!(result.truncated_count, 1);
        assert!(result.is_truncated);
    }

    #[test]
    fn collect_glob_matches_sorts_by_modification_time_newest_first() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();

        create_dir(root, "src");
        write_file(root, "src/old.rs", "");
        write_file(root, "src/middle.rs", "");
        write_file(root, "src/new.rs", "");

        set_mtime(
            root,
            "src/old.rs",
            SystemTime::UNIX_EPOCH + Duration::from_secs(100),
        );
        set_mtime(
            root,
            "src/middle.rs",
            SystemTime::UNIX_EPOCH + Duration::from_secs(200),
        );
        set_mtime(
            root,
            "src/new.rs",
            SystemTime::UNIX_EPOCH + Duration::from_secs(300),
        );

        let result =
            collect_glob_matches(root, root, glob_search("**/*.rs", 100)).unwrap_or_abort();

        assert_eq!(
            result.paths,
            vec!["src/new.rs", "src/middle.rs", "src/old.rs"]
        );
        assert_eq!(result.total_count, 3);
        assert_eq!(result.returned_count, 3);
        assert!(!result.is_truncated);
    }

    #[test]
    fn collect_glob_matches_applies_slash_pattern_relative_to_search_base() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();

        create_dir(root, "src/nested");
        write_file(root, "src/lib.rs", "pub fn lib() {}\n");
        write_file(root, "src/nested/mod.rs", "pub fn nested() {}\n");

        // act
        let result = collect_glob_matches(root, &root.join("src"), glob_search("nested/*.rs", 100))
            .unwrap_or_abort();

        // assert
        assert_eq!(result.paths, vec!["src/nested/mod.rs"]);
    }

    #[tokio::test]
    async fn fs_glob_accepts_absolute_workspace_paths() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let workspace = tempdir.path().join("workspace");
        create_dir(&workspace, "src");
        write_file(&workspace, "src/lib.rs", "pub fn lib() {}\n");

        let result = FsGlobTool
            .call(
                test_context(&workspace, "glob-absolute-path"),
                json!({
                    "pattern": "**/*.rs",
                    "path": workspace.display().to_string(),
                }),
            )
            .await
            .unwrap_or_abort();

        let structured = result.structured_json.unwrap_or_abort();
        assert_eq!(structured.get("path"), Some(&json!(".")));
        assert_eq!(structured.get("paths"), Some(&json!(["src/lib.rs"])));
    }
}
