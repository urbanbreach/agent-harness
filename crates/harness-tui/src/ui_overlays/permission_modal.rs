// allow: SIZE_OK — TUI overlay rendering (indivisible view model)
use super::*;
use unicode_segmentation::UnicodeSegmentation as _;

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
    let muted_style = Style::default().fg(theme.text.secondary).bg(surface);
    let navigation_key_style = Style::default()
        .fg(ui_chrome::question_prompt_accent(theme))
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let navigation_label_style = muted_style.add_modifier(Modifier::BOLD);
    let action_key_style = navigation_key_style.bg(theme.question_prompt.selected);
    let action_label_style = muted_style.bg(theme.question_prompt.selected);
    let tab = app
        .question_prompt_tab(&permission.permission_id)
        .min(prompts.len().saturating_sub(1));
    let full_navigation = if prompts.len() > 1 {
        format!(
            "[{}/{}] ↑/↓ navigate · ←/→ question · y copy",
            tab.saturating_add(1),
            prompts.len()
        )
    } else {
        "↑/↓ navigate · y copy".to_string()
    };
    let prompt = prompts.get(tab);
    let selected = app.question_prompt_selection(&permission.permission_id);
    let enter_label = if app.question_prompt_editing(&permission.permission_id) {
        "commit"
    } else if prompt.is_some_and(|prompt| prompt.custom && selected == prompt.options.len()) {
        "edit"
    } else if tab + 1 >= prompts.len() {
        "submit"
    } else {
        "select"
    };
    let action = format!("Enter:{enter_label}");
    let navigation_width = usize::from(content_width)
        .saturating_sub(display_width(&action))
        .saturating_sub(1);
    let compact_navigation = if prompts.len() > 1 {
        format!(
            "[{}/{}] ↑/↓ · ←/→ · y",
            tab.saturating_add(1),
            prompts.len()
        )
    } else {
        "↑/↓ · y".to_string()
    };
    let navigation_kind = if display_width(&full_navigation) <= navigation_width {
        2
    } else if display_width(&compact_navigation) <= navigation_width {
        1
    } else {
        0
    };
    let navigation = match navigation_kind {
        2 => &full_navigation,
        1 => &compact_navigation,
        _ => "",
    };
    let visible_action = truncate_question_text_with_ellipsis(&action, usize::from(content_width));
    let padding = usize::from(content_width)
        .saturating_sub(display_width(&navigation))
        .saturating_sub(display_width(&visible_action));
    let mut spans = Vec::new();
    if prompts.len() > 1 && navigation_kind > 0 {
        spans.push(Span::styled(
            format!("[{}/{}] ", tab.saturating_add(1), prompts.len()),
            navigation_label_style,
        ));
    }
    if navigation_kind > 0 {
        spans.push(Span::styled("↑/↓", navigation_key_style));
        spans.push(Span::styled(
            if navigation_kind == 2 {
                " navigate · "
            } else {
                " · "
            },
            navigation_label_style,
        ));
        if prompts.len() > 1 {
            spans.push(Span::styled("←/→", navigation_key_style));
            spans.push(Span::styled(
                if navigation_kind == 2 {
                    " question · "
                } else {
                    " · "
                },
                navigation_label_style,
            ));
        }
        spans.push(Span::styled("y", navigation_key_style));
        if navigation_kind == 2 {
            spans.push(Span::styled(" copy", navigation_label_style));
        }
    }
    spans.push(Span::styled(" ".repeat(padding), muted_style));
    if visible_action == action {
        spans.push(Span::styled("Enter", action_key_style));
        spans.push(Span::styled(format!(":{enter_label}"), action_label_style));
    } else {
        spans.push(Span::styled(visible_action, action_key_style));
    }
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
    let active_row_style = Style::default()
        .fg(theme.question_prompt.primary)
        .bg(theme.question_prompt.selected);
    let selected_style = Style::default()
        .fg(theme.question_prompt.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let error_style = Style::default().fg(theme.status.error).bg(surface);
    let tab = app
        .question_prompt_tab(&permission.permission_id)
        .min(prompts.len().saturating_sub(1));
    let answers = app.question_prompt_answers(&permission.permission_id);
    let mut lines = Vec::new();
    let prompt = &prompts[tab];
    let selected = app.question_prompt_selection(&permission.permission_id);
    let hovered = app.question_prompt_hovered(&permission.permission_id);
    let fullscreen = app.question_prompt_fullscreen(&permission.permission_id);
    let current_answers = answers.get(tab).cloned().unwrap_or_default();

    let (question_label, question_description) = prompt
        .question
        .split_once("\n\n")
        .map_or((prompt.question.trim(), ""), |(label, description)| {
            (label.trim(), description.trim())
        });
    let question_line = Line::from(vec![Span::styled(
        question_label.to_string(),
        primary_style.add_modifier(Modifier::BOLD),
    )]);
    lines.extend(wrap_question_line_preserving_spans(
        question_line,
        usize::from(content_width.max(1)),
    ));
    if !question_description.is_empty() {
        lines.push(Line::default());
        let mut wrapped = wrap_question_line_preserving_spans(
            Line::from(Span::styled(question_description.to_string(), muted_style)),
            usize::from(content_width.max(1)),
        );
        if !fullscreen && wrapped.len() > 4 {
            wrapped.truncate(3);
            wrapped.push(Line::from(Span::styled(
                "... Ctrl-F to expand",
                muted_style,
            )));
        }
        lines.extend(wrapped);
    }
    if let Some(preview) = prompt
        .options
        .get(selected)
        .and_then(|option| option.preview.as_deref())
        .filter(|preview| !preview.is_empty())
    {
        lines.push(Line::default());
        let mut wrapped = wrap_question_line_preserving_spans(
            Line::from(Span::styled(preview.to_string(), muted_style)),
            usize::from(content_width.max(1)),
        );
        if !fullscreen && wrapped.len() > 3 {
            wrapped.truncate(2);
            wrapped.push(Line::from(Span::styled(
                "... Ctrl-F to expand",
                muted_style,
            )));
        }
        lines.extend(wrapped);
    }
    // Waiting-state layout: two blank rows between title and options.
    lines.push(Line::default());
    lines.push(Line::default());

    // Label-column packing: pad option labels so descriptions share a column
    // (e.g. "Red    Choose red" / "Green  Choose green").
    let label_column_width =
        crate::layout::question_label_column_width(&prompt.options, usize::from(content_width));

    for (index, option) in prompt.options.iter().enumerate() {
        let normalized_label = normalize_question_label(&option.label);
        let picked = current_answers.iter().any(|value| value == &option.label);
        let active = index == selected;
        let hovered = hovered == Some(index);
        let row_background = question_row_background(theme, surface, hovered, active);
        let marker = question_choice_marker(theme, prompt.multiple, picked);
        let row_style = question_row_style(theme, row_background, active, picked);
        let number_style = Style::default().fg(question_accent).bg(row_background);
        let marker_style = (if picked { selected_style } else { muted_style }).bg(row_background);
        let shortcut =
            crate::app::permissions::question_option_shortcut_label(index).unwrap_or(' ');
        let prefix = vec![
            Span::styled(format!("{shortcut} "), number_style),
            Span::styled(format!("{marker} "), marker_style),
        ];
        let prefix_width = prefix.iter().map(|span| span.width()).sum::<usize>();
        if active && display_width(&normalized_label) > label_column_width {
            let label_width = usize::from(content_width)
                .saturating_sub(prefix_width)
                .max(1);
            for (line_index, label_line) in wrap_question_line_preserving_spans(
                Line::from(Span::styled(normalized_label.clone(), row_style)),
                label_width,
            )
            .into_iter()
            .enumerate()
            {
                let mut line_spans = if line_index == 0 {
                    prefix.clone()
                } else {
                    vec![Span::styled(" ".repeat(prefix_width), row_style)]
                };
                line_spans.extend(label_line.spans);
                lines.push(Line::from(line_spans).style(Style::default().bg(row_background)));
            }
            if !option.description.is_empty() {
                for description_line in wrap_question_line_preserving_spans(
                    Line::from(Span::styled(option.description.clone(), active_row_style)),
                    label_width,
                ) {
                    let mut line_spans =
                        vec![Span::styled(" ".repeat(prefix_width), active_row_style)];
                    line_spans.extend(description_line.spans);
                    lines.push(Line::from(line_spans).style(Style::default().bg(row_background)));
                }
            }
            continue;
        }
        let label = if option.description.is_empty() {
            normalized_label
        } else {
            let label = truncate_question_text_with_ellipsis(&normalized_label, label_column_width);
            let pad = label_column_width.saturating_sub(display_width(&label));
            format!("{label}{}", " ".repeat(pad))
        };
        if active && !option.description.is_empty() {
            let description_indent = prefix_width
                .saturating_add(label_column_width)
                .saturating_add(2);
            let description_width = usize::from(content_width)
                .saturating_sub(description_indent)
                .max(1);
            for (line_index, description_line) in wrap_question_line_preserving_spans(
                Line::from(Span::styled(option.description.clone(), active_row_style)),
                description_width,
            )
            .into_iter()
            .enumerate()
            {
                let mut line_spans = if line_index == 0 {
                    let mut first = prefix.clone();
                    first.push(Span::styled(label.clone(), row_style));
                    first.push(Span::styled("  ", active_row_style));
                    first
                } else {
                    vec![Span::styled(
                        " ".repeat(description_indent),
                        active_row_style,
                    )]
                };
                line_spans.extend(description_line.spans);
                lines.push(Line::from(line_spans).style(Style::default().bg(row_background)));
            }
            continue;
        }
        let mut spans = prefix;
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
        if active {
            lines.extend(wrap_question_line_preserving_spans(
                option_line,
                usize::from(content_width.max(1)),
            ));
        } else {
            lines.push(truncate_question_line_with_ellipsis(
                option_line,
                usize::from(content_width.max(1)),
            ));
        }
    }

    if let Some(custom_row) = question_custom_row(
        app,
        permission,
        prompt,
        theme,
        QuestionCustomRowLayout {
            surface,
            content_width,
            tab,
            selected,
            hovered,
        },
    ) {
        lines.push(custom_row);
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

fn question_row_background(theme: &Theme, surface: Color, hovered: bool, active: bool) -> Color {
    if hovered {
        theme.surface.hover
    } else if active {
        theme.question_prompt.selected
    } else {
        surface
    }
}

fn question_choice_marker(theme: &Theme, multiple: bool, picked: bool) -> String {
    if multiple {
        return if picked { "[x]" } else { "[ ]" }.to_string();
    }
    let glyphs = theme.live_shell.transcript_glyphs;
    format!(
        "({})",
        if picked {
            glyphs.choice_selected
        } else {
            glyphs.choice_unselected
        }
    )
}

fn question_row_style(theme: &Theme, background: Color, active: bool, picked: bool) -> Style {
    let style = Style::default()
        .fg(theme.question_prompt.primary)
        .bg(background);
    if active || picked {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

struct QuestionCustomRowLayout {
    surface: Color,
    content_width: u16,
    tab: usize,
    selected: usize,
    hovered: Option<usize>,
}

fn question_custom_row(
    app: &AppState,
    permission: &crate::app::ActivePermissionView,
    prompt: &crate::app::QuestionPromptView,
    theme: &Theme,
    layout: QuestionCustomRowLayout,
) -> Option<Line<'static>> {
    if !prompt.custom {
        return None;
    }
    let custom_value = app
        .question_prompt_custom(&permission.permission_id, layout.tab)
        .unwrap_or_default();
    let picked = app.question_prompt_custom_selected(&permission.permission_id, layout.tab);
    let active = layout.selected == prompt.options.len();
    let background = question_row_background(
        theme,
        layout.surface,
        layout.hovered == Some(prompt.options.len()),
        active,
    );
    let marker = question_choice_marker(theme, prompt.multiple, picked);
    let row_style = question_row_style(theme, background, active, picked);
    let number_style = Style::default()
        .fg(ui_chrome::question_prompt_accent(theme))
        .bg(background)
        .add_modifier(if active {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });
    let editing = app.question_prompt_editing(&permission.permission_id) && active;
    let glyphs = theme.live_shell.transcript_glyphs;
    let (text, text_style) = if editing {
        (
            format!(
                "{} {}",
                glyphs.user_marker,
                app.question_answer_preview(&permission.permission_id)
            ),
            row_style,
        )
    } else if picked {
        (format!("{} {custom_value}", glyphs.user_marker), row_style)
    } else if !custom_value.is_empty() {
        (
            custom_value.to_string(),
            Style::default()
                .fg(theme.question_prompt.secondary)
                .bg(background),
        )
    } else {
        ("Type your answer here".to_string(), row_style)
    };
    Some(truncate_question_line_with_ellipsis(
        Line::from(vec![
            Span::styled(format!("z {marker} "), number_style),
            Span::styled(text, text_style),
        ])
        .style(Style::default().bg(background)),
        usize::from(layout.content_width.max(1)),
    ))
}

#[derive(Debug, Clone)]
struct QuestionVisualCluster {
    text: String,
    style: Style,
    width: usize,
}

fn wrap_question_line_preserving_spans(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![line];
    }

    let line_style = line.style;
    let mut logical_lines = vec![Vec::new()];
    for span in line.spans {
        for text in span.content.as_ref().graphemes(true) {
            if text == "\n" {
                logical_lines.push(Vec::new());
            } else if let Some(logical_line) = logical_lines.last_mut() {
                logical_line.push(QuestionVisualCluster {
                    text: text.to_string(),
                    style: span.style,
                    width: Line::from(text.to_string()).width().max(1),
                });
            }
        }
    }
    logical_lines
        .into_iter()
        .flat_map(|clusters| wrap_question_clusters_by_word(&clusters, width))
        .map(|line| line.style(line_style))
        .collect()
}

fn truncate_question_line_with_ellipsis(line: Line<'static>, width: usize) -> Line<'static> {
    if width == 0 || line.width() <= width {
        return line;
    }

    let line_style = line.style;
    let clusters = line
        .spans
        .into_iter()
        .flat_map(|span| {
            let style = span.style;
            span.content
                .as_ref()
                .graphemes(true)
                .map(move |text| QuestionVisualCluster {
                    text: text.to_string(),
                    style,
                    width: Line::from(text.to_string()).width().max(1),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let target = width.saturating_sub(1);
    let mut used = 0usize;
    let end = clusters
        .iter()
        .position(|visual_char| {
            let fits = used.saturating_add(visual_char.width) <= target;
            if fits {
                used = used.saturating_add(visual_char.width);
            }
            !fits
        })
        .unwrap_or(clusters.len());
    let ellipsis_style = clusters
        .get(end.saturating_sub(1))
        .or_else(|| clusters.first())
        .map_or(Style::default(), |visual_char| visual_char.style);
    let mut truncated = question_clusters_to_line(&clusters[..end]);
    truncated
        .spans
        .push(Span::styled("…".to_string(), ellipsis_style));
    truncated.style(line_style)
}

fn truncate_question_text_with_ellipsis(text: &str, width: usize) -> String {
    if display_width(text) <= width {
        return text.to_string();
    }
    let target = width.saturating_sub(1);
    let mut truncated = String::new();
    let mut used = 0usize;
    for grapheme in text.graphemes(true) {
        let grapheme_width = display_width(grapheme).max(1);
        if used.saturating_add(grapheme_width) > target {
            break;
        }
        truncated.push_str(grapheme);
        used = used.saturating_add(grapheme_width);
    }
    truncated.push('…');
    truncated
}

fn normalize_question_label(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn wrap_question_clusters_by_word(
    clusters: &[QuestionVisualCluster],
    width: usize,
) -> Vec<Line<'static>> {
    if clusters.is_empty() {
        return vec![Line::default()];
    }

    let mut lines = Vec::new();
    let mut start = 0usize;
    while start < clusters.len() {
        let fit_end = question_fit_end(clusters, start, width);
        if fit_end >= clusters.len() {
            lines.push(question_clusters_to_line(&clusters[start..]));
            break;
        }

        if let Some(break_at) = clusters[start..fit_end]
            .iter()
            .rposition(|cluster| cluster.text.chars().all(char::is_whitespace))
            .map(|offset| start + offset)
            .filter(|break_at| *break_at > start)
        {
            let end = break_at + 1;
            lines.push(question_clusters_to_line(&clusters[start..end]));
            start = end;
        } else if clusters[fit_end].text.chars().all(char::is_whitespace) {
            lines.push(question_clusters_to_line(&clusters[start..fit_end]));
            start = fit_end + 1;
        } else {
            lines.push(question_clusters_to_line(&clusters[start..fit_end]));
            start = fit_end;
        }
    }
    lines
}

fn question_fit_end(clusters: &[QuestionVisualCluster], start: usize, width: usize) -> usize {
    let mut used = 0usize;
    for (position, cluster) in clusters.iter().enumerate().skip(start) {
        if position > start && used.saturating_add(cluster.width) > width {
            return position;
        }
        used = used.saturating_add(cluster.width);
    }
    clusters.len()
}

fn question_clusters_to_line(clusters: &[QuestionVisualCluster]) -> Line<'static> {
    if clusters.is_empty() {
        return Line::default();
    }

    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_style = clusters[0].style;
    for cluster in clusters {
        if cluster.style == current_style {
            current.push_str(&cluster.text);
        } else {
            spans.push(Span::styled(std::mem::take(&mut current), current_style));
            current_style = cluster.style;
            current.push_str(&cluster.text);
        }
    }
    spans.push(Span::styled(current, current_style));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn question_truncation_keeps_combining_graphemes_intact() {
        // arrange
        let line = Line::from("ae\u{301}bc");

        // act
        let truncated = truncate_question_line_with_ellipsis(line, 3);

        // assert
        assert_eq!(line_text(&truncated), "ae\u{301}…");
    }

    #[test]
    fn question_wrapping_keeps_combining_graphemes_intact() {
        // arrange
        let line = Line::from("ae\u{301}bc");

        // act
        let wrapped = wrap_question_line_preserving_spans(line, 2);
        let text = wrapped.iter().map(line_text).collect::<Vec<_>>();

        // assert
        assert_eq!(text, ["ae\u{301}", "bc"]);
    }

    #[test]
    fn question_wrapping_preserves_explicit_newlines_as_rows() {
        // arrange
        let line = Line::from(Span::raw("alpha\nbeta".to_string()));

        // act
        let wrapped = wrap_question_line_preserving_spans(line, 20);
        let text = wrapped.iter().map(line_text).collect::<Vec<_>>();

        // assert
        assert_eq!(text, ["alpha", "beta"]);
    }
}
