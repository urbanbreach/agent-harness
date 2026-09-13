use super::*;
use crate::app::ToolCallEntry;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RecordedToolOutput {
    Search {
        mode: String,
        groups: Vec<SearchFile>,
        total: Option<u64>,
    },
    Web {
        text: String,
        sources: Vec<String>,
        metadata: Vec<(String, String)>,
    },
    Mcp {
        text: String,
        arguments: Vec<(String, String)>,
        error: Option<String>,
    },
    Directory {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SearchFile {
    path: String,
    rows: Vec<(Option<u64>, String)>,
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

pub(super) fn web_sources(tool: &ToolCallEntry) -> Vec<String> {
    tool.output_json
        .as_ref()
        .and_then(|value| value.get("sources").or_else(|| value.get("citations")))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|source| {
            source
                .as_str()
                .map(str::to_string)
                .or_else(|| string(source, "url"))
        })
        .collect()
}

fn project_file_listing(tool: &ToolCallEntry) -> Option<RecordedToolOutput> {
    let id = tool.effective_tool_id();
    let value = tool.output_json.as_ref();
    if matches!(id, "fs.ls" | "list") && tool.status != crate::app::ToolCallDisplayStatus::Failed {
        return Some(RecordedToolOutput::Directory {
            text: tool.output_summary.clone()?,
        });
    }
    if matches!(id, "fs.glob" | "glob") {
        let value = value?;
        return Some(RecordedToolOutput::Search {
            mode: "files_with_matches".into(),
            groups: value
                .get("paths")?
                .as_array()?
                .iter()
                .filter_map(|path| {
                    Some(SearchFile {
                        path: path.as_str()?.into(),
                        rows: Vec::new(),
                    })
                })
                .collect(),
            total: value.get("total_count").and_then(Value::as_u64),
        });
    }
    None
}

pub(super) fn project(tool: &ToolCallEntry) -> Option<RecordedToolOutput> {
    let id = tool.effective_tool_id();
    let value = tool.output_json.as_ref();
    let failed = tool.status == crate::app::ToolCallDisplayStatus::Failed;
    if failed && !super::ui_tool_titles::is_mcp_tool_id(id) {
        // Failure summaries are rendered once by the error block, not again
        // as if they were a successful web/MCP response.
        return None;
    }
    if matches!(id, "fs.ls" | "list" | "fs.glob" | "glob") {
        return project_file_listing(tool);
    }
    if matches!(id, "fs.grep" | "grep") {
        let value = value?;
        let mode = string(value, "output_mode").unwrap_or_else(|| "content".into());
        let mut groups: Vec<SearchFile> = Vec::new();
        let mut push = |path: String, line, text| {
            if let Some(group) = groups.last_mut().filter(|group| group.path == path) {
                group.rows.push((line, text));
            } else {
                groups.push(SearchFile {
                    path,
                    rows: vec![(line, text)],
                });
            }
        };
        match mode.as_str() {
            "files_with_matches" => {
                for path in value.get("files")?.as_array()? {
                    if let Some(path) = path.as_str() {
                        push(path.to_string(), None, String::new());
                    }
                }
            }
            "count" => {
                for entry in value.get("counts")?.as_array()? {
                    if let (Some(path), Some(count)) = (
                        string(entry, "file"),
                        entry.get("count").and_then(Value::as_u64),
                    ) {
                        push(path, None, format!("{count} matches"));
                    }
                }
            }
            _ => {
                for entry in value.get("matches")?.as_array()? {
                    let parsed = search_match(entry);
                    if let Some((path, line, text)) = parsed {
                        push(path, line, text);
                    }
                }
            }
        }
        return Some(RecordedToolOutput::Search {
            mode,
            groups,
            total: value.get("total_count").and_then(Value::as_u64),
        });
    }
    if matches!(
        id,
        "web.fetch" | "webfetch" | "search.web" | "websearch" | "search.code"
    ) {
        let text = value
            .and_then(|value| {
                string(value, "text")
                    .or_else(|| string(value, "content"))
                    .or_else(|| string(value, "output"))
            })
            .or_else(|| tool.output_summary.clone())
            .unwrap_or_default();
        let sources = web_sources(tool);
        if text.is_empty() && sources.is_empty() && value.is_none() {
            return None;
        }
        let metadata = value
            .map(|value| {
                ["status", "content_type", "size"]
                    .iter()
                    .filter_map(|key| {
                        if *key == "size" {
                            return value
                                .get("bytes")
                                .or_else(|| value.get("byte_len"))
                                .and_then(Value::as_u64)
                                .map(|bytes| ("size".into(), format_bytes(bytes)));
                        }
                        value
                            .get(*key)
                            .filter(|value| value.is_string() || value.is_number())
                            .map(|value| {
                                (
                                    (*key).to_string(),
                                    value
                                        .as_str()
                                        .map(str::to_string)
                                        .unwrap_or_else(|| value.to_string()),
                                )
                            })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        return Some(RecordedToolOutput::Web {
            text,
            sources,
            metadata,
        });
    }
    if super::ui_tool_titles::is_mcp_tool_id(id) {
        let arguments = mcp_arguments(&tool.args_summary);
        let text = value
            .and_then(|value| {
                value
                    .get("content")
                    .or_else(|| value.pointer("/payload/result/content"))
            })
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        string(item, "text").or_else(|| {
                            item.get("resource").and_then(|value| string(value, "text"))
                        })
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|text| !text.is_empty())
            .or_else(|| tool.output_summary.clone())
            .unwrap_or_default();
        return Some(RecordedToolOutput::Mcp {
            text: if failed { String::new() } else { text },
            arguments,
            error: failed.then(|| {
                tool.output_summary
                    .clone()
                    .or_else(|| super::ui_tool_error::tool_error_text(tool))
                    .unwrap_or_else(|| "Tool failed".into())
            }),
        });
    }
    None
}

// Read the recorded input in wire order without enabling serde_json's
// workspace-wide preserve_order feature (which would change durable encoding).
struct McpArguments(Vec<(String, String)>);

impl<'de> serde::Deserialize<'de> for McpArguments {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(ArgumentsVisitor)
    }
}

struct ArgumentsVisitor;

impl<'de> serde::de::Visitor<'de> for ArgumentsVisitor {
    type Value = McpArguments;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("tool input arguments")
    }

    fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let mut arguments = Vec::<(String, String)>::new();
        while let Some((key, value)) = map.next_entry::<String, Value>()? {
            let text = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            if let Some((_, previous)) = arguments.iter_mut().find(|(name, _)| *name == key) {
                *previous = text;
            } else {
                arguments.push((key, text));
            }
        }
        Ok(McpArguments(arguments))
    }
}

fn mcp_arguments(input: &str) -> Vec<(String, String)> {
    #[derive(serde::Deserialize)]
    struct WrappedArguments {
        arguments: McpArguments,
    }
    serde_json::from_str::<WrappedArguments>(input)
        .map(|wrapped| wrapped.arguments)
        .or_else(|_| serde_json::from_str::<McpArguments>(input))
        .map(|arguments| arguments.0)
        .unwrap_or_default()
}

impl RecordedToolOutput {
    pub(super) fn search_summary(&self) -> Option<String> {
        let Self::Search {
            mode,
            groups,
            total,
        } = self
        else {
            return None;
        };
        let count = total.unwrap_or_else(|| u64::try_from(groups.len()).unwrap_or(u64::MAX));
        let files = groups.len();
        Some(if mode == "files_with_matches" {
            if count == 0 {
                "(no files)".into()
            } else {
                format!("({count} file{})", if count == 1 { "" } else { "s" })
            }
        } else if count == 0 {
            "(no matches)".into()
        } else if files > 1 {
            format!(
                "({count} matches {} {files} files)",
                if mode == "count" { "across" } else { "in" }
            )
        } else {
            format!("({count} match{})", if count == 1 { "" } else { "es" })
        })
    }

    pub(super) fn full_text(&self) -> String {
        let text = match self {
            Self::Search {
                mode,
                groups,
                total,
            } => {
                let mut parts = vec![format!(
                    "Search · {mode}{}",
                    total
                        .map(|total| format!(" · {total} matches"))
                        .unwrap_or_default()
                )];
                for group in groups {
                    parts.push(group.path.clone());
                    parts.extend(group.rows.iter().map(|(line, text)| {
                        line.map_or_else(|| text.clone(), |line| format!("{line}  {text}"))
                    }));
                }
                if groups.is_empty() {
                    parts.push("No matches".into());
                }
                parts.join("\n")
            }
            Self::Web {
                text,
                sources,
                metadata,
            } => format!(
                "{}\n{text}\n{}",
                metadata
                    .iter()
                    .map(|(key, value)| format!("{key}: {value}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                sources.join("\n")
            ),
            Self::Mcp {
                text,
                arguments,
                error,
            } => format!(
                "{}\n\n{text}{}",
                arguments
                    .iter()
                    .map(|(key, value)| format!("{key}: {value}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                error.as_deref().unwrap_or_default()
            ),
            Self::Directory { text } => text.clone(),
        };
        super::ui_tool_output::safe_tool_text(&text)
    }

    pub(super) fn lines(&self, theme: &Theme, width: u16, expanded: bool) -> Vec<Line<'static>> {
        // Tool bodies share the blank separator used by the reference blocks.
        let mut lines = vec![Line::default()];
        match self {
            Self::Search { mode, groups, .. } => {
                append_search_lines(&mut lines, mode, groups, theme, width);
            }
            Self::Web {
                text,
                sources,
                metadata,
            } => {
                if !metadata.is_empty() {
                    lines.push(web_metadata_line(metadata, theme));
                    lines.push(Line::default());
                }
                append_plain_panel(&mut lines, text, theme, width, expanded);
                let domains = source_domains(sources);
                if !domains.is_empty() {
                    lines.push(Line::default());
                    lines.push(web_sources_line(&domains, theme));
                }
            }
            Self::Mcp {
                text,
                arguments,
                error,
            } => {
                for (key, value) in arguments {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("  {}: ", super::ui_tool_output::safe_tool_text(key)),
                            Style::default().fg(theme.text.secondary),
                        ),
                        Span::styled(
                            super::ui_tool_output::safe_tool_text(value),
                            Style::default().fg(theme.text.primary),
                        ),
                    ]));
                }
                if !arguments.is_empty() && !text.is_empty() {
                    lines.push(Line::default());
                }
                if !text.is_empty() {
                    append_plain_panel(&mut lines, text, theme, width, expanded);
                }
                if let Some(error) = error {
                    lines.push(Line::default());
                    lines.push(Line::from(Span::styled(
                        format!("  {}", super::ui_tool_output::safe_tool_text(error)),
                        Style::default().fg(theme.terminal_colors.error),
                    )));
                }
            }
            Self::Directory { text } => {
                let text = super::ui_tool_output::safe_tool_text(text);
                for row in text.lines() {
                    lines.push(
                        Line::from(Span::styled(
                            format!("  {row}"),
                            Style::default().fg(theme.text.primary),
                        ))
                        .style(Style::default().bg(theme.markdown.code_background)),
                    );
                }
            }
        }
        lines
    }
}

fn web_metadata_line(metadata: &[(String, String)], theme: &Theme) -> Line<'static> {
    let mut spans = vec![Span::raw("  ")];
    for (index, (key, value)) in metadata.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                ", ",
                Style::default().fg(theme.text.secondary),
            ));
        }
        spans.push(Span::styled(
            format!("{key}: "),
            Style::default().fg(theme.text.secondary),
        ));
        spans.push(Span::styled(
            super::ui_tool_output::safe_tool_text(value),
            Style::default().fg(theme.text.primary),
        ));
    }
    Line::from(spans)
}

