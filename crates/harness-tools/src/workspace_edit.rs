use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use harness_core::edit::hashline::{
    apply_hashline_patch, compute_line_hash, HashlineOp, HashlinePatch, HashlineWorkspaceOp,
    LineAnchor,
};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use similar::TextDiff;

use crate::hashline_apply::{
    apply_hashline_workspace_op_to_workspace, resolve_workspace_target_path,
    write_file_via_hashline_engine,
};

pub(crate) struct WorkspaceEditExecutor;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    modified_at: Option<SystemTime>,
    created_at: Option<SystemTime>,
    size: u64,
}

#[derive(Debug, Clone)]
struct FileReadStamp {
    read_at: SystemTime,
    fingerprint: FileFingerprint,
}

#[derive(Default)]
struct FileReadRegistry {
    reads: BTreeMap<(String, String), FileReadStamp>,
    locks: BTreeMap<String, Arc<Mutex<()>>>,
}

static FILE_READ_REGISTRY: OnceLock<Mutex<FileReadRegistry>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelaxedMatch {
    None,
    Unique { start: usize, end: usize },
    Multiple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelaxedSubsequenceMatch {
    None,
    Unique { start: usize },
    Multiple,
}

pub(crate) struct EditTool {
    executor: Arc<WorkspaceEditExecutor>,
}

pub(crate) struct ApplyPatchTool {
    executor: Arc<WorkspaceEditExecutor>,
}

pub(crate) struct PatchTool {
    executor: Arc<WorkspaceEditExecutor>,
}

impl WorkspaceEditExecutor {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn write_file(
        &self,
        ctx: &ToolContext,
        file_path: &str,
        content: &str,
    ) -> Result<ToolResult, ToolError> {
        let (path, resolved_path) = resolve_edit_target_path(ctx, file_path)?;
        let existed = resolved_path.exists();

        with_workspace_file_lock(&resolved_path, || {
            if existed {
                assert_file_read_fresh(ctx, &resolved_path)?;
            }

            let mut result = write_file_via_hashline_engine(
                ctx,
                &path,
                content,
                &format!("fs-write-{}", ctx.tool_call_id),
            )?;
            record_file_read(ctx, &resolved_path)?;
            result.display_text = format!("Wrote file successfully: {}", resolved_path.display());
            Ok(result)
        })
    }

    pub(crate) fn apply_patch(
        &self,
        ctx: &ToolContext,
        patch_text: &str,
    ) -> Result<ToolResult, ToolError> {
        let sections = parse_apply_patch_sections(patch_text)?;
        if sections.is_empty() {
            return Err(ToolError::InvalidArguments(
                "patch rejected: empty patch".to_string(),
            ));
        }

        let translated = translate_apply_patch_sections(ctx, &sections)?;
        for op in translated.ops {
            let _ = apply_hashline_workspace_op_to_workspace(ctx, op)?;
        }

        for edit in &translated.edits {
            if edit.deleted {
                continue;
            }
            let resolved_path = resolve_workspace_target_path(ctx, &edit.path)?;
            if resolved_path.exists() {
                record_file_read(ctx, &resolved_path)?;
            }
        }

        let mut artifacts = Vec::new();
        let mut edits = Vec::new();
        for edit in translated.edits {
            let artifact = ctx
                .artifact_store()
                .map_err(|err| {
                    ToolError::Execution(format!("failed to access artifact store: {err}"))
                })?
                .write_text(&format!("edit-{}.diff", edit.edit_id), &edit.diff_content)
                .map_err(|err| {
                    ToolError::Execution(format!("failed to write diff artifact: {err}"))
                })?;
            edits.push(json!({
                "edit_id": edit.edit_id,
                "path": edit.path,
                "summary": edit.summary,
                "deleted": edit.deleted,
                "diff_rel_path": artifact.path,
                "diff_digest": artifact.digest,
            }));
            artifacts.push(artifact);
        }

        Ok(ToolResult {
            display_text: format!(
                "Success. Updated the following files:\n{}",
                translated.files.join("\n")
            ),
            structured_json: Some(json!({
                "files": translated.files,
                "edits": edits,
            })),
            artifacts,
        })
    }

    pub(crate) fn edit_file(
        &self,
        ctx: &ToolContext,
        file_path: &str,
        old_string: &str,
        new_string: &str,
        replace_all: bool,
    ) -> Result<ToolResult, ToolError> {
        if old_string == new_string {
            return Err(ToolError::InvalidArguments(
                "No changes to apply: oldString and newString are identical.".to_string(),
            ));
        }

        let (path, resolved_path) = resolve_edit_target_path(ctx, file_path)?;

        if old_string.is_empty() {
            return with_workspace_file_lock(&resolved_path, || {
                let mut result = write_file_via_hashline_engine(
                    ctx,
                    &path,
                    new_string,
                    &format!("edit-{}", ctx.tool_call_id),
                )?;
                record_file_read(ctx, &resolved_path)?;
                result.display_text = "Edit applied successfully.".to_string();
                Ok(result)
            });
        }

        with_workspace_file_lock(&resolved_path, || {
            let metadata = std::fs::metadata(&resolved_path).map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => {
                    ToolError::Execution(format!("File {} not found", resolved_path.display()))
                }
                _ => ToolError::Execution(format!("failed to inspect file for edit: {err}")),
            })?;
            if metadata.is_dir() {
                return Err(ToolError::Execution(format!(
                    "Path is a directory, not a file: {}",
                    resolved_path.display()
                )));
            }

            assert_file_read_fresh(ctx, &resolved_path)?;

            let source = std::fs::read_to_string(&resolved_path).map_err(|err| {
                ToolError::Execution(format!("failed to read file for edit: {err}"))
            })?;
            let ending = detect_line_ending(&source);
            let old = normalize_line_endings(old_string, ending);
            let new = normalize_line_endings(new_string, ending);
            let matches = source.matches(&old).count();

            if matches == 0 {
                if old.contains('\n') {
                    match find_relaxed_multiline_match_range(&source, &old) {
                        RelaxedMatch::Unique { start, end } => {
                            let updated = format!("{}{}{}", &source[..start], new, &source[end..]);
                            let mut result = write_file_via_hashline_engine(
                                ctx,
                                &path,
                                &updated,
                                &format!("edit-{}", ctx.tool_call_id),
                            )?;
                            record_file_read(ctx, &resolved_path)?;
                            result.display_text = "Edit applied successfully.".to_string();
                            return Ok(result);
                        }
                        RelaxedMatch::Multiple => {
                            return Err(ToolError::Execution(
                                "oldString was not found exactly; multiple multiline regions match when ignoring trailing spaces"
                                    .to_string(),
                            ));
                        }
                        RelaxedMatch::None => {}
                    }
                }
                return Err(ToolError::Execution(
                    "oldString not found in target file; re-read the file and include exact current text, including whitespace. For a more robust path, prefer read(hashlineAnchors=true) plus edit.hashline_apply"
                        .to_string(),
                ));
            }
            if matches > 1 && !replace_all {
                return Err(ToolError::Execution(
                    "oldString matches multiple locations; set replaceAll=true to replace all occurrences"
                        .to_string(),
                ));
            }

            let updated = if replace_all {
                source.replace(&old, &new)
            } else {
                source.replacen(&old, &new, 1)
            };

            let mut result = write_file_via_hashline_engine(
                ctx,
                &path,
                &updated,
                &format!("edit-{}", ctx.tool_call_id),
            )?;
            record_file_read(ctx, &resolved_path)?;
            result.display_text = "Edit applied successfully.".to_string();
            Ok(result)
        })
    }
}

