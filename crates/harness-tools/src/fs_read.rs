use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use harness_core::edit::hashline::LineAnchor;
use harness_core::tool::{
    ArtifactRef, Tool, ToolCapability, ToolContext, ToolError, ToolResult, ToolResultContent,
};
use serde_json::json;

use crate::workspace_edit::{record_file_hashline_read, record_file_read};
use crate::{parse_tool_args, text_json_artifacts_tool_result};

mod args;
mod render;
#[cfg(test)]
mod tests;
mod window;

use args::{fs_read_parameters_json_schema, FsReadArgs, FsReadRequest};
use render::{
    append_truncation_marker, build_fs_read_anchor_payload, build_fs_read_line_anchor,
    format_fs_read_hashline_line, format_fs_read_output_line, truncate_fs_read_line,
    FsReadRenderOptions,
};
use window::{
    open_fs_read_file, read_fs_window, visit_fs_read_lines, FsReadWindow, MAX_READ_RENDER_BYTES,
};

const MEDIA_SAMPLE_BYTES: usize = 4096;
const SUPPORTED_IMAGE_MIMES: [&str; 4] = ["image/jpeg", "image/png", "image/gif", "image/webp"];
const PDF_MIME: &str = "application/pdf";

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
        if let Some(result) = build_fs_read_media_result(&request, &resolved)? {
            record_fs_read_access(&ctx, &resolved, None)?;
            return Ok(result);
        }

        let start_line_index = request.start_line_index();
        let line_limit = request.line_limit();
        let render = request.render;
        let read = read_fs_window(
            &resolved,
            start_line_index,
            line_limit,
            MAX_READ_RENDER_BYTES,
            render,
        )
        .map_err(|err| map_fs_read_error(err, &resolved))?;
        let display = build_fs_read_display(&ctx, &resolved, &read, start_line_index, render)?;
        let structured_json =
            build_fs_read_structured_json(&request, &resolved, &read, display.artifacts.first());

        record_fs_read_access(&ctx, &resolved, read.anchors)?;

        Ok(text_json_artifacts_tool_result(
            display.text,
            structured_json,
            display.artifacts,
        ))
    }
}

struct FsReadMediaResult {
    message: &'static str,
    mime: String,
    data_url: String,
    file_name: Option<String>,
}

fn build_fs_read_media_result(
    request: &FsReadRequest,
    resolved: &Path,
) -> Result<Option<ToolResult>, ToolError> {
    let sample = read_fs_read_media_sample(resolved)?;
    let mime = sniff_fs_read_attachment_mime(&sample, resolved);
    let message = if is_supported_image_mime(&mime) {
        "Image read successfully"
    } else if mime == PDF_MIME {
        "PDF read successfully"
    } else {
        return Ok(None);
    };

    let bytes = std::fs::read(resolved)
        .map_err(|err| ToolError::Execution(format!("failed to read file: {err}")))?;
    let media = FsReadMediaResult {
        message,
        data_url: format!("data:{mime};base64,{}", STANDARD.encode(bytes)),
        mime,
        file_name: resolved
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string),
    };
    let structured_json = build_fs_read_media_structured_json(request, resolved, &media);

    Ok(Some(
        ToolResult::structured(media.message, structured_json).with_provider_content(vec![
            ToolResultContent::text(media.message),
            ToolResultContent::file(media.data_url, media.mime, media.file_name),
        ]),
    ))
}

fn read_fs_read_media_sample(path: &Path) -> Result<Vec<u8>, ToolError> {
    let mut file = std::fs::File::open(path)
        .map_err(|err| ToolError::Execution(format!("failed to read file: {err}")))?;
    let mut buffer = vec![0_u8; MEDIA_SAMPLE_BYTES];
    let bytes_read = file
        .read(&mut buffer)
        .map_err(|err| ToolError::Execution(format!("failed to read file: {err}")))?;
    buffer.truncate(bytes_read);
    Ok(buffer)
}

fn build_fs_read_media_structured_json(
    request: &FsReadRequest,
    resolved: &Path,
    media: &FsReadMediaResult,
) -> serde_json::Value {
    json!({
        "title": request.path.as_str(),
        "path": request.path.as_str(),
        "resolved_path": resolved.display().to_string(),
        "metadata": {
            "preview": media.message,
            "truncated": false,
            "loaded": [],
        },
        "attachments": [{
            "type": "file",
            "mime": media.mime,
            "url": media.data_url,
        }],
    })
}