fn web_sources_line(domains: &[&str], theme: &Theme) -> Line<'static> {
    let mut spans = vec![Span::styled(
        "  Sources: ",
        Style::default().fg(theme.text.secondary),
    )];
    for (index, domain) in domains.iter().take(3).enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                ", ",
                Style::default().fg(theme.text.secondary),
            ));
        }
        spans.push(Span::styled(
            (*domain).to_string(),
            Style::default().fg(theme.text.primary),
        ));
    }
    if domains.len() > 3 {
        spans.push(Span::styled(
            format!(" (+{} more)", domains.len() - 3),
            Style::default().fg(theme.text.secondary),
        ));
    }
    Line::from(spans)
}

fn append_plain_panel(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    theme: &Theme,
    width: u16,
    expanded: bool,
) {
    let panel = Style::default().bg(theme.markdown.code_background);
    lines.push(Line::default().style(panel));
    let text = super::ui_tool_output::safe_tool_text(text);
    let limit = if expanded { 10 } else { 3 };
    for row in text.lines().take(limit) {
        // The native inline panel clips each logical line. Full output stays
        // available in the block viewer, including the clipped suffix.
        let row = super::ui_chrome::take_width_prefix(row, usize::from(width.saturating_sub(2)));
        lines.push(Line::from(Span::styled(format!("  {row}"), theme.text.primary)).style(panel));
    }
    let remaining = text.lines().count().saturating_sub(limit);
    if remaining > 0 {
        lines.push(
            Line::from(Span::styled(
                format!("  ... ({remaining} more lines, press Enter to view)"),
                Style::default().fg(theme.terminal_colors.muted),
            ))
            .style(panel),
        );
    }
    lines.push(Line::default().style(panel));
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let (unit, divisor) = if bytes < 1024 * 1024 {
        ("KB", 1024)
    } else if bytes < 1024 * 1024 * 1024 {
        ("MB", 1024 * 1024)
    } else {
        ("GB", 1024 * 1024 * 1024)
    };
    let tenths = bytes.saturating_mul(10).saturating_add(divisor / 2) / divisor;
    format!("{}.{:01} {unit}", tenths / 10, tenths % 10)
}

