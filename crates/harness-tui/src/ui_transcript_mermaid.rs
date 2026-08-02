use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::ui_transcript_mermaid_art::{
    render_class_diagram, render_flowchart, render_sequence, render_source_frame,
};
use crate::theme::Theme;

const MAX_NODES: usize = 24;
const MIN_NODE_WIDTH: usize = 7;
const MAX_LABEL_WIDTH: usize = 28;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DiagramDirection {
    Down,
    Right,
}

pub(super) struct Flowchart {
    pub(super) direction: DiagramDirection,
    pub(super) labels: Vec<String>,
    pub(super) groups: Vec<String>,
    pub(super) edge_labels: Vec<Option<String>>,
}

pub(super) struct SequenceDiagram {
    pub(super) participants: Vec<String>,
    pub(super) messages: Vec<SequenceMessage>,
    pub(super) rows: Vec<SequenceRow>,
}

pub(super) struct SequenceMessage {
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) text: String,
    pub(super) dotted: bool,
    pub(super) cross: bool,
}

pub(super) enum SequenceRow {
    Message(usize),
    Note(String),
    Control(String),
    End,
}

pub(super) struct ClassDiagram {
    pub(super) entities: Vec<ClassEntity>,
    pub(super) relationships: Vec<ClassRelationship>,
}

pub(super) struct ClassEntity {
    pub(super) name: String,
    pub(super) members: Vec<String>,
}

pub(super) struct ClassRelationship {
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) label: Option<String>,
    pub(super) source_head: Option<char>,
    pub(super) dotted: bool,
}

pub(super) fn is_mermaid_language(language: Option<&str>) -> bool {
    language.is_some_and(|lang| {
        lang.split_whitespace()
            .next()
            .is_some_and(|token| token.eq_ignore_ascii_case("mermaid"))
    })
}

pub(super) fn render_mermaid_diagram(
    body: &str,
    prefix: &str,
    theme: &Theme,
    width: u16,
) -> Vec<Line<'static>> {
    let content_width = usize::from(width).saturating_sub(UnicodeWidthStr::width(prefix));
    let art = parse_flowchart(body)
        .map(|flowchart| render_flowchart(&flowchart, content_width))
        .or_else(|| parse_state(body).map(|state| render_flowchart(&state, content_width)))
        .or_else(|| parse_class(body).map(|diagram| render_class_diagram(&diagram, content_width)))
        .or_else(|| parse_er(body).map(|diagram| render_class_diagram(&diagram, content_width)))
        .or_else(|| parse_sequence(body).map(|sequence| render_sequence(&sequence, content_width)))
        .unwrap_or_else(|| render_source_frame(body, content_width));
    let border = Style::default().fg(theme.text.secondary);
    let node = Style::default().fg(theme.text.primary);

    art.into_iter()
        .map(|row| {
            let style = if row.contains('┌')
                || row.contains('└')
                || row.contains('│')
                || row.contains('─')
                || row.contains('▼')
                || row.contains('▶')
            {
                border
            } else {
                node
            };
            Line::from(vec![
                Span::raw(prefix.to_string()),
                Span::styled(row, style),
            ])
        })
        .collect()
}

fn parse_flowchart(body: &str) -> Option<Flowchart> {
    let mut lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    let header = lines.next()?;
    let mut header_tokens = header.split_whitespace();
    if !matches!(
        header_tokens.next()?.to_ascii_lowercase().as_str(),
        "graph" | "flowchart"
    ) {
        return None;
    }
    let direction = match header_tokens
        .next()
        .unwrap_or("TB")
        .to_ascii_uppercase()
        .as_str()
    {
        "LR" | "RL" => DiagramDirection::Right,
        _ => DiagramDirection::Down,
    };
    let mut nodes = Vec::new();
    let mut groups = Vec::new();
    let mut edge_labels = Vec::new();
    for statement in lines {
        if let Some(group) = flowchart_group_label(statement) {
            groups.push(group);
            continue;
        }
        if statement.eq_ignore_ascii_case("end") {
            continue;
        }
        edge_labels.push(flowchart_edge_label(statement));
        let statement = statement
            .replace("-.->", "-->")
            .replace("==>", "-->")
            .replace("---", "-->");
        for fragment in statement.split("-->") {
            let fragment = fragment.trim().trim_start_matches('|');
            let fragment = fragment
                .split_once('|')
                .map_or(fragment, |(_, remainder)| remainder.trim());
            let (id, label) = node_label(fragment)?;
            if let Some((_, existing_label)) = nodes.iter_mut().find(|(node_id, _)| node_id == &id)
            {
                if let Some(label) = label {
                    *existing_label = label;
                }
            } else {
                let label = label.unwrap_or_else(|| id.clone());
                nodes.push((id, label));
                if nodes.len() == MAX_NODES {
                    break;
                }
            }
        }
    }
    let labels: Vec<String> = nodes.into_iter().map(|(_, label)| label).collect();
    (!labels.is_empty()).then_some(Flowchart {
        direction,
        labels,
        groups,
        edge_labels,
    })
}

