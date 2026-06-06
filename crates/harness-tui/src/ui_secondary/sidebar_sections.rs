use super::*;

#[derive(Debug, Clone)]
struct StyledTextChunk {
    text: String,
    style: Style,
}

pub(super) fn build_operator_rail_body_layout(
    body: &OperatorRailBody,
    theme: &Theme,
    width: u16,
    animation_phase: usize,
) -> OperatorRailBodyLayout {
    let mut lines = Vec::new();
    let mut heading_hit_regions = Vec::new();
    let mut subagent_hit_regions = Vec::new();
    let mut subagent_group_hit_regions = Vec::new();
    let mut visual_row = 0usize;

    let presentation = OperatorRailBodyPresentation::Regular;
    match presentation {
        OperatorRailBodyPresentation::Regular => {
            for (index, section) in body.sections.iter().enumerate() {
                if index > 0 {
                    let blank = Line::from("");
                    visual_row = visual_row.saturating_add(visual_rows_for_line(&blank, width));
                    lines.push(blank);
                }
                let section_top = visual_row;
                let section_lines =
                    build_operator_rail_section_lines(theme, section, width, animation_phase);
                if let Some(disclosure) = section.disclosure() {
                    let heading_height = section_lines
                        .first()
                        .map(|line| visual_rows_for_line(line, width))
                        .unwrap_or(1);
                    heading_hit_regions.push(OperatorRailHeadingHitRegion {
                        section: disclosure.section,
                        top_row: visual_row,
                        height: heading_height,
                    });
                }
                if matches!(section, OperatorRailBodySection::Subagents { .. })
                    && !section.collapsed()
                {
                    let subagent_regions = subagent_hit_regions_for_section(
                        section,
                        theme,
                        width,
                        section_top,
                        animation_phase,
                    );
                    subagent_hit_regions.extend(subagent_regions.item_regions);
                    subagent_group_hit_regions.extend(subagent_regions.group_regions);
                }
                visual_row =
                    visual_row.saturating_add(visual_rows_for_lines(&section_lines, width));
                lines.extend(section_lines);
            }
        }
    }

    OperatorRailBodyLayout {
        lines,
        heading_hit_regions,
        subagent_hit_regions,
        subagent_group_hit_regions,
    }
}

fn build_operator_rail_section_lines(
    theme: &Theme,
    section: &OperatorRailBodySection,
    width: u16,
    animation_phase: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![section.heading_line(theme)];

    if section.collapsed() {
        return lines;
    }

    match section {
        OperatorRailBodySection::Subagents { groups, .. } => {
            for group in groups {
                if group.items.len() > 1 {
                    lines.push(subagent_group_line(theme, group, animation_phase, width));
                    if !group.expanded {
                        continue;
                    }
                }
                for item in &group.items {
                    lines.push(subagent_item_line(
                        theme,
                        group,
                        item,
                        animation_phase,
                        width,
                    ));
                }
            }
        }
        OperatorRailBodySection::Todo { items, .. }
        | OperatorRailBodySection::Mcp { items, .. }
        | OperatorRailBodySection::Lsp { items, .. }
        | OperatorRailBodySection::ModifiedFiles { items, .. } => {
            for item in items {
                append_operator_rail_item(&mut lines, theme, section, item);
            }
        }
    }

    lines
}

struct OperatorRailSubagentRegions {
    item_regions: Vec<OperatorRailSubagentHitRegion>,
    group_regions: Vec<OperatorRailSubagentGroupHitRegion>,
}