fn append_search_lines(
    lines: &mut Vec<Line<'static>>,
    mode: &str,
    groups: &[SearchFile],
    theme: &Theme,
    width: u16,
) {
    let label = match mode {
        "files_with_matches" => "files",
        "count" => "count",
        _ => "pattern",
    };
    lines.push(Line::from(vec![
        Span::styled("  mode: ", Style::default().fg(theme.text.secondary)),
        Span::styled(label, Style::default().fg(theme.text.primary)),
    ]));
    lines.push(Line::default());
    for (index, group) in groups.iter().enumerate() {
        if mode == "content" && index > 0 {
            lines.push(Line::default());
        }
        if mode == "count" {
            let count = group
                .rows
                .first()
                .map(|(_, text)| text.trim_end_matches(" matches"))
                .unwrap_or("");
            lines.push(
                Line::from(vec![
                    Span::styled(
                        format!(
                            "  {}",
                            super::ui_tool_paths::tool_header_path(&group.path, true)
                        ),
                        Style::default().fg(super::ui_tool_paths::tool_path_color(theme)),
                    ),
                    Span::styled(format!(":{count}"), Style::default().fg(theme.text.primary)),
                ])
                .style(Style::default().bg(theme.markdown.code_background)),
            );
        } else {
            lines.extend(group.lines(theme, width));
        }
    }
    if groups.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no results)",
            Style::default().fg(theme.text.secondary),
        )));
    }
}

