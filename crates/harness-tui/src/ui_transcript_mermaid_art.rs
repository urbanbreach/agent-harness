use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::ui_transcript_mermaid::{
    ClassDiagram, DiagramDirection, Flowchart, SequenceDiagram, SequenceRow,
};

const MIN_WIDTH: usize = 7;
const MAX_LABEL_WIDTH: usize = 28;

pub(super) fn render_flowchart(flowchart: &Flowchart, content_width: usize) -> Vec<String> {
    match flowchart.direction {
        DiagramDirection::Down => render_vertical(flowchart, content_width),
        DiagramDirection::Right => render_horizontal(flowchart, content_width),
    }
}

pub(super) fn render_sequence(sequence: &SequenceDiagram, content_width: usize) -> Vec<String> {
    let labels = sequence
        .participants
        .iter()
        .map(|label| display_label(label, MAX_LABEL_WIDTH))
        .collect::<Vec<_>>();
    let columns = sequence_columns(&labels, content_width);
    let mut rows = vec![
        sequence_header(&labels, &columns),
        sequence_lifelines(&columns),
    ];
    for sequence_row in &sequence.rows {
        match sequence_row {
            SequenceRow::Message(index) => {
                if let Some(message) = sequence.messages.get(*index) {
                    rows.extend(render_sequence_message(message, &columns));
                }
            }
            SequenceRow::Note(text) => rows.push(render_sequence_note(text, content_width)),
            SequenceRow::Control(text) => {
                rows.push(format!("┄ {text} {}", "┄".repeat(8)));
            }
            SequenceRow::End => rows.push(format!("┄ end {}", "┄".repeat(8))),
        }
    }
    rows.push(sequence_header(&labels, &columns));
    rows
}

pub(super) fn render_class_diagram(diagram: &ClassDiagram, content_width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    let entity_width = diagram
        .entities
        .iter()
        .flat_map(|entity| {
            std::iter::once(entity.name.as_str()).chain(entity.members.iter().map(String::as_str))
        })
        .map(|text| UnicodeWidthStr::width(text).saturating_add(2))
        .max()
        .unwrap_or(MIN_WIDTH)
        .clamp(MIN_WIDTH, content_width.saturating_sub(2).max(MIN_WIDTH));

    for (index, entity) in diagram.entities.iter().enumerate() {
        rows.extend(class_entity_box(
            entity.name.as_str(),
            &entity.members,
            entity_width,
        ));
        if let Some(relationship) = diagram
            .relationships
            .iter()
            .find(|relationship| relationship.from == index || relationship.to == index)
        {
            let glyph =
                relationship
                    .source_head
                    .unwrap_or(if relationship.dotted { '╎' } else { '│' });
            rows.push(format!("{}{}", " ".repeat(entity_width / 2), glyph));
            if let Some(label) = relationship.label.as_deref() {
                rows.push(format!(
                    "{}{}",
                    " ".repeat(entity_width / 2),
                    display_label(label, entity_width)
                ));
            }
        }
    }
    rows
}

fn class_entity_box(name: &str, members: &[String], width: usize) -> Vec<String> {
    let name = display_label(name, width);
    let name_padding = width.saturating_sub(UnicodeWidthStr::width(name.as_str()));
    let mut rows = vec![
        format!("┌{}┐", "─".repeat(width)),
        format!("│{}{}│", name, " ".repeat(name_padding)),
    ];
    if !members.is_empty() {
        rows.push(format!("├{}┤", "─".repeat(width)));
        for member in members {
            let member = display_label(member, width);
            let padding = width.saturating_sub(UnicodeWidthStr::width(member.as_str()));
            rows.push(format!("│{member}{}│", " ".repeat(padding)));
        }
    }
    rows.push(format!("└{}┘", "─".repeat(width)));
    rows
}

fn render_sequence_message(
    message: &super::ui_transcript_mermaid::SequenceMessage,
    columns: &[usize],
) -> Vec<String> {
    let start = columns[message.from];
    let end = columns[message.to];
    if start == end {
        return vec![
            format!("{}╮", " ".repeat(start)),
            format!("{}╰── {} ╯", " ".repeat(start), message.text),
        ];
    }
    let left = start.min(end);
    let right = start.max(end);
    let connector = if message.dotted { '╌' } else { '─' };
    let mut row = String::new();
    row.push_str(&" ".repeat(left));
    row.push('├');
    if !message.text.is_empty() {
        row.push_str(&format!(" {} ", message.text));
    }
    let used = UnicodeWidthStr::width(row.as_str());
    row.push_str(&connector.to_string().repeat(right.saturating_sub(used)));
    row.push(if message.cross {
        '×'
    } else if start <= end {
        '▶'
    } else {
        '◀'
    });
    vec![row]
}

fn render_sequence_note(text: &str, content_width: usize) -> String {
    let text = display_label(text, content_width.saturating_sub(10));
    format!("┌ Note: {text} ┐")
}

pub(super) fn render_source_frame(body: &str, content_width: usize) -> Vec<String> {
    let inner_width = content_width
        .saturating_sub(2)
        .clamp(MIN_WIDTH, MAX_LABEL_WIDTH);
    let mut rows = vec![format!(
        "┌ mermaid {}",
        "─".repeat(inner_width.saturating_sub(8))
    )];
    for line in body.lines().filter(|line| !line.trim().is_empty()) {
        let text = display_label(line.trim(), inner_width);
        let padding = inner_width.saturating_sub(UnicodeWidthStr::width(text.as_str()));
        rows.push(format!("│{text}{}│", " ".repeat(padding)));
    }
    rows.push(format!("└{}┘", "─".repeat(inner_width)));
    rows
}