fn subagent_hit_regions_for_section(
    section: &OperatorRailBodySection,
    theme: &Theme,
    width: u16,
    section_top: usize,
    animation_phase: usize,
) -> OperatorRailSubagentRegions {
    let OperatorRailBodySection::Subagents { groups, .. } = section else {
        return OperatorRailSubagentRegions {
            item_regions: Vec::new(),
            group_regions: Vec::new(),
        };
    };

    let mut item_regions = Vec::new();
    let mut group_regions = Vec::new();
    let heading_height = visual_rows_for_line(&section.heading_line(theme), width);
    let mut visual_row = section_top.saturating_add(heading_height);
    for group in groups {
        if group.items.len() > 1 {
            let group_line = subagent_group_line(theme, group, animation_phase, width);
            let height = visual_rows_for_line(&group_line, width);
            group_regions.push(OperatorRailSubagentGroupHitRegion {
                agent_name: group.agent_name.clone(),
                top_row: visual_row,
                height,
            });
            visual_row = visual_row.saturating_add(height);
            if !group.expanded {
                continue;
            }
        }
        for item in &group.items {
            let item_line = subagent_item_line(theme, group, item, animation_phase, width);
            let height = visual_rows_for_line(&item_line, width);
            if let Some(session_id) = item.child_session_id.as_ref() {
                item_regions.push(OperatorRailSubagentHitRegion {
                    session_id: session_id.clone(),
                    top_row: visual_row,
                    height,
                });
            }
            visual_row = visual_row.saturating_add(height);
        }
    }
    OperatorRailSubagentRegions {
        item_regions,
        group_regions,
    }
}

fn subagent_group_line(
    theme: &Theme,
    group: &SubagentRailGroup,
    animation_phase: usize,
    width: u16,
) -> Line<'static> {
    let status = subagent_group_status(group);
    let active = status.is_active();
    subagent_compact_line(
        vec![
            StyledTextChunk {
                text: format!("{} ", if group.expanded { "▼" } else { "▶" }),
                style: subagent_bullet_style(theme, active),
            },
            StyledTextChunk {
                text: group.agent_name.clone(),
                style: Style::default().fg(theme.text.primary),
            },
            StyledTextChunk {
                text: format!(" {} ", status.glyph(animation_phase)),
                style: subagent_indicator_style(theme, status),
            },
            StyledTextChunk {
                text: subagent_group_summary(group),
                style: subagent_description_style(theme, status),
            },
        ],
        width,
    )
}

fn subagent_item_line(
    theme: &Theme,
    group: &SubagentRailGroup,
    item: &SubagentRailItem,
    animation_phase: usize,
    width: u16,
) -> Line<'static> {
    let multi_item = group.items.len() > 1;
    let leading = if multi_item { "  " } else { "• " };
    let bullet_style = subagent_bullet_style(theme, item.status.is_active());
    let mut chunks = Vec::new();
    chunks.push(StyledTextChunk {
        text: leading.to_string(),
        style: bullet_style,
    });
    chunks.extend([
        StyledTextChunk {
            text: format!("{} ", item.status.glyph(animation_phase)),
            style: subagent_indicator_style(theme, item.status),
        },
        StyledTextChunk {
            text: item.description.clone(),
            style: subagent_description_style(theme, item.status),
        },
    ]);
    subagent_compact_line(chunks, width)
}

fn subagent_group_status(group: &SubagentRailGroup) -> SubagentRailStatus {
    if let Some(status) = group
        .items
        .iter()
        .find_map(|item| item.status.is_active().then_some(item.status))
    {
        return status;
    }

    if group
        .items
        .iter()
        .any(|item| matches!(item.status, SubagentRailStatus::Error))
    {
        return SubagentRailStatus::Error;
    }

    SubagentRailStatus::Completed
}

pub(super) fn subagent_group_summary(group: &SubagentRailGroup) -> String {
    let total = group.items.len();
    let active = group
        .items
        .iter()
        .filter(|item| item.status.is_active())
        .count();
    let failed = group
        .items
        .iter()
        .filter(|item| matches!(item.status, SubagentRailStatus::Error))
        .count();

    if active > 0 {
        return format!("{} · {} active", subagent_task_count(total), active);
    }
    if failed > 0 {
        return format!("{} · {} failed", subagent_task_count(total), failed);
    }

    format!("{} done", subagent_task_count(total))
}

fn subagent_task_count(count: usize) -> String {
    if count == 1 {
        "1 task".to_string()
    } else {
        format!("{count} tasks")
    }
}

fn subagent_bullet_style(theme: &Theme, active: bool) -> Style {
    if active {
        Style::default().fg(theme.status.success)
    } else {
        Style::default().fg(theme.text.primary)
    }
}

fn subagent_indicator_style(theme: &Theme, status: SubagentRailStatus) -> Style {
    if status.is_active() {
        Style::default().fg(theme.status.success)
    } else {
        Style::default().fg(theme.text.secondary)
    }
}