impl SearchFile {
    fn lines(&self, theme: &Theme, _width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let path = super::ui_tool_paths::tool_header_path(&self.path, true);
        let panel = Style::default().bg(theme.markdown.code_background);
        lines.push(
            Line::from(Span::styled(
                format!("  {path}"),
                Style::default().fg(super::ui_tool_paths::tool_path_color(theme)),
            ))
            .style(panel),
        );
        let gutter = self
            .rows
            .iter()
            .filter_map(|(line, _)| *line)
            .max()
            // SearchToolCallBlock uses a fixed four-cell number field.
            .map_or(0, |line| line.to_string().len().max(4) + 2);
        for (number, text) in &self.rows {
            if number.is_none() && text.is_empty() {
                continue;
            }
            // Native search results occupy one terminal row per match; the
            // surface clips long content without adding continuation rows.
            let text = super::ui_tool_output::safe_tool_text(text.trim_end());
            let prefix = number
                .map(|number| format!("{number:>digits$}  ", digits = gutter.saturating_sub(2)))
                .unwrap_or_else(|| " ".repeat(gutter));
            lines.push(
                Line::from(vec![
                    Span::raw("    "),
                    Span::styled(prefix, Style::default().fg(theme.text.secondary)),
                    Span::styled(text, Style::default().fg(theme.text.primary)),
                ])
                .style(panel),
            );
        }

        lines
    }
}

pub(super) fn source_domains(sources: &[String]) -> Vec<&str> {
    let mut domains = Vec::new();
    for source in sources {
        let Some(host) = source
            .strip_prefix("https://")
            .or_else(|| source.strip_prefix("http://"))
            .and_then(|rest| rest.split('/').next())
            .filter(|host| !host.is_empty() && !host.contains('@'))
        else {
            continue;
        };
        if !domains.contains(&host) {
            domains.push(host);
        }
    }
    domains
}