pub(crate) struct FsWriteTool {
    executor: Arc<WorkspaceEditExecutor>,
}

impl EditTool {
    pub(crate) fn new(executor: Arc<WorkspaceEditExecutor>) -> Self {
        Self { executor }
    }
}

impl ApplyPatchTool {
    pub(crate) fn new(executor: Arc<WorkspaceEditExecutor>) -> Self {
        Self { executor }
    }
}

impl PatchTool {
    pub(crate) fn new(executor: Arc<WorkspaceEditExecutor>) -> Self {
        Self { executor }
    }
}

impl FsWriteTool {
    pub(crate) fn new(executor: Arc<WorkspaceEditExecutor>) -> Self {
        Self { executor }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FsWriteArgs {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ApplyPatchArgs {
    #[serde(rename = "patchText")]
    patch_text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    #[serde(rename = "filePath")]
    file_path: String,
    #[serde(rename = "oldString")]
    old_string: String,
    #[serde(rename = "newString")]
    new_string: String,
    #[serde(rename = "replaceAll", default)]
    replace_all: bool,
}

#[derive(Debug)]
enum ApplyPatchSection {
    Add {
        path: String,
        lines: Vec<String>,
    },
    Update {
        path: String,
        move_to: Option<String>,
        hunks: Vec<Vec<String>>,
    },
    Delete {
        path: String,
    },
}

struct TranslatedPatchPlan {
    ops: Vec<HashlineWorkspaceOp>,
    files: Vec<String>,
    edits: Vec<TranslatedPatchEdit>,
}

struct TranslatedPatchEdit {
    edit_id: String,
    path: String,
    summary: String,
    deleted: bool,
    diff_content: String,
}

#[async_trait]
impl Tool for FsWriteTool {
    fn id(&self) -> &str {
        "fs.write"
    }

    fn description(&self) -> &str {
        "Writes file contents to a workspace-relative path via the hashline edit engine."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<FsWriteArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: FsWriteArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.write_file(&ctx, &args.path, &args.content)
    }
}

#[async_trait]
impl Tool for EditTool {
    fn id(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Modifies an existing workspace file via exact string replacement. Prefer read(hashlineAnchors=true) + edit.hashline_apply for robust, anchor-driven edits when text may be stale or repeated."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<EditArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: EditArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.edit_file(
            &ctx,
            &args.file_path,
            &args.old_string,
            &args.new_string,
            args.replace_all,
        )
    }
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn id(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Applies stripped-down apply_patch diffs compatible with Opencode's patch envelope. For the most robust edits, prefer hashline anchors via read(hashlineAnchors=true) or edit.hashline_scan plus edit.hashline_apply."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<ApplyPatchArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: ApplyPatchArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.apply_patch(&ctx, &args.patch_text)
    }
}

#[async_trait]
impl Tool for PatchTool {
    fn id(&self) -> &str {
        "patch"
    }

    fn description(&self) -> &str {
        "Applies stripped-down patch envelopes to workspace files. For the most robust edits, prefer hashline anchors via read(hashlineAnchors=true) or edit.hashline_scan plus edit.hashline_apply."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<ApplyPatchArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: ApplyPatchArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.apply_patch(&ctx, &args.patch_text)
    }
}

fn resolve_edit_target_path(
    ctx: &ToolContext,
    file_path: &str,
) -> Result<(String, std::path::PathBuf), ToolError> {
    let resolved_path = resolve_workspace_target_path(ctx, file_path)?;
    let workspace = ctx
        .workspace_root
        .canonicalize()
        .map_err(|err| ToolError::Execution(format!("failed to resolve workspace root: {err}")))?;
    let path = resolved_path
        .strip_prefix(&workspace)
        .map_err(|_| ToolError::PathEscapesWorkspace {
            workspace_root: workspace.display().to_string(),
            path: resolved_path.display().to_string(),
        })?
        .to_string_lossy()
        .replace('\\', "/");
    Ok((path, resolved_path))
}

fn detect_line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn normalize_line_endings(text: &str, ending: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if ending == "\r\n" {
        normalized.replace('\n', "\r\n")
    } else {
        normalized
    }
}

pub(crate) fn record_file_read(ctx: &ToolContext, resolved_path: &Path) -> Result<(), ToolError> {
    let stamp = file_read_stamp_from_metadata(resolved_path)?;
    let key = file_read_key(ctx, resolved_path);
    let registry = file_read_registry();
    let mut state = lock_mutex(registry);
    state.reads.insert(key, stamp);
    Ok(())
}

fn assert_file_read_fresh(ctx: &ToolContext, resolved_path: &Path) -> Result<(), ToolError> {
    let key = file_read_key(ctx, resolved_path);
    let prior = {
        let registry = file_read_registry();
        let state = lock_mutex(registry);
        state.reads.get(&key).cloned()
    };

    let Some(prior) = prior else {
        return Err(ToolError::Execution(format!(
            "You must read file {} before overwriting it. Prefer read(hashlineAnchors=true) or edit.hashline_scan first so you have fresh LINE#HASH anchors.",
            resolved_path.display()
        )));
    };

    let current = file_read_stamp_from_metadata(resolved_path)?;
    if current.fingerprint == prior.fingerprint {
        return Ok(());
    }

    Err(ToolError::Execution(format!(
        "File {} has been modified since it was last read.\nLast modification: {}\nLast read: {}\n\nPlease read the file again before modifying it. Prefer read(hashlineAnchors=true) or edit.hashline_scan so you can refresh LINE#HASH anchors before editing.",
        resolved_path.display(),
        format_fingerprint_time(&current.fingerprint),
        format_system_time(prior.read_at),
    )))
}

fn with_workspace_file_lock<T>(
    resolved_path: &Path,
    f: impl FnOnce() -> Result<T, ToolError>,
) -> Result<T, ToolError> {
    let key = resolved_path.display().to_string();
    let lock = {
        let registry = file_read_registry();
        let mut state = lock_mutex(registry);
        state
            .locks
            .entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let _guard = lock_mutex(&lock);
    f()
}

fn file_read_registry() -> &'static Mutex<FileReadRegistry> {
    FILE_READ_REGISTRY.get_or_init(|| Mutex::new(FileReadRegistry::default()))
}

fn lock_mutex<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn file_read_key(ctx: &ToolContext, resolved_path: &Path) -> (String, String) {
    (ctx.run_id.clone(), resolved_path.display().to_string())
}

fn file_read_stamp_from_metadata(resolved_path: &Path) -> Result<FileReadStamp, ToolError> {
    let metadata = std::fs::metadata(resolved_path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => {
            ToolError::Execution(format!("File {} not found", resolved_path.display()))
        }
        _ => ToolError::Execution(format!(
            "failed to inspect file freshness for {}: {err}",
            resolved_path.display()
        )),
    })?;

    Ok(FileReadStamp {
        read_at: SystemTime::now(),
        fingerprint: FileFingerprint {
            modified_at: metadata.modified().ok(),
            created_at: metadata.created().ok(),
            size: metadata.len(),
        },
    })
}

fn format_fingerprint_time(fingerprint: &FileFingerprint) -> String {
    fingerprint
        .modified_at
        .or(fingerprint.created_at)
        .map(format_system_time)
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_system_time(time: SystemTime) -> String {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => format!(
            "{}.{:03}s since UNIX epoch",
            duration.as_secs(),
            duration.subsec_millis()
        ),
        Err(_) => "before UNIX epoch".to_string(),
    }
}

fn find_relaxed_multiline_match_range(source: &str, old: &str) -> RelaxedMatch {
    let source_lines = collect_line_ranges(source);
    let old_lines = collect_line_ranges(old);
    if source_lines.is_empty() || old_lines.is_empty() {
        return RelaxedMatch::None;
    }
    if old_lines.len() > source_lines.len() {
        return RelaxedMatch::None;
    }

    let mut found = None;
    for window in source_lines.windows(old_lines.len()) {
        let matches = window.iter().zip(old_lines.iter()).all(|(src, old)| {
            trim_edit_match_line(src.content) == trim_edit_match_line(old.content)
        });
        if !matches {
            continue;
        }

        let start = window.first().map(|line| line.start).unwrap_or(0);
        let end = window.last().map(|line| line.end).unwrap_or(start);
        if found.replace((start, end)).is_some() {
            return RelaxedMatch::Multiple;
        }
    }

    match found {
        Some((start, end)) => RelaxedMatch::Unique { start, end },
        None => RelaxedMatch::None,
    }
}

#[derive(Debug, Clone, Copy)]
struct LineRange<'a> {
    content: &'a str,
    start: usize,
    end: usize,
}

