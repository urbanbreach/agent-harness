use super::*;
use std::borrow::Cow;

pub(super) fn preferred_binding<'a>(
    app: &AppState,
    action: Action,
    preferred: &'a str,
) -> Cow<'a, str> {
    if app.composer.composer_multiline_mode()
        && matches!(action, Action::SubmitPrompt | Action::InsertNewline)
    {
        return super::super::composer_footer_binding(app, action).into();
    }
    let bindings = app.keymap.get_binding_strs(action);
    if bindings.iter().any(|label| label == preferred) {
        preferred.into()
    } else {
        bindings
            .into_iter()
            .next()
            .filter(|label| label != "-")
            .map_or(Cow::Borrowed(preferred), Cow::Owned)
    }
}

pub(super) fn shortcut_row(
    app: &AppState,
    theme: &Theme,
    compact: bool,
    primary_only: bool,
) -> Line<'static> {
    let base = Style::default().bg(theme.surface.canvas);
    let key = base
        .fg(theme.terminal_colors.primary)
        .add_modifier(Modifier::BOLD);
    let label = base.fg(theme.terminal_colors.secondary);
    let separator = label.add_modifier(Modifier::DIM);
    let turn = app.active_turn_in_progress();
    let mut spans = Vec::new();
    let mut push = |binding: Cow<'static, str>, text| {
        if !spans.is_empty() {
            spans.push(Span::styled("  │  ", separator));
        }
        spans.extend([Span::styled(binding, key), Span::styled(text, label)]);
    };
    if turn && app.composer.composer_multiline_mode() && !primary_only {
        push("Enter".into(), ":newline");
        push("Alt+Enter".into(), ":send");
        push(
            preferred_binding(app, Action::InterjectPrompt, "Alt+i"),
            ":interject",
        );
        push(
            preferred_binding(app, Action::CancelAndReplacePrompt, "Alt+r"),
            ":replace",
        );
    } else {
        push(
            preferred_binding(app, Action::SubmitPrompt, "Enter"),
            if turn { ":queue" } else { ":send" },
        );
        if primary_only || !app.composer.prompt_buffer.is_empty() && (turn || compact) {
            push(
                preferred_binding(app, Action::InsertNewline, "Alt+Enter"),
                ":newline",
            );
        }
        if !primary_only && !compact {
            push(
                preferred_binding(app, Action::VariantCycle, "Shift+Tab"),
                ":mode",
            );
        }
        if !primary_only && turn {
            push("Ctrl+c".into(), ":cancel");
        }
        push(preferred_binding(app, Action::Help, "Ctrl+x"), ":shortcuts");
    }
    Line::from(spans)
}

pub(super) fn hint_candidates(app: &AppState, theme: &Theme) -> Vec<Line<'static>> {
    let shortcuts: &[(Action, &str, &str)] = if app.completed_session_shell_active() {
        &[
            (Action::FocusNext, "Tab", " focus"),
            (Action::Palette, "Ctrl+p", " commands"),
            (Action::Quit, "q", " quit"),
        ]
    } else if app.composer_disabled() {
        &[
            (Action::Palette, "Ctrl+p", " commands"),
            (Action::Quit, "q", " quit"),
        ]
    } else if app.interrupt_hint_visible() {
        return vec![line(&[(Cow::Borrowed("ctrl+c"), " interrupt")], theme)];
    } else {
        &[(Action::Palette, "Ctrl+p", " commands")]
    };
    let items = shortcuts
        .iter()
        .map(|(action, fallback, label)| {
            let binding = app
                .keymap
                .get_binding_strs(*action)
                .into_iter()
                .next()
                .map_or(Cow::Borrowed(*fallback), Cow::Owned);
            (binding, *label)
        })
        .collect::<Vec<_>>();
    let mut candidates = (0..items.len())
        .map(|start| line(&items[start..], theme))
        .collect::<Vec<_>>();
    if !app.completed_session_shell_active() && !app.composer_disabled() {
        candidates.push(Line::default());
    }
    candidates
}

fn line(items: &[(Cow<'static, str>, &'static str)], theme: &Theme) -> Line<'static> {
    let base = Style::default().bg(theme.surface.canvas);
    let key = base.fg(theme.text.primary).add_modifier(Modifier::BOLD);
    let label = base.fg(theme.text.secondary);
    let mut spans = Vec::with_capacity(items.len() * 3);
    for (binding, text) in items {
        if !spans.is_empty() {
            spans.push(Span::styled("  ·  ", base.fg(theme.text.tertiary)));
        }
        spans.extend([
            Span::styled(binding.clone(), key),
            Span::styled(*text, label),
        ]);
    }
    Line::from(spans)
}

pub(super) fn starting(app: &AppState, theme: &Theme) -> Line<'static> {
    let glyph = crate::ui::ui_transcript_style::glyph_routed_streaming_spinner_frame(
        theme,
        app.startup_motion_phase(),
        true,
    );
    pulse(glyph, "Starting session…".into(), theme)
}

pub(super) fn background(app: &AppState, theme: &Theme, count: usize) -> Line<'static> {
    let glyph = crate::ui::ui_transcript_style::glyph_routed_monitor_pulse_frame(
        theme,
        app.transcript_animation_phase(),
        true,
    );
    let noun = if count == 1 { "task" } else { "tasks" };
    pulse(
        glyph,
        format!("{count} background {noun} still running").into(),
        theme,
    )
}

fn pulse(glyph: &str, text: Cow<'static, str>, theme: &Theme) -> Line<'static> {
    let base = Style::default().bg(theme.surface.canvas);
    Line::from(vec![
        Span::styled(format!("{glyph} "), base.fg(theme.text.accent)),
        Span::styled(text, base.fg(theme.text.secondary)),
    ])
}