fn search_match(entry: &Value) -> Option<(String, Option<u64>, String)> {
    if let Some(text) = entry.as_str() {
        let (path, rest) = text.split_once(':')?;
        let (line, text) = rest.split_once(':')?;
        return Some((
            path.into(),
            Some(line.parse().ok()?),
            text.strip_prefix(' ').unwrap_or(text).into(),
        ));
    }
    let path = string(entry, "path").or_else(|| string(entry, "file"))?;
    let line = entry
        .get("line_number")
        .or_else(|| entry.get("line"))
        .and_then(Value::as_u64);
    Some((path, line, string(entry, "text").unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::ui_transcript_test_helpers::transcript_section_model_test_tool_call;
    use crate::UnwrapOrAbort;

    #[test]
    fn recorded_outputs_keep_typed_metadata_and_full_view_content() {
        let mut tool = transcript_section_model_test_tool_call("search", "fs.grep");
        tool.output_json = Some(
            serde_json::json!({"matches": ["a.rs:7:     needle", "b.rs:12: needle"], "total_count": 2}),
        );
        let output = project(&tool).unwrap_or_abort();
        let lines = output.lines(&Theme::default(), 40, false);
        let text = lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("  a.rs\n       7      needle")
                && text.contains("  b.rs\n      12  needle"),
            "{text}"
        );
        tool.output_json = Some(
            serde_json::json!({"output_mode": "count", "counts": [{"file": "a.rs", "count": 3}], "total_count": 3}),
        );
        assert!(project(&tool)
            .unwrap_or_abort()
            .full_text()
            .contains("3 matches"));
        tool.output_json = Some(serde_json::json!({"matches": [], "total_count": 0}));
        assert!(project(&tool)
            .unwrap_or_abort()
            .full_text()
            .contains("No matches"));
        tool.tool_id = "search.web".into();
        let body = (1..=12)
            .map(|n| format!("web row {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        tool.output_json = Some(
            serde_json::json!({"text": body, "sources": ["https://a.test/one", "https://a.test/two", "https://b.test/", "https://c.test/", "https://d.test/"]}),
        );
        let output = project(&tool).unwrap_or_abort();
        let citations = tool
            .output_json
            .as_mut()
            .unwrap_or_abort()
            .as_object_mut()
            .unwrap_or_abort();
        let sources = citations.remove("sources").unwrap_or_abort();
        citations.insert("citations".into(), sources);
        assert_eq!(
            project(&tool).unwrap_or_abort().full_text(),
            output.full_text()
        );
        for (expanded, last, hidden) in [
            (false, "web row 3", "web row 4"),
            (true, "web row 10", "web row 11"),
        ] {
            let text = output
                .lines(&Theme::default(), 80, expanded)
                .iter()
                .map(Line::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains(last) && !text.contains(hidden), "{text}");
            assert!(
                text.contains("Sources: a.test, b.test, c.test (+1 more)"),
                "{text}"
            );
        }
        assert!(output.full_text().contains("web row 12"));
        tool.tool_id = "webfetch".into();
        tool.output_summary = Some(body);
        tool.output_json =
            Some(serde_json::json!({"status":200, "content_type":"text/plain", "byte_len":2048}));
        let theme = Theme::default();
        let lines = project(&tool).unwrap_or_abort().lines(&theme, 80, true);
        assert!(lines.iter().any(|line| line
            .to_string()
            .contains("status: 200, content_type: text/plain, size: 2.0 KB")));
        assert!(lines
            .iter()
            .filter(|line| line.to_string().contains("web row"))
            .all(|line| line.style.bg == Some(theme.markdown.code_background)));

        tool.tool_id = "mcp.linear.save_issue".into();
        tool.args_summary =
            r#"{"arguments":{"title":"Preserve output","labels":["terminal"]}}"#.into();
        tool.output_json =
            Some(serde_json::json!({"content":[{"text":"First"},{"text":"Second"}]}));
        let text = project(&tool).unwrap_or_abort().full_text();
        assert!(
            text.starts_with("title: Preserve output\nlabels:"),
            "{text}"
        );
        assert!(text.ends_with("First\nSecond"), "{text}");
        tool.status = crate::app::ToolCallDisplayStatus::Failed;
        tool.output_summary = Some("Error: permission denied".into());
        let viewer = crate::ui::recorded_tool_viewer_text(&tool);
        assert!(
            viewer.contains("title: Preserve output") && viewer.contains("permission denied"),
            "{viewer}"
        );
        tool.output_summary = None;
        tool.output_json = Some(serde_json::json!({"error":{"message":"resource unavailable"}}));
        assert!(crate::ui::recorded_tool_viewer_text(&tool).contains("resource unavailable"));
    }
}
