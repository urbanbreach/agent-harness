use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use harness_core::edit::hashline::LineAnchor;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::hashline_apply::{resolve_workspace_target_path, write_file_via_hashline_engine};

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
    hashline_anchors: Option<Vec<LineAnchor>>,
}

#[derive(Default)]
struct FileReadRegistry {
    reads: BTreeMap<(String, String), FileReadStamp>,
    locks: BTreeMap<String, Arc<Mutex<()>>>,
}

static FILE_READ_REGISTRY: OnceLock<Mutex<FileReadRegistry>> = OnceLock::new();

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
}

pub(crate) struct FsWriteTool {
    executor: Arc<WorkspaceEditExecutor>,
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

pub(crate) fn record_file_read(ctx: &ToolContext, resolved_path: &Path) -> Result<(), ToolError> {
    record_file_read_with_anchors(ctx, resolved_path, None)
}

pub(crate) fn record_file_hashline_read(
    ctx: &ToolContext,
    resolved_path: &Path,
    anchors: Vec<LineAnchor>,
) -> Result<(), ToolError> {
    record_file_read_with_anchors(ctx, resolved_path, Some(anchors))
}

pub(crate) fn recent_hashline_anchors(
    ctx: &ToolContext,
    resolved_path: &Path,
) -> Option<Vec<LineAnchor>> {
    let key = file_read_key(ctx, resolved_path);
    let prior = {
        let registry = file_read_registry();
        let state = lock_mutex(registry);
        state.reads.get(&key).cloned()
    }?;

    let anchors = prior.hashline_anchors?;
    let current = file_read_stamp_from_metadata(resolved_path).ok()?;
    (current.fingerprint == prior.fingerprint).then_some(anchors)
}

fn record_file_read_with_anchors(
    ctx: &ToolContext,
    resolved_path: &Path,
    anchors: Option<Vec<LineAnchor>>,
) -> Result<(), ToolError> {
    let stamp = file_read_stamp_from_metadata(resolved_path)?;
    let key = file_read_key(ctx, resolved_path);
    let registry = file_read_registry();
    let mut state = lock_mutex(registry);
    state.reads.insert(
        key,
        FileReadStamp {
            hashline_anchors: anchors,
            ..stamp
        },
    );
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
        hashline_anchors: None,
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
