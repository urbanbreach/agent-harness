use super::*;

pub(in crate::ui) fn permission_modal_metadata_line(
    permission: &crate::app::ActivePermissionView,
) -> String {
    let subject = permission
        .tool_label
        .as_deref()
        .map(|tool| format!("tool {tool}"))
        .or_else(|| {
            permission
                .tool_call_id
                .as_deref()
                .map(|tool_call_id| format!("call {tool_call_id}"))
        })
        .unwrap_or_else(|| format!("perm {}", permission.permission_id));

    format!(
        "{} · scopes once=one-shot always=session · dig {} · timeout {}s countdown",
        subject,
        abbreviated_digest(&permission.request_digest),
        permission.timeout_ms / 1_000,
    )
}

fn abbreviated_digest(digest: &str) -> String {
    let mut short = digest.chars().take(6).collect::<String>();
    if digest.chars().count() > 6 {
        short.push('…');
    }
    short
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

    let summary = permission.summary.trim();
    if !summary.is_empty() && !summary.starts_with('{') && !summary.starts_with('[') {
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
) -> &'static str {
    if permission.kind.eq_ignore_ascii_case("question")
        || permission.kind.eq_ignore_ascii_case("ask")
        || permission.kind.eq_ignore_ascii_case("ask_user")
    {
        "Question required"
    } else {
        "Permission required"
    }
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
        "Safest next step: deny. Allow once only after review."
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
) -> Text<'static> {
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

    let mut spans = Vec::new();
    if !single {
        spans.push(Span::styled("⇆", primary_style));
        spans.push(Span::styled(" tab  ", metadata_style));
    }
    if !confirm {
        spans.push(Span::styled("↑↓", primary_style));
        spans.push(Span::styled(" select  ", metadata_style));
    }
    spans.push(Span::styled("enter", primary_style));
    spans.push(Span::styled(format!(" {submit_label}  "), metadata_style));
    spans.push(Span::styled("esc", primary_style));
    spans.push(Span::styled(" dismiss", metadata_style));
    Text::from(Line::from(spans))
}

pub(in crate::ui) fn question_permission_body_text(
    app: &AppState,
    permission: &crate::app::ActivePermissionView,
    prompts: &[crate::app::QuestionPromptView],
    theme: &Theme,
    surface: Color,
) -> Text<'static> {
    if prompts.is_empty() {
        return Text::default();
    }

    let primary_style = Style::default().fg(theme.text.primary).bg(surface);
    let muted_style = Style::default().fg(theme.text.secondary).bg(surface);
    let accent_style = Style::default()
        .fg(theme.text.inverse)
        .bg(ui_chrome::question_prompt_accent(theme));
    let active_surface = theme.surface.panel_elevated;
    let active_number_style = Style::default()
        .fg(question_prompt_tint(
            theme.text.secondary,
            ui_chrome::question_prompt_secondary(theme),
            0.6,
        ))
        .bg(active_surface);
    let active_label_style = Style::default()
        .fg(ui_chrome::question_prompt_secondary(theme))
        .bg(active_surface);
    let success_style = Style::default().fg(theme.status.success).bg(surface);
    let error_style = Style::default().fg(theme.status.error).bg(surface);
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
        lines.push(Line::from(tabs));
        lines.push(Line::default());
    }

    if confirm {
        lines.push(Line::from(vec![Span::styled("Review", primary_style)]));
        for (index, prompt) in prompts.iter().enumerate() {
            let value = answers
                .get(index)
                .map(|value| value.join(", "))
                .unwrap_or_default();
            lines.push(Line::from(vec![
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
            ]));
        }
        return Text::from(lines);
    }

    let prompt = &prompts[tab.min(prompts.len().saturating_sub(1))];
    let selected = app.question_prompt_selection(&permission.permission_id);
    let current_answers = answers.get(tab).cloned().unwrap_or_default();

    lines.push(Line::from(vec![Span::styled(
        if prompt.multiple {
            format!("{} (select all that apply)", prompt.question)
        } else {
            prompt.question.clone()
        },
        primary_style,
    )]));
    lines.push(Line::default());

    for (index, option) in prompt.options.iter().enumerate() {
        let picked = current_answers.iter().any(|value| value == &option.label);
        let active = index == selected;
        let mut row = vec![Span::styled(
            format!("{}.", index + 1),
            if active {
                active_number_style
            } else {
                muted_style
            },
        )];
        row.push(Span::styled(
            " ",
            Style::default().bg(if active { active_surface } else { surface }),
        ));
        row.push(Span::styled(
            if prompt.multiple {
                format!("[{}] {}", if picked { '✓' } else { ' ' }, option.label)
            } else {
                option.label.clone()
            },
            if active {
                active_label_style
            } else if prompt.multiple && picked {
                success_style
            } else {
                primary_style
            },
        ));
        if !prompt.multiple {
            row.push(Span::styled(if picked { "✓" } else { "" }, success_style));
        }
        lines.push(Line::from(row));
        lines.push(Line::from(vec![Span::styled(
            format!("   {}", option.description),
            muted_style,
        )]));
    }

    if prompt.custom {
        let custom_value = app
            .question_prompt_custom(&permission.permission_id, tab)
            .unwrap_or_default();
        let picked =
            !custom_value.is_empty() && current_answers.iter().any(|value| value == custom_value);
        let active = selected == prompt.options.len();
        let mut row = vec![Span::styled(
            format!("{}.", prompt.options.len() + 1),
            if active {
                active_number_style
            } else {
                muted_style
            },
        )];
        row.push(Span::styled(
            " ",
            Style::default().bg(if active { active_surface } else { surface }),
        ));
        row.push(Span::styled(
            if prompt.multiple {
                format!("[{}] Type your own answer", if picked { '✓' } else { ' ' })
            } else {
                "Type your own answer".to_string()
            },
            if active {
                active_label_style
            } else if prompt.multiple && picked {
                success_style
            } else {
                primary_style
            },
        ));
        if !prompt.multiple {
            row.push(Span::styled(if picked { "✓" } else { "" }, success_style));
        }
        lines.push(Line::from(row));

        let editing = app.question_prompt_editing(&permission.permission_id) && active;
        if editing {
            let preview = app.question_answer_preview(&permission.permission_id);
            let (text, style) = if preview == "█" {
                ("Type your own answer".to_string(), muted_style)
            } else {
                (preview, primary_style)
            };
            lines.push(Line::from(vec![Span::styled(format!("   {text}"), style)]));
        } else if !custom_value.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                format!("   {custom_value}"),
                muted_style,
            )]));
        }
    }

    if let Some(error) = app.question_answer_error(&permission.permission_id) {
        lines.push(Line::default());
        lines.push(Line::from(vec![Span::styled(
            error.to_string(),
            error_style,
        )]));
    }

    Text::from(lines)
}

fn question_prompt_tint(base: Color, overlay: Color, alpha: f32) -> Color {
    match (base, overlay) {
        (
            Color::Rgb(base_red, base_green, base_blue),
            Color::Rgb(overlay_red, overlay_green, overlay_blue),
        ) => {
            let blend = |base: u8, overlay: u8| -> u8 {
                let value = (f32::from(base) * (1.0 - alpha)) + (f32::from(overlay) * alpha);
                value.round().clamp(0.0, 255.0) as u8
            };
            Color::Rgb(
                blend(base_red, overlay_red),
                blend(base_green, overlay_green),
                blend(base_blue, overlay_blue),
            )
        }
        _ => overlay,
    }
}
