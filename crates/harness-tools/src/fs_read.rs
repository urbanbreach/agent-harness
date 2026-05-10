use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use harness_core::edit::hashline::{compute_line_hash, LineAnchor};
use harness_core::tool::{ArtifactRef, Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::read_window::{
    normalize_read_limit, normalize_read_offset, READ_DEFAULT_LIMIT, READ_DEFAULT_OFFSET,
};
use crate::workspace_edit::{record_file_hashline_read, record_file_read};
use crate::{parse_tool_args, text_json_artifacts_tool_result};

pub(crate) struct FsReadTool {
    default_hashline_anchors: bool,
}

impl FsReadTool {
    pub(crate) fn new(default_hashline_anchors: bool) -> Self {
        Self {
            default_hashline_anchors,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FsReadRenderOptions {
    line_numbers: bool,
    hashline_anchors: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FsReadArgs {
    path: String,
    #[serde(default = "default_fs_read_offset")]
    offset: u32,
    #[serde(default = "default_fs_read_limit")]
    limit: u32,
    #[serde(default = "default_fs_read_line_numbers")]
    line_numbers: bool,
    #[serde(default)]
    hashline_anchors: Option<bool>,
}

#[derive(Debug)]
struct FsReadRequest {
    path: String,
    offset: u32,
    limit: u32,
    render: FsReadRenderOptions,
}

impl FsReadArgs {
    fn into_request(self, default_hashline_anchors: bool) -> FsReadRequest {
        FsReadRequest {
            path: self.path,
            offset: normalize_read_offset(self.offset),
            limit: normalize_read_limit(self.limit),
            render: FsReadRenderOptions {
                line_numbers: self.line_numbers,
                hashline_anchors: self.hashline_anchors.unwrap_or(default_hashline_anchors),
            },
        }
    }
}

impl FsReadRequest {
    fn path(&self) -> &Path {
        Path::new(&self.path)
    }

    fn start_line_index(&self) -> usize {
        (self.offset - 1) as usize
    }

    fn line_limit(&self) -> usize {
        self.limit as usize
    }
}

fn default_fs_read_offset() -> u32 {
    READ_DEFAULT_OFFSET
}

fn default_fs_read_limit() -> u32 {
    READ_DEFAULT_LIMIT
}

fn default_fs_read_line_numbers() -> bool {
    true
}

fn fs_read_parameters_json_schema(default_hashline_anchors: bool) -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string"
            },
            "offset": {
                "type": "integer",
                "minimum": 1,
                "default": READ_DEFAULT_OFFSET
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "default": READ_DEFAULT_LIMIT
            },
            "line_numbers": {
                "type": "boolean",
                "default": true
            },
            "hashline_anchors": {
                "type": "boolean",
                "default": default_hashline_anchors,
                "description": "When true, render lines as LINE#HASH|text for anchor-driven edit workflows"
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

fn format_fs_read_line(line: &str, line_number: usize, line_numbers: bool) -> String {
    if line_numbers {
        format!("{line_number}: {line}")
    } else {
        line.to_string()
    }
}

fn format_fs_read_hashline_line(anchor: &LineAnchor, line: &str) -> String {
    format!(
        "{}#{}|{}",
        anchor.line,
        anchor.hash,
        fs_read_hashline_text(line)
    )
}

fn build_fs_read_line_anchor(line_number: usize, line: &str) -> LineAnchor {
    LineAnchor {
        line: line_number as u32,
        hash: compute_line_hash(line),
    }
}

fn fs_read_hashline_text(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

fn format_fs_read_output_line(
    line: &str,
    line_number: usize,
    render: FsReadRenderOptions,
) -> String {
    if render.hashline_anchors {
        let anchor = build_fs_read_line_anchor(line_number, line);
        format_fs_read_hashline_line(&anchor, line)
    } else {
        format_fs_read_line(line, line_number, render.line_numbers)
    }
}

fn append_truncation_marker(display_text: &mut String, marker: &str) {
    if display_text.is_empty() {
        display_text.push_str(marker);
        return;
    }

    if !display_text.ends_with('\n') {
        display_text.push('\n');
    }
    display_text.push_str(marker);
}

#[async_trait]
impl Tool for FsReadTool {
    fn id(&self) -> &str {
        "fs.read"
    }

    fn description(&self) -> &str {
        "Reads UTF-8 text from a workspace file with optional offset/limit and line numbers."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        fs_read_parameters_json_schema(self.default_hashline_anchors)
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let request =
            parse_tool_args::<FsReadArgs>(args_json)?.into_request(self.default_hashline_anchors);
        let resolved = ctx.resolve_workspace_path(request.path())?;
        let start_line_index = request.start_line_index();
        let line_limit = request.line_limit();
        let render = request.render;
        let read = read_fs_window(&resolved, start_line_index, line_limit, render)?;
        let display = build_fs_read_display(&ctx, &resolved, &read, start_line_index, render)?;
        let structured_json = build_fs_read_structured_json(&request, &resolved, &read);

        record_fs_read_access(&ctx, &resolved, read.anchors)?;

        Ok(text_json_artifacts_tool_result(
            display.text,
            structured_json,
            display.artifacts,
        ))
    }
}

fn record_fs_read_access(
    ctx: &ToolContext,
    resolved: &Path,
    anchors: Option<Vec<LineAnchor>>,
) -> Result<(), ToolError> {
    if let Some(anchors) = anchors {
        record_file_hashline_read(ctx, resolved, anchors)
    } else {
        record_file_read(ctx, resolved)
    }
}

struct FsReadDisplay {
    text: String,
    artifacts: Vec<ArtifactRef>,
}

fn build_fs_read_display(
    ctx: &ToolContext,
    resolved: &Path,
    read: &FsReadWindow,
    start_line_index: usize,
    render: FsReadRenderOptions,
) -> Result<FsReadDisplay, ToolError> {
    let mut text = read.display_text.clone();
    let mut artifacts = Vec::new();

    if read.truncated {
        let artifact = write_fs_read_artifact_streaming(ctx, resolved, start_line_index, render)?;
        append_fs_read_truncation_marker(&mut text, read, start_line_index, &artifact);
        artifacts.push(artifact);
    }

    Ok(FsReadDisplay { text, artifacts })
}

fn append_fs_read_truncation_marker(
    display_text: &mut String,
    read: &FsReadWindow,
    start_line_index: usize,
    artifact: &ArtifactRef,
) {
    let marker = format!(
        "... [truncated: showing {} of {} lines from line {}; full output: {}]",
        read.shown_lines.len(),
        read.available_lines,
        start_line_index + 1,
        artifact.path
    );
    append_truncation_marker(display_text, &marker);
}

fn build_fs_read_structured_json(
    request: &FsReadRequest,
    resolved: &Path,
    read: &FsReadWindow,
) -> serde_json::Value {
    let anchors = read
        .anchors
        .as_ref()
        .map(|anchors| build_fs_read_anchor_payload(anchors, &read.shown_lines))
        .unwrap_or(serde_json::Value::Null);

    json!({
        "path": request.path.as_str(),
        "resolved_path": resolved.display().to_string(),
        "offset": request.offset,
        "limit": request.limit,
        "total_lines": read.total_lines,
        "line_numbers": request.render.line_numbers,
        "hashline_anchors": request.render.hashline_anchors,
        "anchors": anchors,
        "truncated": read.truncated,
    })
}

#[derive(Debug)]
struct FsReadWindow {
    shown_lines: Vec<String>,
    display_text: String,
    anchors: Option<Vec<LineAnchor>>,
    total_lines: usize,
    available_lines: usize,
    truncated: bool,
}

struct FsReadWindowBuilder {
    shown_lines: Vec<String>,
    display_parts: Vec<String>,
    anchors: Option<Vec<LineAnchor>>,
    line_limit: usize,
    render: FsReadRenderOptions,
}

impl FsReadWindowBuilder {
    fn new(line_limit: usize, render: FsReadRenderOptions) -> Self {
        Self {
            shown_lines: Vec::new(),
            display_parts: Vec::new(),
            anchors: render.hashline_anchors.then(Vec::new),
            line_limit,
            render,
        }
    }

    fn push_visible_line(&mut self, line_number: usize, line: String) {
        if self.shown_lines.len() >= self.line_limit {
            return;
        }

        let rendered = self.render_visible_line(line_number, &line);
        self.shown_lines.push(line);
        self.display_parts.push(rendered);
    }

    fn render_visible_line(&mut self, line_number: usize, line: &str) -> String {
        match self.anchors.as_mut() {
            Some(anchors) => {
                let anchor = build_fs_read_line_anchor(line_number, line);
                let rendered = format_fs_read_hashline_line(&anchor, line);
                anchors.push(anchor);
                rendered
            }
            None => format_fs_read_line(line, line_number, self.render.line_numbers),
        }
    }

    fn finish(self, total_lines: usize, available_lines: usize) -> FsReadWindow {
        FsReadWindow {
            shown_lines: self.shown_lines,
            display_text: self.display_parts.join("\n"),
            anchors: self.anchors,
            total_lines,
            available_lines,
            truncated: available_lines > self.line_limit,
        }
    }
}

fn read_fs_window(
    path: &Path,
    start_line_index: usize,
    line_limit: usize,
    render: FsReadRenderOptions,
) -> Result<FsReadWindow, ToolError> {
    let mut available_lines = 0usize;
    let mut window = FsReadWindowBuilder::new(line_limit, render);
    let mut reader = open_fs_read_file(path)?;

    let total_lines = visit_fs_read_lines(&mut reader, start_line_index, |line_number, line| {
        available_lines += 1;
        window.push_visible_line(line_number, line);
        Ok(())
    })?;

    Ok(window.finish(total_lines, available_lines))
}

fn write_fs_read_artifact_streaming(
    ctx: &ToolContext,
    path: &Path,
    start_line_index: usize,
    render: FsReadRenderOptions,
) -> Result<ArtifactRef, ToolError> {
    let target = prepare_fs_read_artifact_target(ctx)?;
    let mut reader = open_fs_read_file(path)?;
    let artifact = std::fs::File::create(&target.file_path)
        .map_err(|err| ToolError::Execution(format!("failed to write fs.read artifact: {err}")))?;
    let mut artifact_writer = FsReadArtifactWriter::new(artifact);

    visit_fs_read_lines(&mut reader, start_line_index, |line_number, line| {
        let rendered = format_fs_read_output_line(&line, line_number, render);
        artifact_writer.write_rendered_line(&rendered)
    })?;

    Ok(ArtifactRef {
        path: target.artifact_path,
        digest: None,
    })
}

struct FsReadArtifactTarget {
    file_path: PathBuf,
    artifact_path: String,
}

fn prepare_fs_read_artifact_target(ctx: &ToolContext) -> Result<FsReadArtifactTarget, ToolError> {
    let relative = format!("toolcalls/{}/fs.read.redacted.txt", ctx.tool_call_id);
    let file_path = ctx.artifacts_dir.join(&relative);
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            ToolError::Execution(format!(
                "failed to create fs.read artifact directory: {err}"
            ))
        })?;
    }

    Ok(FsReadArtifactTarget {
        file_path,
        artifact_path: format!("artifacts/{relative}"),
    })
}

struct FsReadArtifactWriter<W> {
    artifact: W,
    wrote_any: bool,
}

impl<W: Write> FsReadArtifactWriter<W> {
    fn new(artifact: W) -> Self {
        Self {
            artifact,
            wrote_any: false,
        }
    }

    fn write_rendered_line(&mut self, rendered: &str) -> Result<(), ToolError> {
        if self.wrote_any {
            self.write_bytes(b"\n")?;
        }
        self.write_bytes(rendered.as_bytes())?;
        self.wrote_any = true;
        Ok(())
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), ToolError> {
        self.artifact
            .write_all(bytes)
            .map_err(|err| ToolError::Execution(format!("failed to write fs.read artifact: {err}")))
    }
}

fn open_fs_read_file(path: &Path) -> Result<std::io::BufReader<std::fs::File>, ToolError> {
    let file = std::fs::File::open(path)
        .map_err(|err| ToolError::Execution(format!("failed to read file: {err}")))?;
    Ok(std::io::BufReader::new(file))
}

fn visit_fs_read_lines(
    reader: &mut impl BufRead,
    start_line_index: usize,
    mut visit: impl FnMut(usize, String) -> Result<(), ToolError>,
) -> Result<usize, ToolError> {
    let mut raw_line = Vec::new();
    let mut total_lines = 0usize;

    loop {
        raw_line.clear();
        let bytes_read = reader
            .read_until(b'\n', &mut raw_line)
            .map_err(|err| ToolError::Execution(format!("failed to read file: {err}")))?;
        if bytes_read == 0 {
            break;
        }

        total_lines += 1;
        let line = decode_fs_read_line(&raw_line)?;
        if total_lines > start_line_index {
            visit(total_lines, line)?;
        }
    }

    Ok(total_lines)
}

fn decode_fs_read_line(raw_line: &[u8]) -> Result<String, ToolError> {
    let line = strip_fs_read_line_terminator(raw_line);
    String::from_utf8(line.to_vec())
        .map_err(|_| ToolError::Execution("binary file not supported".to_string()))
}

fn strip_fs_read_line_terminator(raw_line: &[u8]) -> &[u8] {
    let Some(without_lf) = raw_line.strip_suffix(b"\n") else {
        return raw_line;
    };
    without_lf.strip_suffix(b"\r").unwrap_or(without_lf)
}

fn build_fs_read_anchor_payload(anchors: &[LineAnchor], lines: &[String]) -> serde_json::Value {
    debug_assert_eq!(anchors.len(), lines.len());

    json!(anchors
        .iter()
        .zip(lines)
        .map(|(anchor, line)| {
            json!({
                "line": anchor.line,
                "hash": anchor.hash,
                "text": fs_read_hashline_text(line),
            })
        })
        .collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator_registry;
    use crate::test_support::{
        read_spilled_artifact, tool_context as fs_read_context, write_workspace_file,
    };
    use harness_core::config::ShellAllowlist;
    use harness_core::tool::{Tool, ToolError};
    use serde_json::json;

    #[tokio::test]
    async fn fs_read_supports_offset_and_limit_with_line_numbers() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_workspace_file(
            temp.path(),
            "fixture.txt",
            "line one\nline two\nline three\n",
        );

        let tool = FsReadTool::new(false);
        let result = tool
            .call(
                fs_read_context(temp.path(), "toolcall-offset-limit"),
                json!({
                    "path": "fixture.txt",
                    "offset": 2,
                    "limit": 2
                }),
            )
            .await
            .expect("fs.read should succeed");

        assert_eq!(result.display_text, "2: line two\n3: line three");
        assert!(result.artifacts.is_empty());

        let metadata = result.structured_json.expect("structured json");
        assert_eq!(metadata["offset"], json!(2));
        assert_eq!(metadata["limit"], json!(2));
        assert_eq!(metadata["total_lines"], json!(3));
        assert_eq!(metadata["truncated"], json!(false));
    }

    #[tokio::test]
    async fn fs_read_can_render_hashline_anchors() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_workspace_file(temp.path(), "fixture.txt", "alpha\nbeta\n");

        let tool = FsReadTool::new(false);
        let result = tool
            .call(
                fs_read_context(temp.path(), "toolcall-hashline-read"),
                json!({
                    "path": "fixture.txt",
                    "hashline_anchors": true,
                }),
            )
            .await
            .expect("fs.read hashline mode should succeed");

        assert_eq!(
            result.display_text,
            format!(
                "1#{}|alpha\n2#{}|beta",
                compute_line_hash("alpha"),
                compute_line_hash("beta")
            )
        );

        let metadata = result.structured_json.expect("structured json");
        assert_eq!(metadata["hashline_anchors"], json!(true));
        assert_eq!(metadata["anchors"][0]["line"], json!(1));
        assert_eq!(
            metadata["anchors"][0]["hash"],
            json!(compute_line_hash("alpha"))
        );
        assert_eq!(metadata["anchors"][0]["text"], json!("alpha"));
        assert_eq!(metadata["anchors"][1]["line"], json!(2));
        assert_eq!(
            metadata["anchors"][1]["hash"],
            json!(compute_line_hash("beta"))
        );
        assert_eq!(metadata["anchors"][1]["text"], json!("beta"));
    }

    #[tokio::test]
    async fn fs_read_strips_crlf_line_endings() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_workspace_file(temp.path(), "fixture.txt", "alpha\r\nbeta\r\n");

        let tool = FsReadTool::new(false);
        let result = tool
            .call(
                fs_read_context(temp.path(), "toolcall-crlf-read"),
                json!({
                    "path": "fixture.txt",
                }),
            )
            .await
            .expect("fs.read should read CRLF text");

        assert_eq!(result.display_text, "1: alpha\n2: beta");
    }

    #[tokio::test]
    async fn fs_read_normalizes_zero_offset_and_limit_to_defaults() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_workspace_file(
            temp.path(),
            "fixture.txt",
            "line one\nline two\nline three\n",
        );

        let tool = FsReadTool::new(false);
        let result = tool
            .call(
                fs_read_context(temp.path(), "toolcall-zero-paging"),
                json!({
                    "path": "fixture.txt",
                    "offset": 0,
                    "limit": 0
                }),
            )
            .await
            .expect("fs.read should normalize zero paging values");

        assert_eq!(
            result.display_text,
            "1: line one\n2: line two\n3: line three"
        );

        let metadata = result.structured_json.expect("structured json");
        assert_eq!(metadata["offset"], json!(1));
        assert_eq!(metadata["limit"], json!(READ_DEFAULT_LIMIT));
        assert_eq!(metadata["hashline_anchors"], json!(false));
    }

    #[tokio::test]
    async fn read_tool_normalizes_zero_offset_and_limit_for_model_compatibility() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_workspace_file(
            temp.path(),
            "fixture.txt",
            "line one\nline two\nline three\n",
        );

        let registry = coordinator_registry(ShellAllowlist::default());
        let read = registry.get("read").expect("read tool");
        let result = read
            .call(
                fs_read_context(temp.path(), "toolcall-read-zero-paging"),
                json!({
                    "filePath": "fixture.txt",
                    "offset": 0,
                    "limit": 0
                }),
            )
            .await
            .expect("read should normalize zero paging values");

        assert_eq!(
            result.display_text,
            format!(
                "1#{}|line one\n2#{}|line two\n3#{}|line three",
                compute_line_hash("line one"),
                compute_line_hash("line two"),
                compute_line_hash("line three")
            )
        );

        let metadata = result.structured_json.expect("structured json");
        assert_eq!(metadata["offset"], json!(1));
        assert_eq!(metadata["limit"], json!(READ_DEFAULT_LIMIT));
        assert_eq!(metadata["hashline_anchors"], json!(true));
    }

    #[tokio::test]
    async fn read_tool_exposes_hashline_anchor_mode_for_model_workflows() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_workspace_file(temp.path(), "fixture.txt", "alpha\nbeta\n");

        let registry = coordinator_registry(ShellAllowlist::default());
        let read = registry.get("read").expect("read tool");
        let result = read
            .call(
                fs_read_context(temp.path(), "toolcall-read-hashline"),
                json!({
                    "filePath": "fixture.txt",
                    "hashlineAnchors": true,
                }),
            )
            .await
            .expect("read hashline mode should succeed");

        assert!(result.display_text.contains("1#"));
        assert!(result.display_text.contains("|alpha"));
        assert!(result.display_text.contains("2#"));
        assert!(result.display_text.contains("|beta"));

        let metadata = result.structured_json.expect("structured json");
        assert_eq!(metadata["hashline_anchors"], json!(true));
        assert_eq!(metadata["anchors"][0]["line"], json!(1));
        assert_eq!(metadata["anchors"][1]["line"], json!(2));
    }

    #[tokio::test]
    async fn read_tool_accepts_absolute_workspace_paths() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = write_workspace_file(temp.path(), "fixture.txt", "alpha\nbeta\n");

        let registry = coordinator_registry(ShellAllowlist::default());
        let read = registry.get("read").expect("read tool");
        let result = read
            .call(
                fs_read_context(temp.path(), "toolcall-read-absolute"),
                json!({
                    "filePath": source,
                }),
            )
            .await
            .expect("absolute workspace path should read successfully");

        assert!(result.display_text.contains("|alpha"));
        assert!(result.display_text.contains("|beta"));
        let metadata = result.structured_json.expect("structured json");
        assert!(metadata["resolved_path"]
            .as_str()
            .expect("resolved path string")
            .ends_with("fixture.txt"));
    }

    #[tokio::test]
    async fn read_tool_rejects_absolute_paths_outside_workspace() {
        let workspace = tempfile::tempdir().expect("workspace tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        let source = outside.path().join("escape.txt");
        std::fs::write(&source, "blocked\n").expect("write outside fixture");

        let registry = coordinator_registry(ShellAllowlist::default());
        let read = registry.get("read").expect("read tool");
        let error = read
            .call(
                fs_read_context(workspace.path(), "toolcall-read-absolute-escape"),
                json!({
                    "filePath": source,
                }),
            )
            .await
            .expect_err("absolute path outside workspace should be rejected");

        assert!(matches!(error, ToolError::PathEscapesWorkspace { .. }));
    }

    #[tokio::test]
    async fn fs_read_adds_truncation_marker_and_spills_full_output_artifact() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_workspace_file(temp.path(), "fixture.txt", "alpha\nbeta\ngamma\ndelta\n");

        let context = fs_read_context(temp.path(), "toolcall-truncated");
        let tool = FsReadTool::new(false);
        let result = tool
            .call(
                context.clone(),
                json!({
                    "path": "fixture.txt",
                    "limit": 2
                }),
            )
            .await
            .expect("fs.read should succeed");

        assert!(result.display_text.contains("1: alpha\n2: beta"));
        assert!(result.display_text.contains("[truncated:"));
        assert_eq!(result.artifacts.len(), 1);

        let metadata = result.structured_json.expect("structured json");
        assert_eq!(metadata["truncated"], json!(true));
        assert_eq!(metadata["total_lines"], json!(4));

        let spilled = read_spilled_artifact(&context, &result.artifacts[0].path);
        assert!(spilled.contains("1: alpha"));
        assert!(spilled.contains("4: delta"));
    }

    #[tokio::test]
    async fn fs_read_rejects_binary_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_workspace_file(temp.path(), "fixture.bin", [0xff_u8, 0xfe, 0x00]);

        let tool = FsReadTool::new(false);
        let error = tool
            .call(
                fs_read_context(temp.path(), "toolcall-binary"),
                json!({
                    "path": "fixture.bin"
                }),
            )
            .await
            .expect_err("fs.read should fail for binary");

        match error {
            ToolError::Execution(message) => assert_eq!(message, "binary file not supported"),
            other => panic!("unexpected error variant: {other}"),
        }
    }
}
