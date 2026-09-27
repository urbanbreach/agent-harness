pub(crate) mod edit;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub(crate) const MAX_FILE: u64 = 8 * 1024 * 1024;
const MAX_DISPLAY: usize = 64 * 1024;
#[derive(Clone, Copy)]
pub(crate) enum FileTool {
    Read,
    Write,
    Edit,
    List,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    #[serde(rename = "filePath", alias = "file_path", alias = "path")]
    path: PathBuf,
    #[serde(default = "first_line")]
    offset: usize,
    #[serde(default = "line_limit")]
    limit: usize,
}
fn first_line() -> usize {
    1
}
fn line_limit() -> usize {
    2000
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    #[serde(default = "current_dir")]
    path: PathBuf,
}
fn current_dir() -> PathBuf {
    ".".into()
}
fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, ToolError> {
    serde_json::from_value(value).map_err(|e| ToolError::InvalidArguments(e.to_string()))
}
#[async_trait::async_trait]
impl Tool for FileTool {
    fn id(&self) -> &str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Edit => "edit",
            Self::List => "list",
        }
    }
    fn description(&self) -> &str {
        match self {
            Self::Read => "Read UTF-8 text with LINE#HASH anchors, return a PNG/JPEG/GIF/WebP image, or retain a PDF artifact without extracting its contents. Text offset is 1-based; output is bounded.",
            Self::Write => "Create a UTF-8 file or replace a file that has been read since its last change.",
            Self::Edit => "Edit a previously read file using oldString/newString or edits with op (replace/append/prepend), pos/end LINE#HASH anchors, and lines. Ambiguous exact matches require replaceAll.",
            Self::List => "List the immediate children of a directory in name order.",
        }
    }
    fn parameters_json_schema(&self) -> Value {
        match self {
            Self::Read => schemars::schema_for!(ReadArgs).to_value(),
            Self::Write => schemars::schema_for!(edit::WriteArgs).to_value(),
            Self::Edit => schemars::schema_for!(edit::EditArgs).to_value(),
            Self::List => schemars::schema_for!(ListArgs).to_value(),
        }
    }
    fn capability(&self) -> ToolCapability {
        match self {
            Self::Write | Self::Edit => ToolCapability::EditFs,
            _ => ToolCapability::ReadFs,
        }
    }
    fn filesystem_paths(&self, args: &Value) -> Result<Vec<PathBuf>, ToolError> {
        let path = match self {
            Self::Read => parse::<ReadArgs>(args.clone())?.path,
            Self::Write => parse::<edit::WriteArgs>(args.clone())?.path,
            Self::Edit => parse::<edit::EditArgs>(args.clone())?.path,
            Self::List => parse::<ListArgs>(args.clone())?.path,
        };
        Ok(vec![path])
    }
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        let permission = match self {
            Self::Write | Self::Edit => "edit",
            _ => "read",
        };
        let path = ["filePath", "file_path", "path"]
            .into_iter()
            .find_map(|k| args.get(k).and_then(Value::as_str))
            .unwrap_or(".");
        vec![(permission.into(), path.into())]
    }
    async fn call(&self, context: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let tool = *self;
        tokio::task::spawn_blocking(move || {
            if context.cancellation.is_cancelled() {
                return Err(ToolError::Cancelled);
            }
            match tool {
                Self::Read => read(&context, parse(args)?),
                Self::Write => edit::write(&context, parse(args)?),
                Self::Edit => edit::edit(&context, parse(args)?),
                Self::List => list(&context, parse(args)?),
            }
        })
        .await
        .map_err(|_| ToolError::Execution("file operation stopped unexpectedly".into()))?
    }
}
pub(crate) fn contents(path: &Path) -> Result<String, ToolError> {
    String::from_utf8(bytes(path)?)
        .map_err(|_| ToolError::InvalidArguments("file is not UTF-8 text".into()))
}
fn bytes(path: &Path) -> Result<Vec<u8>, ToolError> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_FILE {
        return Err(ToolError::InvalidArguments(
            "path must be a regular file of at most 8 MiB".into(),
        ));
    }
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(ToolError::InvalidArguments(
            "path is not a regular file".into(),
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FILE {
        return Err(ToolError::Execution("file exceeds the 8 MiB limit".into()));
    }
    Ok(bytes)
}
fn read(ctx: &ToolContext, args: ReadArgs) -> Result<ToolResult, ToolError> {
    if args.offset == 0 || args.limit == 0 {
        return Err(ToolError::InvalidArguments(
            "offset and limit must be positive".into(),
        ));
    }
    let path = ctx.resolve_workspace_path(&args.path)?;
    let bytes = bytes(&path)?;
    if let Some(mime) = crate::media::mime(&bytes) {
        if mime == "application/pdf" {
            let metadata = json!({"path":args.path,"mime":mime,"bytes":bytes.len()});
            return crate::media::artifact(ctx, bytes, metadata);
        }
        let attachment = crate::media::attachment(ctx.tool_call_id.to_string(), mime, &bytes)?;
        return Ok(ToolResult::structured(
            "Image read successfully.",
            json!({"path":args.path,"mime":mime,"bytes":bytes.len(),"kind":"image"}),
        )
        .with_attachments(vec![attachment]));
    }
    let text = String::from_utf8(bytes).map_err(|_| {
        ToolError::InvalidArguments("file is not UTF-8 text, PDF, or a supported image".into())
    })?;
    ctx.tool_state.record_read(&path, digest(&text))?;
    let mut output = String::new();
    let mut displayed = 0;
    let mut anchors = Vec::new();
    let mut truncated = false;
    for (index, line) in text
        .strip_prefix('\u{feff}')
        .unwrap_or(&text)
        .lines()
        .enumerate()
        .skip(args.offset - 1)
    {
        if displayed >= args.limit.min(2000) || output.len() >= MAX_DISPLAY {
            truncated = true;
            break;
        }
        let hash = crate::hashline::hash(line);
        let numbered = format!("{}#{hash}|{line}\n", index + 1);
        let end = numbered.floor_char_boundary((MAX_DISPLAY - output.len()).min(numbered.len()));
        output.push_str(&numbered[..end]);
        displayed += 1;
        if end < numbered.len() {
            truncated = true;
            break;
        }
        anchors.push(json!({"line":index + 1, "hash":hash}));
    }
    if truncated {
        output.push_str("\n[Output truncated; request a narrower range.]\n");
    }
    Ok(ToolResult::structured(
        output,
        json!({"path": args.path, "offset": args.offset, "lines": displayed, "anchors":anchors, "truncated": truncated}),
    ))
}
fn digest(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}
fn list(ctx: &ToolContext, args: ListArgs) -> Result<ToolResult, ToolError> {
    let path = ctx.resolve_workspace_path(&args.path)?;
    let mut entries = std::collections::BTreeSet::new();
    let mut truncated = false;
    for entry in fs::read_dir(path)? {
        if ctx.cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        entries.insert(entry?.file_name().to_string_lossy().into_owned());
        if entries.len() > 1000 {
            entries.pop_last();
            truncated = true;
        }
    }
    let entries: Vec<_> = entries.into_iter().collect();
    Ok(ToolResult::structured(
        entries.join("\n"),
        json!({"entries": entries, "truncated": truncated}),
    ))
}