fn flowchart_group_label(statement: &str) -> Option<String> {
    let group = statement.strip_prefix("subgraph ")?.trim();
    let group = group
        .split_once('[')
        .and_then(|(_, label)| label.strip_suffix(']'))
        .unwrap_or(group)
        .trim_matches('"')
        .trim();
    (!group.is_empty()).then(|| group.to_string())
}

fn flowchart_edge_label(statement: &str) -> Option<String> {
    let (_, remainder) = statement.split_once('|')?;
    let (label, _) = remainder.split_once('|')?;
    let label = label.trim();
    (!label.is_empty()).then(|| label.to_string())
}

fn node_label(fragment: &str) -> Option<(String, Option<String>)> {
    let fragment = fragment.trim();
    let id_len = fragment
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
        .map(char::len_utf8)
        .sum::<usize>();
    if id_len == 0 {
        return None;
    }
    let id = &fragment[..id_len];
    let label = fragment[id_len..].trim();
    let label = label
        .trim_matches(|character| matches!(character, '[' | ']' | '(' | ')' | '{' | '}' | '"'))
        .trim();
    Some((
        id.to_string(),
        if label.is_empty() {
            None
        } else {
            Some(label.to_string())
        },
    ))
}

fn parse_state(body: &str) -> Option<Flowchart> {
    let mut lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    if !lines
        .next()?
        .split_whitespace()
        .next()?
        .to_ascii_lowercase()
        .starts_with("statediagram")
    {
        return None;
    }
    let mut labels = Vec::new();
    let mut aliases = std::collections::BTreeMap::new();
    let mut in_note = false;
    for statement in lines {
        if in_note {
            in_note = !statement.eq_ignore_ascii_case("end note");
            continue;
        }
        if statement.starts_with("note ") {
            in_note = !statement.contains(':');
            continue;
        }
        if let Some((alias, label)) = state_alias_label(statement) {
            aliases.insert(alias.to_string(), label.to_string());
            if !labels.contains(&label.to_string()) {
                labels.push(label.to_string());
            }
            continue;
        }
        if let Some(choice) = state_choice_label(statement) {
            labels.push(format!("◇ {choice}"));
            continue;
        }
        let is_transition = statement.contains("-->");
        for fragment in statement.split("-->") {
            let label = if is_transition {
                state_transition_label(fragment)?
            } else {
                state_label(fragment)?
            };
            let label = aliases.get(&label).cloned().unwrap_or(label);
            let choice_label = format!("◇ {label}");
            let label = if labels.contains(&choice_label) {
                choice_label
            } else {
                label
            };
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
    }
    (!labels.is_empty()).then_some(Flowchart {
        direction: DiagramDirection::Down,
        labels,
        groups: Vec::new(),
        edge_labels: Vec::new(),
    })
}

fn state_alias_label(statement: &str) -> Option<(&str, &str)> {
    let state = statement.strip_prefix("state ")?.trim();
    let (label, alias) = state.split_once(" as ")?;
    let label = label.trim().trim_matches('"');
    let alias = alias.trim();
    (!label.is_empty() && !alias.is_empty()).then_some((alias, label))
}

fn state_choice_label(statement: &str) -> Option<&str> {
    let state = statement.strip_prefix("state ")?.trim();
    let choice = state.strip_suffix("<<choice>>")?.trim();
    (!choice.is_empty()).then_some(choice)
}

fn state_transition_label(fragment: &str) -> Option<String> {
    let fragment = fragment.split_once(':').map_or(fragment, |(node, _)| node);
    state_label(fragment)
}

fn state_label(fragment: &str) -> Option<String> {
    let fragment = fragment.trim();
    if fragment == "[*]" {
        return Some("Start".to_string());
    }
    if let Some((_, description)) = fragment.split_once(':') {
        return (!description.trim().is_empty()).then(|| description.trim().to_string());
    }
    let fragment = fragment.strip_prefix("state ").unwrap_or(fragment).trim();
    let label = fragment
        .split_once(" as ")
        .map_or(fragment, |(_, label)| label.trim())
        .trim_matches(|character| matches!(character, '"' | '[' | ']' | '{' | '}'))
        .trim();
    (!label.is_empty() && label != "end").then(|| label.to_string())
}

fn parse_class(body: &str) -> Option<ClassDiagram> {
    let mut lines = diagram_lines(body);
    let header = lines.next()?;
    if !header.eq_ignore_ascii_case("classDiagram") {
        return None;
    }

    let mut diagram = ClassDiagram {
        entities: Vec::new(),
        relationships: Vec::new(),
    };
    let mut entity_index = std::collections::BTreeMap::new();
    let mut active_entity: Option<usize> = None;
    for line in lines {
        if line == "}" {
            active_entity = None;
            continue;
        }
        if let Some(entity) = active_entity {
            diagram.entities[entity]
                .members
                .push(normalize_class_text(line));
            continue;
        }
        if let Some(name) = line.strip_prefix("class ") {
            let name = name.trim_end_matches('{').trim();
            if name.is_empty() {
                return None;
            }
            let entity = class_entity_index(&mut diagram, &mut entity_index, name);
            if line.ends_with('{') {
                active_entity = Some(entity);
            }
            continue;
        }
        if let Some((annotation, name)) = class_annotation(line) {
            let entity = class_entity_index(&mut diagram, &mut entity_index, name);
            diagram.entities[entity].name = format!("«{annotation}» {name}");
            continue;
        }
        if let Some((name, member)) = line.split_once(':') {
            let name = name.trim();
            let member = member.trim();
            if !name.is_empty() && !member.is_empty() {
                let entity = class_entity_index(&mut diagram, &mut entity_index, name);
                diagram.entities[entity]
                    .members
                    .push(normalize_class_text(member));
                continue;
            }
        }
        if let Some(relationship) = parse_class_relationship(line, &mut diagram, &mut entity_index)
        {
            diagram.relationships.push(relationship);
            continue;
        }
        if !matches!(
            line,
            "direction LR" | "direction RL" | "direction TD" | "direction BT"
        ) {
            return None;
        }
    }
    (!diagram.entities.is_empty()).then_some(diagram)
}

fn parse_er(body: &str) -> Option<ClassDiagram> {
    let mut lines = diagram_lines(body);
    let header = lines.next()?;
    if !header.eq_ignore_ascii_case("erDiagram") {
        return None;
    }

    let mut diagram = ClassDiagram {
        entities: Vec::new(),
        relationships: Vec::new(),
    };
    let mut entity_index = std::collections::BTreeMap::new();
    let mut active_entity: Option<usize> = None;
    for line in lines {
        if line == "}" {
            active_entity = None;
            continue;
        }
        if let Some(entity) = active_entity {
            let attribute = line.split('"').next().unwrap_or_default().trim();
            if !attribute.is_empty() {
                diagram.entities[entity].members.push(attribute.to_string());
            }
            continue;
        }
        if let Some(name) = line.strip_suffix('{') {
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            active_entity = Some(class_entity_index(&mut diagram, &mut entity_index, name));
            continue;
        }
        if let Some(relationship) = parse_er_relationship(line, &mut diagram, &mut entity_index) {
            diagram.relationships.push(relationship);
            continue;
        }
        if is_mermaid_identifier(line) {
            class_entity_index(&mut diagram, &mut entity_index, line);
            continue;
        }
        return None;
    }
    (!diagram.entities.is_empty()).then_some(diagram)
}

fn diagram_lines(body: &str) -> impl Iterator<Item = &str> {
    body.lines()
        .flat_map(|line| line.split(';'))
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"))
}

fn class_entity_index(
    diagram: &mut ClassDiagram,
    index: &mut std::collections::BTreeMap<String, usize>,
    raw_name: &str,
) -> usize {
    let name = entity_display_name(raw_name);
    if let Some(entity) = index.get(raw_name) {
        return *entity;
    }
    let next = diagram.entities.len();
    index.insert(raw_name.to_string(), next);
    diagram.entities.push(ClassEntity {
        name,
        members: Vec::new(),
    });
    next
}

fn entity_display_name(raw_name: &str) -> String {
    let raw_name = raw_name.trim();
    let label = raw_name
        .split_once('[')
        .and_then(|(_, label)| label.strip_suffix(']'))
        .unwrap_or(raw_name)
        .trim_matches('"');
    normalize_class_text(label)
}

fn normalize_class_text(value: &str) -> String {
    let mut normalized = String::new();
    let mut generic_open = false;
    for character in value.trim().chars() {
        if character == '~' {
            normalized.push(if generic_open { '>' } else { '<' });
            generic_open = !generic_open;
        } else {
            normalized.push(character);
        }
    }
    normalized
}

fn class_annotation(line: &str) -> Option<(&str, &str)> {
    let annotation = line.strip_prefix("<<")?;
    let (annotation, name) = annotation.split_once(">>")?;
    let name = name.trim();
    (!annotation.trim().is_empty() && !name.is_empty()).then_some((annotation.trim(), name))
}

fn parse_class_relationship(
    line: &str,
    diagram: &mut ClassDiagram,
    index: &mut std::collections::BTreeMap<String, usize>,
) -> Option<ClassRelationship> {
    let operators = ["<|..", "<|--", "*--", "o--", "..>", "-->", "<--", "--"];
    let (operator, position) = operators
        .iter()
        .find_map(|operator| line.find(operator).map(|position| (*operator, position)))?;
    let from = line[..position].trim().trim_matches('"');
    let after_operator = line[position + operator.len()..].trim();
    let (target, label) = after_operator
        .split_once(':')
        .map_or((after_operator, None), |(target, label)| {
            (target.trim(), Some(label.trim()))
        });
    let target = target.trim_matches('"');
    if from.is_empty() || target.is_empty() {
        return None;
    }
    let from = class_entity_index(diagram, index, from);
    let to = class_entity_index(diagram, index, target);
    let source_head = match operator {
        "<|.." | "<|--" => Some('△'),
        "*--" => Some('◆'),
        "o--" => Some('◇'),
        _ => None,
    };
    Some(ClassRelationship {
        from,
        to,
        label: label.filter(|label| !label.is_empty()).map(str::to_string),
        source_head,
        dotted: operator.contains(".."),
    })
}

fn parse_er_relationship(
    line: &str,
    diagram: &mut ClassDiagram,
    index: &mut std::collections::BTreeMap<String, usize>,
) -> Option<ClassRelationship> {
    let mut tokens = line.split_whitespace();
    let from_name = tokens.next()?;
    let operator = tokens.next()?;
    let to_name = tokens.next()?;
    if !(operator.contains("--") || operator.contains("..")) || !is_mermaid_identifier(from_name) {
        return None;
    }
    let from = class_entity_index(diagram, index, from_name);
    let to = class_entity_index(diagram, index, to_name);
    let relation = line
        .split_once(':')
        .map(|(_, relation)| relation.trim())
        .filter(|relation| !relation.is_empty())
        .unwrap_or_default();
    let (from_cardinality, to_cardinality) = er_cardinalities(operator)?;
    let label = [from_cardinality, relation, to_cardinality]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Some(ClassRelationship {
        from,
        to,
        label: Some(label),
        source_head: None,
        dotted: operator.contains(".."),
    })
}

fn er_cardinalities(operator: &str) -> Option<(&'static str, &'static str)> {
    let divider = if operator.contains("--") { "--" } else { ".." };
    let (left, right) = operator.split_once(divider)?;
    Some((er_cardinality(left)?, er_cardinality(right)?))
}

fn er_cardinality(value: &str) -> Option<&'static str> {
    match value {
        "||" => Some("1"),
        "|o" | "o|" => Some("0..1"),
        "}o" | "o{" => Some("0..*"),
        "}|" | "|{" => Some("1..*"),
        _ => None,
    }
}

