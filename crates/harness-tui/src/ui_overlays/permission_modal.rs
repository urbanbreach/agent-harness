// allow: SIZE_OK — TUI overlay rendering (indivisible view model)
use super::*;

pub(in crate::ui) fn permission_modal_metadata_line(
    _permission: &crate::app::ActivePermissionView,
) -> String {
    String::new()
}

pub(in crate::ui) fn permission_modal_icon(
    permission: &crate::app::ActivePermissionView,
) -> &'static str {
    let kind = permission.kind.as_str();
    if kind.eq_ignore_ascii_case("question")
        || kind.eq_ignore_ascii_case("ask")
        || kind.eq_ignore_ascii_case("ask_user")
    {
        return "?";
    }
    if kind.eq_ignore_ascii_case("edit")
        || kind.eq_ignore_ascii_case("edit_fs")
        || kind.eq_ignore_ascii_case("lsp")
    {
        return "→";
    }
    if kind.eq_ignore_ascii_case("shell") || kind.eq_ignore_ascii_case("bash") {
        return "#";
    }
    if kind.eq_ignore_ascii_case("task") {
        return "#";
    }
    if kind.eq_ignore_ascii_case("webfetch") {
        return "%";
    }
    if kind.eq_ignore_ascii_case("websearch") {
        return "◈";
    }
    if kind.eq_ignore_ascii_case("codesearch") {
        return "◇";
    }
    "⚙"
}

pub(in crate::ui) fn permission_modal_subject_line(
    permission: &crate::app::ActivePermissionView,
) -> String {
    if permission.kind.eq_ignore_ascii_case("question")
        || permission.kind.eq_ignore_ascii_case("ask")
        || permission.kind.eq_ignore_ascii_case("ask_user")
    {
        return "Answer operator question".to_string();
    }

    if permission_target_path(permission).is_some() {
        return String::new();
    }

    let summary = permission.summary.trim();
    if !summary.is_empty()
        && !summary.starts_with('{')
        && !summary.starts_with('[')
        && !summary.starts_with("tool=")
    {
        return summary.to_string();
    }

    permission
        .tool_label
        .as_deref()
        .map(|tool| format!("Review {tool}"))
        .unwrap_or_else(|| format!("Review {}", permission.kind.replace('_', " ")))
}

pub(in crate::ui) fn permission_modal_title(
    permission: &crate::app::ActivePermissionView,
) -> String {
    if permission.kind.eq_ignore_ascii_case("question")
        || permission.kind.eq_ignore_ascii_case("ask")
        || permission.kind.eq_ignore_ascii_case("ask_user")
    {
        return "Question required".to_string();
    }

    let action = permission_action_label(permission);
    match permission_target_path(permission) {
        Some(path) => format!("Allow {action} to {path}?"),
        None => format!("Allow {action}?"),
    }
}

fn permission_action_label(permission: &crate::app::ActivePermissionView) -> &'static str {
    let kind = permission.kind.as_str();
    if kind.eq_ignore_ascii_case("edit")
        || kind.eq_ignore_ascii_case("edit_fs")
        || kind.eq_ignore_ascii_case("write")
        || kind.eq_ignore_ascii_case("fs.write")
    {
        return "Edit";
    }
    if kind.eq_ignore_ascii_case("shell") || kind.eq_ignore_ascii_case("bash") {
        return "Shell";
    }
    if kind.eq_ignore_ascii_case("read") || kind.eq_ignore_ascii_case("fs.read") {
        return "Read";
    }
    if kind.eq_ignore_ascii_case("task") {
        return "Task";
    }
    if kind.eq_ignore_ascii_case("webfetch") {
        return "Webfetch";
    }
    if kind.eq_ignore_ascii_case("websearch") {
        return "Websearch";
    }
    if kind.eq_ignore_ascii_case("codesearch") {
        return "Codesearch";
    }
    if kind.eq_ignore_ascii_case("lsp") {
        return "LSP";
    }
    "Permission"
}

