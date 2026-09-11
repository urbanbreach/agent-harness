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
        metadata: String,
    },
    Mcp {
        text: String,
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
            .or_else(|| tool.output_summary.clone())?;
        let sources = value
            .and_then(|value| value.get("sources"))
            .and_then(Value::as_array)
            .map(|sources| {
                sources
                    .iter()
                    .filter_map(|source| {
                        source
                            .as_str()
                            .map(str::to_string)
                            .or_else(|| string(source, "url"))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let metadata = value
            .map(|value| {
                ["status", "content_type", "bytes"]
                    .iter()
                    .filter_map(|key| {
                        value
                            .get(*key)
                            .filter(|value| value.is_string() || value.is_number())
                            .map(|value| {
                                format!(
                                    "{key}: {}",
                                    value
                                        .as_str()
                                        .map(str::to_string)
                                        .unwrap_or_else(|| value.to_string())
                                )
                            })
                    })
                    .collect::<Vec<_>>()
                    .join(" · ")
            })
            .unwrap_or_default();
        return Some(RecordedToolOutput::Web {
            text,
            sources,
            metadata,
        });
    }
    if super::ui_tool_titles::is_mcp_tool_id(id) {
        let text = value
            .and_then(|value| value.get("content"))
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
                    .join("\n\n")
            })
            .filter(|text| !text.is_empty())
            .or_else(|| tool.output_summary.clone())?;
        return Some(RecordedToolOutput::Mcp { text });
    }
    None
}

impl RecordedToolOutput {
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
            } => format!("{metadata}\n{text}\n{}", sources.join("\n")),
            Self::Mcp { text } | Self::Directory { text } => text.clone(),
        };
        super::ui_tool_output::safe_tool_text(&text)
    }

    pub(super) fn lines(&self, theme: &Theme, width: u16, expanded: bool) -> Vec<Line<'static>> {
        // Tool bodies share the blank separator used by the reference blocks.
        let mut lines = vec![Line::default()];
        // WebSearchToolCallBlock pads its output by two cells inside the
        // shared entry content area. MCP output keeps the unindented origin.
        let width = if matches!(self, Self::Web { .. } | Self::Directory { .. }) {
            width.saturating_sub(2).max(1)
        } else {
            width
        };
        match self {
            Self::Search { mode, groups, .. } => {
                append_search_lines(&mut lines, mode, groups, theme, width);
            }
            Self::Web {
                text,
                sources,
                metadata,
            } => {
                // WebSearchToolCallBlock has one extra row inside its panel.
                lines.push(Line::default());
                if !metadata.is_empty() {
                    lines.push(Line::from(Span::styled(
                        super::ui_tool_output::safe_tool_text(metadata),
                        Style::default().fg(theme.text.secondary),
                    )));
                }
                let limit = if expanded { 10 } else { 3 };
                let text = super::ui_tool_output::safe_tool_text(text);
                for row in text.lines().take(limit) {
                    for row in super::wrap_completion_text(row, usize::from(width)) {
                        lines.push(Line::from(Span::styled(
                            row,
                            Style::default().fg(theme.text.primary),
                        )));
                    }
                }
                let remaining = text.lines().count().saturating_sub(limit);
                if remaining > 0 {
                    lines.push(Line::from(Span::styled(
                        format!("… ({remaining} more lines · Ctrl+F full view)"),
                        Style::default().fg(theme.text.secondary),
                    )));
                }
                lines.push(Line::default());
                let domains = source_domains(sources);
                if !domains.is_empty() {
                    lines.push(Line::default());
                    lines.push(Line::from(Span::styled(
                        format!(
                            "Sources: {}{}",
                            domains
                                .iter()
                                .take(3)
                                .copied()
                                .collect::<Vec<_>>()
                                .join(", "),
                            if domains.len() > 3 {
                                format!(" +{}", domains.len() - 3)
                            } else {
                                String::new()
                            }
                        ),
                        Style::default().fg(theme.text.secondary),
                    )));
                }
            }
            Self::Mcp { text } => super::ui_markdown::append_rich_text_block(
                &mut lines,
                &super::ui_tool_output::safe_tool_text(text),
                theme.text.primary,
                "",
                theme,
                width,
            ),
            Self::Directory { text } => {
                let text = super::ui_tool_output::safe_tool_text(text);
                for row in text.lines() {
                    for spans in super::ui_transcript_surface::wrap_preformatted_spans(
                        vec![Span::styled(
                            row.to_string(),
                            Style::default().fg(theme.text.primary),
                        )],
                        usize::from(width),
                    ) {
                        lines.push(Line::from(spans));
                    }
                }
            }
        }
        if matches!(self, Self::Web { .. } | Self::Directory { .. }) {
            for line in &mut lines {
                line.spans.insert(0, Span::raw("  "));
            }
        }
        lines
    }
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
    lines.push(Line::from(Span::styled(
        format!("  mode: {label}"),
        Style::default().fg(theme.text.secondary),
    )));
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
            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "  {}",
                        super::ui_tool_paths::tool_header_path(&group.path, true)
                    ),
                    Style::default().fg(theme.text.accent),
                ),
                Span::styled(format!(":{count}"), Style::default().fg(theme.text.primary)),
            ]));
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
    fn lines(&self, theme: &Theme, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let path = super::ui_tool_paths::tool_header_path(&self.path, true);
        lines.push(Line::from(Span::styled(
            format!("  {path}"),
            Style::default()
                .fg(theme.text.accent)
                .add_modifier(Modifier::BOLD),
        )));
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
            let text = super::ui_tool_output::safe_tool_text(text);
            for (index, row) in super::ui_transcript_surface::wrap_preformatted_spans(
                vec![Span::raw(text)],
                usize::from(width).saturating_sub(gutter + 4).max(1),
            )
            .into_iter()
            .map(|spans| Line::from(spans).to_string())
            .enumerate()
            {
                let prefix = number
                    .filter(|_| index == 0)
                    .map(|number| format!("{number:>digits$}  ", digits = gutter.saturating_sub(2)))
                    .unwrap_or_else(|| " ".repeat(gutter));
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(prefix, Style::default().fg(theme.text.secondary)),
                    Span::styled(row, Style::default().fg(theme.text.primary)),
                ]));
            }
        }

        lines
    }
}

fn source_domains(sources: &[String]) -> Vec<&str> {
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
    fn recorded_search_and_web_keep_typed_metadata_and_full_view_content() {
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
                text.contains("Sources: a.test, b.test, c.test +1"),
                "{text}"
            );
        }
        assert!(output.full_text().contains("web row 12"));
    }
}