fn render_vertical(flowchart: &Flowchart, content_width: usize) -> Vec<String> {
    let labels = &flowchart.labels;
    let width = labels
        .iter()
        .map(|label| display_label(label, MAX_LABEL_WIDTH))
        .map(|label| UnicodeWidthStr::width(label.as_str()) + 2)
        .chain(
            flowchart
                .edge_labels
                .iter()
                .flatten()
                .map(|label| UnicodeWidthStr::width(label.as_str())),
        )
        .max()
        .unwrap_or(MIN_WIDTH)
        .max(MIN_WIDTH)
        .min(content_width.saturating_sub(2).max(MIN_WIDTH));
    let indent = content_width.saturating_sub(width + 2) / 2;
    let group_width = flowchart
        .groups
        .iter()
        .map(|group| UnicodeWidthStr::width(group.as_str()).saturating_add(3))
        .max()
        .unwrap_or(width)
        .max(width);
    let mut rows = flowchart
        .groups
        .iter()
        .map(|group| {
            format!(
                "┌ {group} {}┐",
                "─".repeat(group_width.saturating_sub(UnicodeWidthStr::width(group.as_str()) + 3))
            )
        })
        .collect::<Vec<_>>();
    for (index, label) in labels.iter().enumerate() {
        rows.extend(node_box(label, width, indent));
        if index + 1 < labels.len() {
            if let Some(Some(edge_label)) = flowchart.edge_labels.get(index) {
                rows.push(format!(
                    "{}{}",
                    " ".repeat(indent + width / 2 + 1),
                    display_label(edge_label, width)
                ));
            }
            rows.push(format!("{}│", " ".repeat(indent + width / 2 + 1)));
            rows.push(format!("{}▼", " ".repeat(indent + width / 2 + 1)));
        }
    }
    rows.extend(
        flowchart
            .groups
            .iter()
            .map(|_| format!("└{}┘", "─".repeat(group_width.saturating_sub(2)))),
    );
    rows
}

fn render_horizontal(flowchart: &Flowchart, content_width: usize) -> Vec<String> {
    let labels = flowchart
        .labels
        .iter()
        .map(|label| display_label(label, MAX_LABEL_WIDTH))
        .collect::<Vec<_>>();
    let required = labels
        .iter()
        .map(|label| UnicodeWidthStr::width(label.as_str()) + 2)
        .sum::<usize>()
        + labels.len().saturating_sub(1) * 3;
    if required > content_width {
        return render_vertical(flowchart, content_width);
    }
    let mut top = String::new();
    let mut middle = String::new();
    let mut bottom = String::new();
    for (index, label) in labels.iter().enumerate() {
        let width = UnicodeWidthStr::width(label.as_str()) + 2;
        top.push_str(&format!("┌{}┐", "─".repeat(width)));
        middle.push_str(&format!("│ {label} │"));
        bottom.push_str(&format!("└{}┘", "─".repeat(width)));
        if index + 1 < labels.len() {
            top.push_str("   ");
            middle.push_str("─▶");
            bottom.push_str("   ");
        }
    }
    vec![top, middle, bottom]
}

fn node_box(label: &str, width: usize, indent: usize) -> Vec<String> {
    let label = display_label(label, width.saturating_sub(2));
    let padding = width.saturating_sub(UnicodeWidthStr::width(label.as_str()));
    let pad = " ".repeat(indent);
    vec![
        format!("{pad}┌{}┐", "─".repeat(width)),
        format!(
            "{pad}│{}{}{}│",
            " ".repeat(padding / 2),
            label,
            " ".repeat(padding - padding / 2)
        ),
        format!("{pad}└{}┘", "─".repeat(width)),
    ]
}

fn sequence_columns(labels: &[String], content_width: usize) -> Vec<usize> {
    let step = (content_width / labels.len().max(1)).max(MAX_LABEL_WIDTH / 2 + 5);
    labels
        .iter()
        .enumerate()
        .map(|(index, _)| index * step)
        .collect()
}

fn sequence_header(labels: &[String], columns: &[usize]) -> String {
    let mut row = String::new();
    for (label, column) in labels.iter().zip(columns) {
        row.push_str(&" ".repeat(column.saturating_sub(UnicodeWidthStr::width(row.as_str()))));
        row.push_str(label);
    }
    row
}

fn sequence_lifelines(columns: &[usize]) -> String {
    let mut row = String::new();
    for column in columns {
        row.push_str(&" ".repeat(column.saturating_sub(UnicodeWidthStr::width(row.as_str()))));
        row.push('│');
    }
    row
}

fn display_label(label: &str, max_width: usize) -> String {
    let mut output = String::new();
    for character in label.chars() {
        if UnicodeWidthStr::width(output.as_str())
            + UnicodeWidthChar::width(character).unwrap_or_default()
            > max_width
        {
            while UnicodeWidthStr::width(output.as_str()) >= max_width {
                output.pop();
            }
            if max_width > 0 {
                output.push('…');
            }
            break;
        }
        output.push(character);
    }
    output
}