fn collect_line_ranges(text: &str) -> Vec<LineRange<'_>> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    for raw in text.split_inclusive('\n') {
        let end = start + raw.len();
        lines.push(LineRange {
            content: raw.trim_end_matches(['\r', '\n']),
            start,
            end,
        });
        start = end;
    }
    if start < text.len() {
        lines.push(LineRange {
            content: &text[start..],
            start,
            end: text.len(),
        });
    }
    lines
}

fn trim_edit_match_line(line: &str) -> &str {
    line.trim_end_matches([' ', '\t'])
}

fn parse_apply_patch_sections(patch_text: &str) -> Result<Vec<ApplyPatchSection>, ToolError> {
    let normalized = patch_text.replace("\r\n", "\n").replace('\r', "\n");
    let lines = normalized.lines().collect::<Vec<_>>();
    if lines.first().copied() != Some("*** Begin Patch")
        || lines.last().copied() != Some("*** End Patch")
    {
        return Err(ToolError::InvalidArguments(
            "patch must start with '*** Begin Patch' and end with '*** End Patch'".to_string(),
        ));
    }

    let mut sections = Vec::new();
    let mut idx = 1usize;
    while idx + 1 < lines.len() {
        let line = lines[idx];

        if let Some(path) = line.strip_prefix("*** Add File: ") {
            idx += 1;
            let mut file_lines = Vec::new();
            while idx < lines.len() && !lines[idx].starts_with("*** ") {
                let add_line = lines[idx];
                if !add_line.starts_with('+') {
                    return Err(ToolError::Execution(
                        "apply_patch add-file lines must start with '+'".to_string(),
                    ));
                }
                file_lines.push(add_line[1..].to_string());
                idx += 1;
            }
            sections.push(ApplyPatchSection::Add {
                path: path.to_string(),
                lines: file_lines,
            });
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Delete File: ") {
            idx += 1;
            if idx < lines.len() && !lines[idx].starts_with("*** ") {
                return Err(ToolError::Execution(
                    "apply_patch delete-file sections cannot contain hunks".to_string(),
                ));
            }
            sections.push(ApplyPatchSection::Delete {
                path: path.to_string(),
            });
            continue;
        }

        let Some(path) = line.strip_prefix("*** Update File: ") else {
            return Err(ToolError::Execution(
                "apply_patch translation currently supports only '*** Add File', '*** Update File', and '*** Delete File' sections"
                    .to_string(),
            ));
        };
        idx += 1;

        let mut move_to = None;
        if idx < lines.len() {
            if let Some(destination) = lines[idx].strip_prefix("*** Move to: ") {
                move_to = Some(destination.to_string());
                idx += 1;
            }
        }

        let mut hunks = Vec::new();
        let mut current_hunk = Vec::new();
        while idx < lines.len() && !lines[idx].starts_with("*** ") {
            let hunk_line = lines[idx];
            if hunk_line.starts_with("@@") {
                if !current_hunk.is_empty() {
                    hunks.push(current_hunk);
                    current_hunk = Vec::new();
                }
                idx += 1;
                continue;
            }
            if !(hunk_line.starts_with('+')
                || hunk_line.starts_with('-')
                || hunk_line.starts_with(' '))
            {
                return Err(ToolError::Execution(
                    "apply_patch hunk lines must start with '+', '-', or space".to_string(),
                ));
            }
            current_hunk.push(hunk_line.to_string());
            idx += 1;
        }
        if !current_hunk.is_empty() {
            hunks.push(current_hunk);
        }

        sections.push(ApplyPatchSection::Update {
            path: path.to_string(),
            move_to,
            hunks,
        });
    }

    Ok(sections)
}