fn is_mermaid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '[' | ']' | '"')
        })
}

fn parse_sequence(body: &str) -> Option<SequenceDiagram> {
    let mut lines = diagram_lines(body);
    if !lines
        .next()?
        .split_whitespace()
        .next()?
        .eq_ignore_ascii_case("sequenceDiagram")
    {
        return None;
    }
    let mut participants = Vec::new();
    let mut aliases = std::collections::BTreeMap::new();
    let mut messages = Vec::new();
    let mut rows = Vec::new();
    let mut autonumber = false;
    let mut control_depth = 0usize;
    for statement in lines {
        if statement.eq_ignore_ascii_case("autonumber") {
            autonumber = true;
            continue;
        }
        if let Some((alias, label)) = sequence_participant_declaration(statement) {
            let participant = sequence_participant(&mut participants, &aliases, alias);
            participants[participant] = label.to_string();
            aliases.insert(alias.to_string(), participant);
            continue;
        }
        if let Some(note) = sequence_note(statement) {
            rows.push(SequenceRow::Note(note));
            continue;
        }
        if is_sequence_control_start(statement) {
            rows.push(SequenceRow::Control(statement.to_string()));
            control_depth = control_depth.saturating_add(1);
            continue;
        }
        if statement.eq_ignore_ascii_case("end") {
            if control_depth > 0 {
                rows.push(SequenceRow::End);
                control_depth = control_depth.saturating_sub(1);
            }
            continue;
        }
        if statement.starts_with("rect ") || statement.starts_with("box ") {
            continue;
        }
        if let Some(message) = sequence_message(
            statement,
            &mut participants,
            &aliases,
            autonumber,
            messages.len(),
        ) {
            messages.push(message);
            rows.push(SequenceRow::Message(messages.len() - 1));
        } else {
            return None;
        }
    }
    (!participants.is_empty() && !rows.is_empty()).then_some(SequenceDiagram {
        participants,
        messages,
        rows,
    })
}