fn permission_target_path(permission: &crate::app::ActivePermissionView) -> Option<String> {
    let summary = permission.summary.trim();
    if summary.is_empty() {
        return None;
    }

    if let Some(path) = extract_json_string_field(summary, "filePath")
        .or_else(|| extract_json_string_field(summary, "path"))
    {
        return Some(path);
    }

    for marker in [" to ", " path ", " file "] {
        if let Some(idx) = summary.rfind(marker) {
            let tail = summary[idx + marker.len()..].trim();
            let path = tail
                .trim_matches(|c: char| c == '?' || c == '.' || c == '"' || c == '\'')
                .trim();
            if !path.is_empty()
                && !path.contains('=')
                && path
                    .chars()
                    .all(|c| !c.is_whitespace() || c == '/' || c == '\\')
                && path.len() < 240
            {
                return Some(path.to_string());
            }
        }
    }

    None
}

fn extract_json_string_field(source: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\"");
    let key_pos = source.find(&key)?;
    let after_key = &source[key_pos + key.len()..];
    let colon = after_key.find(':')?;
    let mut rest = after_key[colon + 1..].trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    rest = &rest[1..];
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if let Some(escaped) = chars.next() {
                    out.push(escaped);
                }
            }
            '"' => return Some(out),
            _ => out.push(ch),
        }
    }
    None
}

pub(in crate::ui) fn permission_modal_guidance(
    permission: &crate::app::ActivePermissionView,
    submission_pending: bool,
) -> &'static str {
    if submission_pending {
        "Decision recorded. Wait for confirmation before sending another turn."
    } else if permission.kind.eq_ignore_ascii_case("question")
        || permission.kind.eq_ignore_ascii_case("ask")
        || permission.kind.eq_ignore_ascii_case("ask_user")
    {
        "Safest next step: deny. Answer only after review."
    } else {
        "Safest next step: deny. Approve only after review."
    }
}

pub(in crate::ui) fn permission_modal_summary_line(
    permission: &crate::app::ActivePermissionView,
    submission_pending: bool,
) -> String {
    if submission_pending {
        return "Decision submitted — awaiting confirmation.".to_string();
    }

    permission
        .tool_label
        .as_deref()
        .map(|tool| format!("Tool {tool} is paused for review."))
        .unwrap_or_else(|| {
            if permission.summary.chars().count() > 48 {
                format!(
                    "{} request is paused for review.",
                    permission.kind.replace('_', " ")
                )
            } else {
                permission.summary.clone()
            }
        })
}

pub(in crate::ui) fn permission_modal_draft_line(prompt_buffer: &str) -> String {
    let draft = prompt_buffer.trim();
    if draft.is_empty() {
        String::new()
    } else {
        format!("Draft preserved · {draft}")
    }
}

pub(in crate::ui) fn permission_modal_actions_text(
    app: &AppState,
    theme: &Theme,
    surface: Color,
    submission_pending: bool,
    is_question: bool,
) -> Text<'static> {
    let metadata_style = Style::default().fg(theme.text.secondary).bg(surface);
    let primary_style = Style::default().fg(theme.text.primary).bg(surface);

    if submission_pending {
        return Text::from(vec![
            Line::from(vec![
                status_badge("decision sent", theme.status.info, theme),
                Span::styled("  waiting for confirmation", metadata_style),
            ]),
            Line::from(vec![Span::styled(
                "No new action required until confirmation returns.",
                metadata_style,
            )]),
        ]);
    }

    let deny_label = app.keymap.get_binding_label(Action::DenyPermission, "deny");
    let reject_label = app.keymap.get_binding_label(Action::DismissModal, "reject");
    let allow_label = app
        .keymap
        .get_binding_label(Action::AllowPermission, "allow once");

    Text::from(vec![
        Line::from(vec![
            status_badge(deny_label, theme.status.error, theme),
            Span::styled("  default deny · stays fail-closed", metadata_style),
        ]),
        Line::from(vec![
            Span::styled(
                if is_question {
                    format!("{reject_label} rejects the question")
                } else {
                    format!("{reject_label} rejects")
                },
                metadata_style,
            ),
            Span::styled("  ·  ", metadata_style),
            Span::styled(
                if is_question {
                    format!("{allow_label} sends answers")
                } else {
                    allow_label
                },
                primary_style,
            ),
        ]),
    ])
}