fn translate_apply_patch_sections(
    ctx: &ToolContext,
    sections: &[ApplyPatchSection],
) -> Result<TranslatedPatchPlan, ToolError> {
    let mut virtual_files = BTreeMap::<String, Option<String>>::new();
    let mut ops = Vec::new();
    let mut files = BTreeSet::new();
    let mut edits = Vec::new();

    for (section_index, section) in sections.iter().enumerate() {
        match section {
            ApplyPatchSection::Add { path, lines } => {
                let existing = read_virtual_file_state(ctx, &mut virtual_files, path)?;
                if existing.is_some() {
                    return Err(ToolError::Execution(format!(
                        "apply_patch verification failed: add file target already exists: {path}"
                    )));
                }

                let content = lines.join("\n");
                virtual_files.insert(path.clone(), Some(content.clone()));
                let edit_id = format!("apply-patch-{}-{section_index}", ctx.tool_call_id);
                ops.push(HashlineWorkspaceOp::RewriteFile {
                    edit_id: edit_id.clone(),
                    path: path.clone(),
                    content,
                });
                files.insert(format!("A {path}"));
                edits.push(TranslatedPatchEdit {
                    edit_id,
                    path: path.clone(),
                    summary: format!("apply patch add {path}"),
                    deleted: false,
                    diff_content: build_unified_diff(path, path, "", &lines.join("\n")),
                });
            }
            ApplyPatchSection::Delete { path } => {
                let existing = read_virtual_file_state(ctx, &mut virtual_files, path)?;
                let Some(existing) = existing else {
                    return Err(ToolError::Execution(format!(
                        "apply_patch verification failed: delete file target not found: {path}"
                    )));
                };

                virtual_files.insert(path.clone(), None);
                let edit_id = format!("apply-patch-{}-{section_index}", ctx.tool_call_id);
                ops.push(HashlineWorkspaceOp::DeleteFile {
                    edit_id: edit_id.clone(),
                    path: path.clone(),
                });
                files.insert(format!("D {path}"));
                edits.push(TranslatedPatchEdit {
                    edit_id,
                    path: path.clone(),
                    summary: format!("apply patch delete {path}"),
                    deleted: true,
                    diff_content: build_unified_diff(path, path, &existing, ""),
                });
            }
            ApplyPatchSection::Update {
                path,
                move_to,
                hunks,
            } => {
                let Some(mut source) = read_virtual_file_state(ctx, &mut virtual_files, path)?
                else {
                    return Err(ToolError::Execution(format!(
                        "apply_patch verification failed: update file target not found: {path}"
                    )));
                };
                let original_source = source.clone();

                if hunks.is_empty() && move_to.is_none() {
                    return Err(ToolError::Execution(
                        "apply_patch verification failed: update section has no hunks".to_string(),
                    ));
                }

                for (hunk_index, hunk) in hunks.iter().enumerate() {
                    let patch =
                        translate_update_hunk(ctx, section_index, hunk_index, path, &source, hunk)?;
                    let applied = apply_hashline_patch(&source, &patch).map_err(|err| {
                        ToolError::Execution(format!(
                            "hashline apply rejected [{}]: {err}",
                            err.code()
                        ))
                    })?;
                    source = applied.content;
                    ops.push(HashlineWorkspaceOp::Patch { patch });
                }
                virtual_files.insert(path.clone(), Some(source.clone()));

                if let Some(destination) = move_to {
                    if destination != path {
                        let existing_destination =
                            read_virtual_file_state(ctx, &mut virtual_files, destination)?;
                        if existing_destination.is_some() {
                            return Err(ToolError::Execution(format!(
                                "apply_patch verification failed: move destination already exists: {destination}"
                            )));
                        }

                        virtual_files.insert(path.clone(), None);
                        virtual_files.insert(destination.clone(), Some(source.clone()));
                        ops.push(HashlineWorkspaceOp::MoveFile {
                            edit_id: format!(
                                "apply-patch-{}-{section_index}-move",
                                ctx.tool_call_id
                            ),
                            from_path: path.clone(),
                            to_path: destination.clone(),
                        });
                        files.insert(format!("M {path} -> {destination}"));
                        edits.push(TranslatedPatchEdit {
                            edit_id: format!("apply-patch-{}-{section_index}", ctx.tool_call_id),
                            path: destination.clone(),
                            summary: format!("apply patch move {path} -> {destination}"),
                            deleted: false,
                            diff_content: build_unified_diff(
                                path,
                                destination,
                                &original_source,
                                &source,
                            ),
                        });
                    } else {
                        files.insert(format!("M {path}"));
                        edits.push(TranslatedPatchEdit {
                            edit_id: format!("apply-patch-{}-{section_index}", ctx.tool_call_id),
                            path: path.clone(),
                            summary: format!("apply patch update {path}"),
                            deleted: false,
                            diff_content: build_unified_diff(path, path, &original_source, &source),
                        });
                    }
                } else {
                    files.insert(format!("M {path}"));
                    edits.push(TranslatedPatchEdit {
                        edit_id: format!("apply-patch-{}-{section_index}", ctx.tool_call_id),
                        path: path.clone(),
                        summary: format!("apply patch update {path}"),
                        deleted: false,
                        diff_content: build_unified_diff(path, path, &original_source, &source),
                    });
                }
            }
        }
    }

    Ok(TranslatedPatchPlan {
        ops,
        files: files.into_iter().collect(),
        edits,
    })
}