fn sequence_participant_declaration(statement: &str) -> Option<(&str, &str)> {
    let statement = statement
        .strip_prefix("participant ")
        .or_else(|| statement.strip_prefix("actor "))?;
    statement.split_once(" as ").map_or_else(
        || Some((statement.trim(), statement.trim())),
        |(alias, label)| {
            let alias = alias.trim();
            let label = label.trim().trim_matches('"');
            (!alias.is_empty() && !label.is_empty()).then_some((alias, label))
        },
    )
}

fn sequence_note(statement: &str) -> Option<String> {
    let statement = statement
        .strip_prefix("Note ")
        .or_else(|| statement.strip_prefix("note "))?;
    statement.split_once(':').and_then(|(_, text)| {
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    })
}

fn is_sequence_control_start(statement: &str) -> bool {
    [
        "loop ",
        "alt ",
        "else ",
        "opt ",
        "par ",
        "and ",
        "critical ",
        "option ",
    ]
    .iter()
    .any(|prefix| statement.starts_with(prefix))
}

fn sequence_message(
    statement: &str,
    participants: &mut Vec<String>,
    aliases: &std::collections::BTreeMap<String, usize>,
    autonumber: bool,
    message_index: usize,
) -> Option<SequenceMessage> {
    let operators = ["-->>", "->>", "-->", "-x", "->"];
    let (operator, position) = operators.iter().find_map(|operator| {
        statement
            .find(operator)
            .map(|position| (*operator, position))
    })?;
    let from = &statement[..position];
    let remainder = &statement[position + operator.len()..];
    let (to, text) = remainder.split_once(':').unwrap_or((remainder, ""));
    let from = from.trim().trim_end_matches(['+', '-']);
    let to = to.trim().trim_end_matches(['+', '-']);
    if from.is_empty() || to.is_empty() {
        return None;
    }
    let mut text = text.trim().to_string();
    if autonumber {
        text = format!("{}. {text}", message_index + 1);
    }
    Some(SequenceMessage {
        from: sequence_participant(participants, aliases, from),
        to: sequence_participant(participants, aliases, to),
        text,
        dotted: operator.starts_with("--"),
        cross: operator == "-x",
    })
}

fn sequence_participant(
    participants: &mut Vec<String>,
    aliases: &std::collections::BTreeMap<String, usize>,
    label: &str,
) -> usize {
    if let Some(index) = aliases.get(label) {
        *index
    } else if let Some(index) = participants
        .iter()
        .position(|participant| participant == label)
    {
        index
    } else {
        participants.push(label.to_string());
        participants.len() - 1
    }
}