pub(in crate::ui) fn question_permission_actions_text(
    app: &AppState,
    permission: &crate::app::ActivePermissionView,
    prompts: &[crate::app::QuestionPromptView],
    theme: &Theme,
    surface: Color,
    content_width: u16,
) -> Text<'static> {
    use crate::keybindings::Action;

    let primary_style = Style::default().fg(theme.text.primary).bg(surface);
    let metadata_style = Style::default().fg(theme.text.secondary).bg(surface);
    let single = prompts.len() == 1 && !prompts[0].multiple;
    let confirm = !single && app.question_prompt_tab(&permission.permission_id) >= prompts.len();
    let submit_label = if confirm {
        "submit"
    } else if prompts
        .get(app.question_prompt_tab(&permission.permission_id))
        .is_some_and(|prompt| prompt.multiple)
    {
        "toggle"
    } else if single {
        "submit"
    } else {
        "confirm"
    };
    // Reference question state packs "y copy"; prefer bare `y` when bound.
    let bindings = app.keymap.get_binding_strs(Action::CopyMessage);
    let copy_key = if bindings.iter().any(|binding| binding == "y") {
        "y".to_string()
    } else {
        app.keymap.get_binding_str(Action::CopyMessage)
    };
    let copy_hint = if copy_key == "-" {
        "y copy".to_string()
    } else {
        format!("{copy_key} copy")
    };

    let mut left = String::new();
    if !single {
        left.push_str("tab switch · ");
    }
    if !confirm {
        left.push_str("↑/↓ navigate · ");
        left.push_str(&copy_hint);
    } else if left.ends_with(" · ") {
        left.truncate(left.len().saturating_sub(3));
    }
    let right = format!("Enter:{submit_label}");
    let left_width = Line::from(left.as_str()).width();
    let right_width = Line::from(right.as_str()).width();
    let target = usize::from(content_width);
    let gap = target
        .saturating_sub(left_width)
        .saturating_sub(right_width)
        .max(if left.is_empty() || right.is_empty() {
            0
        } else {
            1
        });

    let mut spans = Vec::new();
    if !left.is_empty() {
        spans.push(Span::styled(left, primary_style));
    }
    if gap > 0 {
        spans.push(Span::styled(" ".repeat(gap), metadata_style));
    }
    spans.push(Span::styled(right, primary_style));
    Text::from(Line::from(spans))
}