fn build_unified_diff(from_path: &str, to_path: &str, before: &str, after: &str) -> String {
    format!(
        "--- {from_path}\n+++ {to_path}\n{}",
        TextDiff::from_lines(before, after).unified_diff()
    )
}

fn translate_update_hunk(
    ctx: &ToolContext,
    section_index: usize,
    hunk_index: usize,
    path: &str,
    source: &str,
    hunk: &[String],
) -> Result<HashlinePatch, ToolError> {
    let source_lines = source.lines().collect::<Vec<_>>();
    let old_lines = hunk
        .iter()
        .filter_map(|line| {
            if line.starts_with('-') || line.starts_with(' ') {
                Some(line[1..].to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let new_lines = hunk
        .iter()
        .filter_map(|line| {
            if line.starts_with('+') || line.starts_with(' ') {
                Some(line[1..].to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if old_lines.is_empty() {
        return Err(ToolError::Execution(
            "apply_patch translation currently requires at least one context or removed line"
                .to_string(),
        ));
    }

    let start_idx = match find_subsequence(&source_lines, &old_lines) {
        Some(start_idx) => start_idx,
        None => match find_relaxed_subsequence(&source_lines, &old_lines) {
            RelaxedSubsequenceMatch::Unique { start } => start,
            RelaxedSubsequenceMatch::Multiple => {
                return Err(ToolError::Execution(format!(
                    "apply_patch verification failed: hunk context not found exactly in {path}; multiple regions match when ignoring trailing spaces. Re-read the file and regenerate the patch against current contents, or switch to read(hashlineAnchors=true) / edit.hashline_scan plus edit.hashline_apply for a safer anchored edit"
                )));
            }
            RelaxedSubsequenceMatch::None => {
                return Err(ToolError::Execution(format!(
                    "apply_patch verification failed: hunk context not found in {path}. Re-read the file and regenerate the patch against current contents, or switch to read(hashlineAnchors=true) / edit.hashline_scan plus edit.hashline_apply for a safer anchored edit"
                )));
            }
        },
    };

    let expected = source_lines[start_idx..start_idx + old_lines.len()]
        .iter()
        .enumerate()
        .map(|(offset, line)| LineAnchor {
            line: (start_idx + offset + 1) as u32,
            hash: compute_line_hash(line),
        })
        .collect::<Vec<_>>();

    Ok(HashlinePatch {
        edit_id: format!(
            "apply-patch-{}-{section_index}-{hunk_index}",
            ctx.tool_call_id
        ),
        path: path.to_string(),
        ops: vec![HashlineOp::Replace {
            expected,
            lines: new_lines,
        }],
    })
}

fn read_virtual_file_state(
    ctx: &ToolContext,
    virtual_files: &mut BTreeMap<String, Option<String>>,
    path: &str,
) -> Result<Option<String>, ToolError> {
    if let Some(existing) = virtual_files.get(path) {
        return Ok(existing.clone());
    }

    let resolved = resolve_workspace_target_path(ctx, path)?;
    let content = match std::fs::read_to_string(&resolved) {
        Ok(content) => Some(content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            return Err(ToolError::Execution(format!(
                "failed to read file for patching: {err}"
            )));
        }
    };
    virtual_files.insert(path.to_string(), content.clone());
    Ok(content)
}

fn find_subsequence(haystack: &[&str], needle: &[String]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let needle_ref = needle.iter().map(String::as_str).collect::<Vec<_>>();
    haystack
        .windows(needle_ref.len())
        .position(|window| window == needle_ref.as_slice())
}

fn find_relaxed_subsequence(haystack: &[&str], needle: &[String]) -> RelaxedSubsequenceMatch {
    if needle.is_empty() || haystack.is_empty() || needle.len() > haystack.len() {
        return RelaxedSubsequenceMatch::None;
    }

    let mut found = None;
    for (start, window) in haystack.windows(needle.len()).enumerate() {
        let matches = window
            .iter()
            .zip(needle.iter())
            .all(|(src, expected)| trim_edit_match_line(src) == trim_edit_match_line(expected));
        if !matches {
            continue;
        }
        if found.replace(start).is_some() {
            return RelaxedSubsequenceMatch::Multiple;
        }
    }

    match found {
        Some(start) => RelaxedSubsequenceMatch::Unique { start },
        None => RelaxedSubsequenceMatch::None,
    }
}