fn sniff_fs_read_attachment_mime(bytes: &[u8], path: &Path) -> String {
    for (prefix, mime) in [
        (
            &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a][..],
            "image/png",
        ),
        (&[0xff, 0xd8, 0xff][..], "image/jpeg"),
        (&[0x47, 0x49, 0x46, 0x38][..], "image/gif"),
        (&[0x25, 0x50, 0x44, 0x46, 0x2d][..], PDF_MIME),
    ] {
        if bytes.starts_with(prefix) {
            return mime.to_string();
        }
    }

    if bytes.starts_with(&[0x52, 0x49, 0x46, 0x46]) && bytes.get(8..12) == Some(&b"WEBP"[..]) {
        return "image/webp".to_string();
    }

    mime_from_path(path)
        .unwrap_or("application/octet-stream")
        .to_string()
}

fn mime_from_path(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "pdf" => Some(PDF_MIME),
        _ => None,
    }
}

fn is_supported_image_mime(mime: &str) -> bool {
    SUPPORTED_IMAGE_MIMES.contains(&mime)
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
    let mut content = read.display_text.clone();
    let mut artifacts = Vec::new();

    if read.truncated {
        let artifact = write_fs_read_artifact_streaming(ctx, resolved, start_line_index, render)?;
        append_fs_read_truncation_marker(&mut content, read, start_line_index, &artifact);
        artifacts.push(artifact);
    } else {
        append_fs_read_end_marker(&mut content, read);
    }

    let text = format!(
        "<path>{}</path>\n<type>file</type>\n<content>\n{}\n</content>",
        resolved.display(),
        content
    );

    Ok(FsReadDisplay { text, artifacts })
}

fn append_fs_read_truncation_marker(
    display_text: &mut String,
    read: &FsReadWindow,
    start_line_index: usize,
    artifact: &ArtifactRef,
) {
    let first_line = start_line_index + 1;
    let last_line = start_line_index + read.shown_lines.len();
    let next_offset = last_line + 1;
    let marker = format!(
        "\n(Showing lines {first_line}-{last_line} of {}. Use offset={next_offset} to continue. full output artifact: {})",
        read.total_lines, artifact.path
    );
    append_truncation_marker(display_text, &marker);
}

fn append_fs_read_end_marker(display_text: &mut String, read: &FsReadWindow) {
    let marker = format!("\n(End of file - total {} lines)", read.total_lines);
    append_truncation_marker(display_text, &marker);
}

fn build_fs_read_structured_json(
    request: &FsReadRequest,
    resolved: &Path,
    read: &FsReadWindow,
    artifact: Option<&ArtifactRef>,
) -> serde_json::Value {
    let anchors = read
        .anchors
        .as_ref()
        .map(|anchors| build_fs_read_anchor_payload(anchors, &read.shown_lines))
        .unwrap_or(serde_json::Value::Null);

    json!({
        "title": request.path.as_str(),
        "path": request.path.as_str(),
        "resolved_path": resolved.display().to_string(),
        "offset": request.offset,
        "limit": request.limit,
        "next_offset": read.truncated.then_some(request.offset as usize + read.shown_lines.len()),
        "total_lines": read.total_lines,
        "line_numbers": request.render.line_numbers,
        "hashline_anchors": request.render.hashline_anchors,
        "anchors": anchors,
        "truncated": read.truncated,
        "metadata": {
            "preview": read.shown_lines.iter().take(20).cloned().collect::<Vec<_>>().join("\n"),
            "truncated": read.truncated,
            "loaded": [],
            "display": {
                "type": "file",
                "path": resolved.display().to_string(),
                "text": read.shown_lines.join("\n"),
                "lineStart": request.offset,
                "lineEnd": request.offset as usize + read.shown_lines.len().saturating_sub(1),
                "totalLines": read.total_lines,
                "truncated": read.truncated,
            }
        },
        "output_artifact": artifact.map(|artifact| json!({
            "path": artifact.path,
            "digest": artifact.digest,
        })),
    })
}

fn map_fs_read_error(err: ToolError, resolved: &Path) -> ToolError {
    match err {
        ToolError::Execution(message) if message == "binary file not supported" => {
            ToolError::Execution(format!("Cannot read binary file: {}", resolved.display()))
        }
        other => other,
    }
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
        let visible_line = truncate_fs_read_line(&line);
        let rendered = if render.hashline_anchors {
            let anchor = build_fs_read_line_anchor(line_number, &line);
            format_fs_read_hashline_line(&anchor, &visible_line)
        } else {
            format_fs_read_output_line(&visible_line, line_number, render)
        };
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