pub(in crate::ui) fn question_permission_body_text(
    app: &AppState,
    permission: &crate::app::ActivePermissionView,
    prompts: &[crate::app::QuestionPromptView],
    theme: &Theme,
    surface: Color,
    content_width: u16,
) -> Text<'static> {
    if prompts.is_empty() {
        return Text::default();
    }

    let primary_style = Style::default()
        .fg(theme.question_prompt.primary)
        .bg(surface);
    let muted_style = Style::default()
        .fg(theme.question_prompt.secondary)
        .bg(surface);
    let question_accent = ui_chrome::question_prompt_accent(theme);
    let accent_style = Style::default()
        .fg(question_accent)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let active_row_style = Style::default()
        .fg(theme.question_prompt.primary)
        .bg(theme.question_prompt.selected);
    let active_number_style = active_row_style
        .fg(question_accent)
        .add_modifier(Modifier::BOLD);
    let active_label_style = active_row_style.add_modifier(Modifier::BOLD);
    let selected_style = Style::default()
        .fg(theme.question_prompt.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let error_style = Style::default().fg(theme.status.error).bg(surface);
    let glyphs = theme.live_shell.transcript_glyphs;
    let single = prompts.len() == 1 && !prompts[0].multiple;
    let tab = app
        .question_prompt_tab(&permission.permission_id)
        .min(prompts.len());
    let confirm = !single && tab >= prompts.len();
    let answers = app.question_prompt_answers(&permission.permission_id);
    let mut lines = Vec::new();

    if !single {
        let mut tabs = Vec::new();
        for (index, prompt) in prompts.iter().enumerate() {
            if index > 0 {
                tabs.push(Span::styled(" ", Style::default().bg(surface)));
            }
            let answered = answers.get(index).is_some_and(|value| !value.is_empty());
            tabs.push(Span::styled(
                format!(" {} ", prompt.header),
                if index == tab {
                    accent_style
                } else if answered {
                    primary_style
                } else {
                    muted_style
                },
            ));
        }
        tabs.push(Span::styled(" ", Style::default().bg(surface)));
        tabs.push(Span::styled(
            " Confirm ",
            if confirm { accent_style } else { muted_style },
        ));
        let tabs_line = Line::from(tabs);
        if tabs_line.width() <= usize::from(content_width) {
            lines.push(tabs_line);
        } else {
            let active = prompts
                .get(tab.min(prompts.len().saturating_sub(1)))
                .map(|prompt| prompt.header.as_str())
                .unwrap_or("Confirm");
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {}/{} {active} ", tab.saturating_add(1), prompts.len()),
                    accent_style,
                ),
                Span::styled(
                    " Confirm ",
                    if confirm { accent_style } else { muted_style },
                ),
            ]));
        }
        lines.push(Line::default());
    }

    if confirm {
        lines.push(Line::from(vec![Span::styled("Review", primary_style)]));
        for (index, prompt) in prompts.iter().enumerate() {
            let value = answers
                .get(index)
                .map(|value| value.join(", "))
                .unwrap_or_default();
            let review_line = Line::from(vec![
                Span::styled(format!("{}: ", prompt.header), muted_style),
                Span::styled(
                    if value.is_empty() {
                        "(not answered)".to_string()
                    } else {
                        value
                    },
                    if answers.get(index).is_some_and(|value| !value.is_empty()) {
                        primary_style
                    } else {
                        error_style
                    },
                ),
            ]);
            lines.extend(wrap_question_line_preserving_spans(
                review_line,
                usize::from(content_width.max(1)),
            ));
        }
        if let Some(error) = app.question_answer_error(&permission.permission_id) {
            lines.push(Line::default());
            lines.extend(wrap_question_line_preserving_spans(
                Line::from(vec![Span::styled(error.to_string(), error_style)]),
                usize::from(content_width.max(1)),
            ));
        }
        return Text::from(lines);
    }

    let prompt = &prompts[tab.min(prompts.len().saturating_sub(1))];
    let selected = app.question_prompt_selection(&permission.permission_id);
    let hovered = app.question_prompt_hovered(&permission.permission_id);
    let current_answers = answers.get(tab).cloned().unwrap_or_default();

    let question_line = Line::from(vec![Span::styled(
        if prompt.multiple {
            format!("{} (select all that apply)", prompt.question)
        } else {
            prompt.question.clone()
        },
        primary_style,
    )]);
    lines.extend(wrap_question_line_preserving_spans(
        question_line,
        usize::from(content_width.max(1)),
    ));
    // Waiting-state layout: two blank rows between title and options.
    lines.push(Line::default());
    lines.push(Line::default());

    // Label-column packing: pad option labels so descriptions share a column
    // (e.g. "Red    Choose red" / "Green  Choose green").
    let label_column_width = prompt
        .options
        .iter()
        .map(|option| display_width(&option.label))
        .max()
        .unwrap_or(0);

    for (index, option) in prompt.options.iter().enumerate() {
        let picked = current_answers.iter().any(|value| value == &option.label);
        let active = index == selected;
        let hovered = hovered == Some(index);
        let row_background = if hovered {
            theme.surface.hover
        } else if active {
            theme.question_prompt.selected
        } else {
            surface
        };
        let marker = if prompt.multiple {
            if picked {
                format!("[{}]", glyphs.choice_checked)
            } else {
                "[ ]".to_string()
            }
        } else if picked {
            glyphs.choice_selected.to_string()
        } else {
            glyphs.choice_unselected.to_string()
        };
        let row_style = (if active {
            active_label_style
        } else if picked {
            selected_style
        } else {
            primary_style
        })
        .bg(row_background);
        let number_style = (if active {
            active_number_style
        } else {
            Style::default().fg(question_accent).bg(surface)
        })
        .bg(row_background);
        let mut spans = vec![Span::styled(
            format!("{} ({marker}) ", index + 1),
            number_style,
        )];
        let label = if option.description.is_empty() {
            option.label.clone()
        } else {
            let pad = label_column_width.saturating_sub(display_width(&option.label));
            format!("{}{}", option.label, " ".repeat(pad))
        };
        spans.push(Span::styled(label, row_style));
        if !option.description.is_empty() {
            spans.push(Span::styled(
                format!("  {}", option.description),
                (if active {
                    active_row_style
                } else {
                    muted_style
                })
                .bg(row_background),
            ));
        }
        let option_line = Line::from(spans).style(Style::default().bg(row_background));
        lines.extend(wrap_question_line_preserving_spans(
            option_line,
            usize::from(content_width.max(1)),
        ));
    }

    if prompt.custom {
        let custom_value = app
            .question_prompt_custom(&permission.permission_id, tab)
            .unwrap_or_default();
        let picked =
            !custom_value.is_empty() && current_answers.iter().any(|value| value == custom_value);
        let active = selected == prompt.options.len();
        let hovered = hovered == Some(prompt.options.len());
        let row_background = if hovered {
            theme.surface.hover
        } else if active {
            theme.question_prompt.selected
        } else {
            surface
        };
        let marker = if prompt.multiple {
            if picked {
                format!("[{}]", glyphs.choice_checked)
            } else {
                "[ ]".to_string()
            }
        } else if picked {
            glyphs.choice_selected.to_string()
        } else {
            glyphs.choice_unselected.to_string()
        };
        let row_style = (if active {
            active_label_style
        } else if picked {
            selected_style
        } else {
            primary_style
        })
        .bg(row_background);
        let number_style = (if active {
            active_number_style
        } else {
            Style::default().fg(question_accent).bg(surface)
        })
        .bg(row_background);
        lines.push(
            Line::from(vec![
                Span::styled(format!("z ({marker}) "), number_style),
                Span::styled("Type your answer here".to_string(), row_style),
            ])
            .style(Style::default().bg(row_background)),
        );

        let editing = app.question_prompt_editing(&permission.permission_id) && active;
        if editing {
            let preview = app.question_answer_preview(&permission.permission_id);
            let (text, style) = if preview == "█" {
                (format!("{} ", glyphs.user_marker), muted_style)
            } else {
                (format!("{} {preview}", glyphs.user_marker), primary_style)
            };
            lines.push(Line::from(vec![Span::styled(format!("    {text}"), style)]));
        } else if !custom_value.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                format!("    {} {custom_value}", glyphs.user_marker),
                muted_style,
            )]));
        }
    }

    if let Some(error) = app.question_answer_error(&permission.permission_id) {
        lines.push(Line::default());
        lines.extend(wrap_question_line_preserving_spans(
            Line::from(vec![Span::styled(error.to_string(), error_style)]),
            usize::from(content_width.max(1)),
        ));
    }

    Text::from(lines)
}

