use std::io::BufRead;
use std::path::Path;

use harness_core::edit::hashline::LineAnchor;
use harness_core::tool::ToolError;
use harness_core::ToolResultExt;

use super::render::{
    build_fs_read_line_anchor, format_fs_read_hashline_line, format_fs_read_output_line,
    truncate_fs_read_line, FsReadRenderOptions,
};

pub(super) const MAX_READ_RENDER_BYTES: usize = 50 * 1024;

pub(super) struct FsReadWindow {
    pub(super) shown_lines: Vec<String>,
    pub(super) display_text: String,
    pub(super) anchors: Option<Vec<LineAnchor>>,
    pub(super) total_lines: usize,
    pub(super) truncated: bool,
}

struct FsReadWindowBuilder {
    shown_lines: Vec<String>,
    display_parts: Vec<String>,
    anchors: Option<Vec<LineAnchor>>,
    line_limit: usize,
    max_bytes: usize,
    display_bytes: usize,
    content_truncated: bool,
    render: FsReadRenderOptions,
}

impl FsReadWindowBuilder {
    fn new(line_limit: usize, max_bytes: usize, render: FsReadRenderOptions) -> Self {
        Self {
            shown_lines: Vec::new(),
            display_parts: Vec::new(),
            anchors: render.hashline_anchors.then(Vec::new),
            line_limit,
            max_bytes,
            display_bytes: 0,
            content_truncated: false,
            render,
        }
    }

    fn push_visible_line(&mut self, line_number: usize, line: String) {
        if self.shown_lines.len() >= self.line_limit || self.content_truncated {
            return;
        }

        let visible_line = truncate_fs_read_line(&line).into_owned();
        let rendered = self.render_visible_line(line_number, &line, &visible_line);
        let separator_bytes = if self.display_parts.is_empty() { 0 } else { 1 };
        let rendered_bytes = rendered.len();

        if self.display_bytes + separator_bytes + rendered_bytes > self.max_bytes {
            if self.display_parts.is_empty() {
                let allowed = self.max_bytes.saturating_sub(separator_bytes);
                let prefix = truncate_to_byte_boundary(&rendered, allowed);
                self.display_bytes += prefix.len();
                self.shown_lines.push(visible_line);
                self.display_parts.push(prefix.to_string());
            } else if let Some(anchors) = self.anchors.as_mut() {
                anchors.pop();
            }
            self.content_truncated = true;
            return;
        }

        self.display_bytes += separator_bytes + rendered_bytes;
        self.shown_lines.push(visible_line);
        self.display_parts.push(rendered);
    }

    fn render_visible_line(
        &mut self,
        line_number: usize,
        source_line: &str,
        visible_line: &str,
    ) -> String {
        match self.anchors.as_mut() {
            Some(anchors) => {
                let anchor = build_fs_read_line_anchor(line_number, source_line);
                let rendered = format_fs_read_hashline_line(&anchor, visible_line);
                anchors.push(anchor);
                rendered
            }
            None => format_fs_read_output_line(visible_line, line_number, self.render),
        }
    }

    fn finish(self, total_lines: usize, available_lines: usize) -> FsReadWindow {
        FsReadWindow {
            shown_lines: self.shown_lines,
            display_text: self.display_parts.join("\n"),
            anchors: self.anchors,
            total_lines,
            truncated: available_lines > self.line_limit || self.content_truncated,
        }
    }
}

fn truncate_to_byte_boundary(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut cut = max_bytes;
    while cut > 0 && !value.is_char_boundary(cut) {
        cut -= 1;
    }
    &value[..cut]
}

pub(super) fn read_fs_window(
    path: &Path,
    start_line_index: usize,
    line_limit: usize,
    max_bytes: usize,
    render: FsReadRenderOptions,
) -> Result<FsReadWindow, ToolError> {
    let mut available_lines = 0usize;
    let mut window = FsReadWindowBuilder::new(line_limit, max_bytes, render);
    let mut reader = open_fs_read_file(path)?;

    let total_lines = visit_fs_read_lines(&mut reader, start_line_index, |line_number, line| {
        available_lines += 1;
        window.push_visible_line(line_number, line);
        Ok(())
    })?;

    let offset = start_line_index + 1;
    if total_lines < offset && !(total_lines == 0 && offset == 1) {
        return Err(ToolError::Execution(format!(
            "Offset {offset} is out of range for this file ({total_lines} lines)"
        )));
    }

    Ok(window.finish(total_lines, available_lines))
}

pub(super) fn open_fs_read_file(
    path: &Path,
) -> Result<std::io::BufReader<std::fs::File>, ToolError> {
    let file = std::fs::File::open(path)
        .tool_err("failed to read file")?;
    Ok(std::io::BufReader::new(file))
}

pub(super) fn visit_fs_read_lines(
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
            .tool_err("failed to read file")?;
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