fn subagent_description_style(theme: &Theme, status: SubagentRailStatus) -> Style {
    if status.is_active() {
        Style::default().fg(theme.text.primary)
    } else {
        Style::default().fg(theme.text.secondary)
    }
}

fn subagent_compact_line(chunks: Vec<StyledTextChunk>, width: u16) -> Line<'static> {
    Line::from(truncate_sidebar_styled_chunks(
        &chunks,
        usize::from(width.max(1)),
    ))
}

fn truncate_sidebar_styled_chunks(
    chunks: &[StyledTextChunk],
    max_width: usize,
) -> Vec<Span<'static>> {
    if max_width == 0 {
        return Vec::new();
    }

    let mut rendered = Vec::new();
    let mut used = 0usize;
    for chunk in chunks {
        if used >= max_width {
            break;
        }
        let remaining = max_width - used;
        let text = truncate_plain_text(&chunk.text, remaining);
        used += display_width(&text);
        rendered.push(Span::styled(text, chunk.style));
    }
    rendered
}

fn visual_rows_for_lines(lines: &[Line<'static>], width: u16) -> usize {
    lines
        .iter()
        .map(|line| visual_rows_for_line(line, width))
        .sum()
}

fn visual_rows_for_line(line: &Line<'static>, width: u16) -> usize {
    if width == 0 {
        return 0;
    }

    let line_width = line.width();
    if line_width == 0 {
        return 1;
    }

    line_width.div_ceil(usize::from(width))
}

fn append_operator_rail_item(
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    section: &OperatorRailBodySection,
    item: &OperatorRailItem,
) {
    let bullet_prefix = "• ";
    let continuation_prefix = if bullet_prefix == "• " {
        "  "
    } else {
        bullet_prefix
    };
    let mut raw_lines = item.text().lines();
    let Some(first_line) = raw_lines.next() else {
        return;
    };

    let prefix_style = Style::default().fg(theme.text.secondary);
    let primary_style = Style::default().fg(theme.text.primary);
    let secondary_style = Style::default().fg(theme.text.secondary);

    match item {
        OperatorRailItem::Plain(_) => lines.push(Line::from(Span::styled(
            format!("{bullet_prefix}{first_line}"),
            secondary_style,
        ))),
        OperatorRailItem::Todo { status, .. } => {
            let style = status.style(theme);
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", status.checkbox_glyph()), style),
                Span::styled(first_line.to_string(), style),
            ]));
        }
        OperatorRailItem::Status { suffix, state, .. } => {
            let dot_color = match state {
                RuntimeHealthState::Healthy => theme.status.success,
                RuntimeHealthState::Unhealthy => theme.status.error,
            };
            let label_style = if matches!(section, OperatorRailBodySection::Mcp { .. }) {
                primary_style
            } else {
                secondary_style
            };
            let mut spans = vec![
                Span::styled(bullet_prefix.to_string(), Style::default().fg(dot_color)),
                Span::styled(first_line.to_string(), label_style),
            ];
            if let Some(suffix) = suffix {
                spans.push(Span::styled(format!(" {suffix}"), secondary_style));
            }
            lines.push(Line::from(spans));
        }
        OperatorRailItem::ModifiedFile {
            additions,
            removals,
            ..
        } => {
            let mut spans = vec![
                Span::styled(bullet_prefix.to_string(), prefix_style),
                Span::styled(first_line.to_string(), secondary_style),
            ];
            if let (Some(additions), Some(removals)) = (additions, removals) {
                spans.push(Span::styled(" · ".to_string(), secondary_style));
                spans.push(Span::styled(
                    format!("+{additions}"),
                    Style::default().fg(theme.status.success),
                ));
                spans.push(Span::styled(" ".to_string(), secondary_style));
                spans.push(Span::styled(
                    format!("-{removals}"),
                    Style::default().fg(theme.status.error),
                ));
            }
            lines.push(Line::from(spans));
        }
    }

    for line in raw_lines {
        lines.push(Line::from(Span::styled(
            format!("{continuation_prefix}{line}"),
            secondary_style,
        )));
    }
}