#[derive(Debug, Clone, Copy)]
struct QuestionVisualChar {
    ch: char,
    style: Style,
    width: usize,
}

fn wrap_question_line_preserving_spans(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 || line.width() <= width {
        return vec![line];
    }

    let line_style = line.style;
    let chars = line
        .spans
        .into_iter()
        .flat_map(|span| {
            let style = span.style;
            span.content
                .chars()
                .map(move |ch| QuestionVisualChar {
                    ch,
                    style,
                    width: Line::from(ch.to_string()).width().max(1),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    wrap_question_chars_by_word(&chars, width)
        .into_iter()
        .map(|line| line.style(line_style))
        .collect()
}

fn wrap_question_chars_by_word(chars: &[QuestionVisualChar], width: usize) -> Vec<Line<'static>> {
    if chars.is_empty() {
        return vec![Line::default()];
    }

    let mut lines = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let fit_end = question_fit_end(chars, start, width);
        if fit_end >= chars.len() {
            lines.push(question_chars_to_line(&chars[start..]));
            break;
        }

        if let Some(break_at) = chars[start..fit_end]
            .iter()
            .rposition(|visual_char| visual_char.ch.is_whitespace())
            .map(|offset| start + offset)
            .filter(|break_at| *break_at > start)
        {
            let end = break_at + 1;
            lines.push(question_chars_to_line(&chars[start..end]));
            start = end;
        } else if chars[fit_end].ch.is_whitespace() {
            lines.push(question_chars_to_line(&chars[start..fit_end]));
            start = fit_end + 1;
        } else {
            lines.push(question_chars_to_line(&chars[start..fit_end]));
            start = fit_end;
        }
    }
    lines
}

fn question_fit_end(chars: &[QuestionVisualChar], start: usize, width: usize) -> usize {
    let mut used = 0usize;
    for (position, visual_char) in chars.iter().enumerate().skip(start) {
        if position > start && used.saturating_add(visual_char.width) > width {
            return position;
        }
        used = used.saturating_add(visual_char.width);
    }
    chars.len()
}

fn question_chars_to_line(chars: &[QuestionVisualChar]) -> Line<'static> {
    if chars.is_empty() {
        return Line::default();
    }

    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_style = chars[0].style;
    for visual_char in chars {
        if visual_char.style == current_style {
            current.push(visual_char.ch);
        } else {
            spans.push(Span::styled(std::mem::take(&mut current), current_style));
            current_style = visual_char.style;
            current.push(visual_char.ch);
        }
    }
    spans.push(Span::styled(current, current_style));
    Line::from(spans)
}
