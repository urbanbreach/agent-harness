// allow: SIZE_OK — TUI control dock rendering (indivisible view model)
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::app::{AppState, RuntimeStateKind};
use crate::keybindings::Action;
use crate::theme::Theme;

use super::{
    composer_agent_accent, composer_input_accent, composer_input_muted, composer_input_text,
    control_dock_surface, truncate_plain_text,
};
use crate::ui::ui_context_budget::ContextBudget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisclosureTone {
    Accent,
    Primary,
    Secondary,
    Tertiary,
}

fn runtime_kind_disclosure_tone(kind: RuntimeStateKind) -> DisclosureTone {
    match kind {
        RuntimeStateKind::Success => DisclosureTone::Accent,
        RuntimeStateKind::Streaming | RuntimeStateKind::Sending | RuntimeStateKind::Ready => {
            DisclosureTone::Primary
        }
        RuntimeStateKind::Failure
        | RuntimeStateKind::Cancelled
        | RuntimeStateKind::Disconnected
        | RuntimeStateKind::PermissionBlocked => DisclosureTone::Accent,
        RuntimeStateKind::Degraded | RuntimeStateKind::PermissionPending => DisclosureTone::Primary,
    }
}

fn control_dock_summary_tone_to_disclosure_tone(
    tone: crate::view_model::ControlDockSummaryTone,
) -> DisclosureTone {
    match tone {
        crate::view_model::ControlDockSummaryTone::Secondary => DisclosureTone::Secondary,
        crate::view_model::ControlDockSummaryTone::Accent => DisclosureTone::Accent,
        crate::view_model::ControlDockSummaryTone::Success => DisclosureTone::Accent,
        crate::view_model::ControlDockSummaryTone::Warning => DisclosureTone::Primary,
        crate::view_model::ControlDockSummaryTone::Error => DisclosureTone::Accent,
    }
}

fn disclosure_color(tone: DisclosureTone, theme: &Theme, _agent_accent: Color) -> Color {
    match tone {
        DisclosureTone::Accent => composer_input_accent(theme),
        DisclosureTone::Primary => composer_input_text(theme),
        DisclosureTone::Secondary => composer_input_muted(theme),
        DisclosureTone::Tertiary => theme.text.tertiary,
    }
}

pub(super) fn composer_shortcut_hints(app: &AppState, composer_disabled: bool) -> String {
    let newline = composer_newline_binding_hint(app);
    if app.startup_shell_visible() {
        let palette = app
            .keymap
            .get_binding_str(Action::Palette)
            .to_ascii_lowercase();
        let variant = app
            .keymap
            .get_binding_str(Action::VariantCycle)
            .to_ascii_lowercase();
        let focus = app
            .keymap
            .get_binding_str(Action::FocusNext)
            .to_ascii_lowercase();
        return format!("{variant} variants  {focus} agents  {palette} commands");
    }

    if composer_disabled || app.completed_session_shell_active() {
        return app.keymap.get_binding_label(Action::Palette, "commands");
    }

    let send = app.keymap.get_binding_label(Action::SubmitPrompt, "send");
    let history = composer_history_binding_hint(app);
    if history == "-" {
        format!("{send} · {newline} newline")
    } else {
        format!("{send} · {newline} newline · {history} history")
    }
}

fn compact_usage_count(value: u64) -> String {
    if value >= 1_000_000 {
        return format!(
            "{:.1}M",
            f64::from(u32::try_from(value).unwrap_or(u32::MAX)) / 1_000_000.0
        );
    }
    if value >= 1_000 {
        return format!(
            "{:.1}K",
            f64::from(u32::try_from(value).unwrap_or(u32::MAX)) / 1_000.0
        );
    }
    value.to_string()
}

pub(super) fn composer_context_summary_candidates(
    app: &AppState,
    theme: &Theme,
    surface: Color,
) -> Vec<Vec<Span<'static>>> {
    let metrics = app.compaction_usage_metrics();
    let budget = ContextBudget::from_app(app);
    let mut primary = budget
        .as_ref()
        .map(|budget| {
            vec![context_budget_segment(
                budget.full_label(),
                budget,
                theme,
                surface,
            )]
        })
        .unwrap_or_default();
    append_composer_compaction_metrics(&mut primary, app, metrics, theme, surface, false);

    let mut candidates = Vec::new();
    if !primary.is_empty() {
        candidates.push(primary);
    }
    if let Some(budget) = budget.as_ref() {
        let mut compact = vec![context_budget_segment(
            budget.compact_label(),
            budget,
            theme,
            surface,
        )];
        append_composer_compaction_metrics(&mut compact, app, metrics, theme, surface, true);
        candidates.push(compact);
    }
    if metrics.completed_count > 0 {
        candidates.push(composer_compaction_only_summary(
            app, metrics, theme, surface,
        ));
    }
    candidates.push(Vec::new());
    candidates
}

fn append_composer_compaction_metrics(
    spans: &mut Vec<Span<'static>>,
    app: &AppState,
    metrics: crate::app::CompactionUsageMetrics,
    theme: &Theme,
    surface: Color,
    compact: bool,
) {
    let compaction_text = composer_compaction_metrics_text(app, metrics, compact);
    if compaction_text.is_empty() {
        return;
    }
    if !spans.is_empty() {
        spans.push(disclosure_separator(theme, surface));
    }
    spans.push(disclosure_segment(
        compaction_text,
        DisclosureTone::Secondary,
        theme,
        surface,
    ));
}

fn composer_compaction_only_summary(
    app: &AppState,
    metrics: crate::app::CompactionUsageMetrics,
    theme: &Theme,
    surface: Color,
) -> Vec<Span<'static>> {
    let text = composer_compaction_metrics_text(app, metrics, false);
    if text.is_empty() {
        Vec::new()
    } else {
        vec![disclosure_segment(
            text,
            DisclosureTone::Secondary,
            theme,
            surface,
        )]
    }
}

fn composer_compaction_metrics_text(
    app: &AppState,
    metrics: crate::app::CompactionUsageMetrics,
    compact: bool,
) -> String {
    if metrics.completed_count == 0 {
        return app
            .compaction_status()
            .and_then(|status| {
                (!matches!(status.state, crate::app::CompactionState::Applied))
                    .then(|| status.message.clone())
            })
            .unwrap_or_default();
    }

    let count_label = if compact { "cmp" } else { "compactions" };
    let summary_label = if compact { "sum" } else { "summary" };
    let mut parts = vec![format!("{count_label} {}", metrics.completed_count)];
    parts.push(format!(
        "{summary_label} {} tok",
        compact_usage_count(metrics.summary_tokens_estimate)
    ));
    if metrics.reduction_tokens_estimate > 0 && !compact {
        parts.push(format!(
            "saved {} tok",
            compact_usage_count(metrics.reduction_tokens_estimate)
        ));
    } else if let Some(percent) = metrics
        .last_reduction_percent_estimate
        .filter(|value| *value > 0)
    {
        parts.push(format!("{percent}% saved"));
    }
    parts.join(" · ")
}

fn composer_newline_binding_hint(app: &AppState) -> String {
    let bindings = app.keymap.get_binding_strs(Action::InsertNewline);
    match bindings.as_slice() {
        [] => "-".to_string(),
        [binding] => binding.clone(),
        [first, second, ..] => format!("{first}/{second}"),
    }
}

fn composer_history_binding_hint(app: &AppState) -> String {
    let up = app
        .keymap
        .get_binding_strs(Action::HistoryUp)
        .into_iter()
        .next();
    let down = app
        .keymap
        .get_binding_strs(Action::HistoryDown)
        .into_iter()
        .next();
    match (up, down) {
        (Some(up), Some(down)) => format!("{up}/{down}"),
        (Some(up), None) => up,
        (None, Some(down)) => down,
        (None, None) => "-".to_string(),
    }
}

pub(super) fn status_context(
    app: &AppState,
    theme: &Theme,
    state: RuntimeStateKind,
) -> (&'static str, Color) {
    if app.startup_shell_visible() {
        return ("startup", theme.text.secondary);
    }
    if app.completed_session_shell_active() {
        return match state {
            RuntimeStateKind::Failure => ("recovery", theme.status.warning),
            _ => ("complete", theme.status.success),
        };
    }
    if app.replay_mode {
        return ("replay", theme.text.secondary);
    }
    if app.details_drawer_open() {
        return ("details", theme.border.focus);
    }
    if matches!(
        state,
        RuntimeStateKind::Sending | RuntimeStateKind::Streaming
    ) {
        return ("live", composer_agent_accent(theme, app));
    }
    ("live", theme.text.secondary)
}

pub(super) fn render_replay_read_only_composer_content(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    dock: &crate::view_model::ControlDockViewModel,
) {
    let surface = control_dock_surface(theme, dock.variant);

    let content_area = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(1),
    );
    if content_area.height == 0 {
        return;
    }

    let hint_visible = content_area.height > 1;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(content_area);

    let rail = "▎ ";
    let body = truncate_plain_text(
        &dock.composer_body,
        usize::from(rows[0].width).saturating_sub(rail.chars().count()),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(rail, Style::default().fg(theme.status.disabled).bg(surface)),
            Span::styled(body, Style::default().fg(theme.status.disabled).bg(surface)),
        ])),
        rows[0],
    );

    if hint_visible && rows[1].height > 0 {
        let hint_prefix = "  ";
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(hint_prefix, Style::default().bg(surface)),
                Span::styled(
                    truncate_plain_text(
                        &dock.composer_disclosure,
                        usize::from(rows[1].width).saturating_sub(hint_prefix.chars().count()),
                    ),
                    Style::default().fg(theme.text.secondary).bg(surface),
                ),
            ])),
            rows[1],
        );
    }
}

pub(super) fn render_control_dock_disclosure(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    theme: &Theme,
    dock: &crate::view_model::ControlDockViewModel,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = control_dock_surface(theme, dock.variant);
    let base = Style::default().bg(surface);

    frame.render_widget(Block::default().style(base), area);
    if dock.variant == crate::view_model::ControlDockVariant::Startup {
        let palette = app.keymap.get_binding_str(Action::Palette);
        let newline = composer_newline_binding_hint(app);
        let candidates = startup_disclosure_candidates(theme, surface, &palette, &newline);
        let spans = candidates
            .into_iter()
            .find(|candidate| startup_disclosure_width(candidate) <= usize::from(area.width))
            .unwrap_or_else(|| {
                vec![Span::styled(
                    truncate_plain_text(&palette.to_ascii_lowercase(), usize::from(area.width)),
                    Style::default()
                        .fg(theme.text.primary)
                        .bg(surface)
                        .add_modifier(Modifier::BOLD),
                )]
            });
        frame.render_widget(
            Paragraph::new(Line::from(spans))
                .style(base)
                .alignment(Alignment::Right),
            area,
        );
        return;
    }

    let active_live_composer =
        dock.variant == crate::view_model::ControlDockVariant::Live && !dock.composer_disabled;
    let clear_prompt_confirmation_pending = app.clear_prompt_confirmation_pending();
    let context_summary_visible = app.current_request_budget_snapshot().is_some()
        || app.uses_unknown_budget_fallback()
        || app.active_context_usage().is_some()
        || app.compaction_usage_metrics().completed_count > 0;

    if active_live_composer
        && !clear_prompt_confirmation_pending
        && app.starting_session_seed_visible()
    {
        let row = starting_session_seed_row(app.startup_motion_phase(), theme, surface);
        frame.render_widget(Paragraph::new(Line::from(row)).style(base), area);
        return;
    }

    let background_task_count = app.active_background_task_count();
    if active_live_composer
        && !clear_prompt_confirmation_pending
        && background_task_count > 0
        && !app.active_turn_in_progress()
    {
        let freeze_row = live_freeze_shortcut_disclosure_row(
            app,
            theme,
            surface,
            area.width <= theme.live_shell.breakpoints.minimum.width,
        );
        if spans_width(&freeze_row) <= usize::from(area.width) {
            frame.render_widget(Paragraph::new(Line::from(freeze_row)).style(base), area);
            return;
        }
    }
    if active_live_composer
        && !clear_prompt_confirmation_pending
        && !app.active_turn_in_progress()
        && background_task_count > 0
        && !context_summary_visible
    {
        let row = monitor_still_running_row(
            background_task_count,
            app.transcript_animation_phase(),
            theme,
            surface,
        );
        frame.render_widget(Paragraph::new(Line::from(row)).style(base), area);
        return;
    }

    if active_live_composer
        && !clear_prompt_confirmation_pending
        && (!app.interrupt_hint_visible() || app.active_turn_in_progress())
        && (!context_summary_visible || app.active_turn_in_progress())
    {
        let freeze_row = live_freeze_shortcut_disclosure_row(
            app,
            theme,
            surface,
            area.width <= theme.live_shell.breakpoints.minimum.width,
        );
        if spans_width(&freeze_row) <= usize::from(area.width) {
            let disclosure_area = Rect {
                x: area.x,
                y: area.y,
                width: area.width.saturating_sub(2).max(1),
                height: area.height,
            };
            frame.render_widget(
                Paragraph::new(Line::from(freeze_row)).style(base),
                disclosure_area,
            );
            return;
        }
    }

    let mut hint_candidates = if clear_prompt_confirmation_pending {
        let key = Style::default()
            .fg(theme.terminal_colors.primary)
            .bg(surface)
            .add_modifier(Modifier::BOLD);
        let text = Style::default()
            .fg(theme.terminal_colors.secondary)
            .bg(surface);
        vec![vec![
            Span::styled("Esc", key),
            Span::styled(":press again to clear", text),
        ]]
    } else if active_live_composer
        && (!app.interrupt_hint_visible() || app.active_turn_in_progress())
    {
        let freeze_row = live_freeze_shortcut_disclosure_row(
            app,
            theme,
            surface,
            area.width <= theme.live_shell.breakpoints.minimum.width,
        );
        if spans_width(&freeze_row) <= usize::from(area.width) {
            vec![freeze_row]
        } else if !app.composer.prompt_buffer.is_empty() {
            vec![live_freeze_primary_shortcut_disclosure_row(
                app, theme, surface,
            )]
        } else {
            composer_disclosure_hint_candidates(app, dock, theme, surface)
        }
    } else {
        composer_disclosure_hint_candidates(app, dock, theme, surface)
    };
    let summary_candidates = if active_live_composer {
        composer_context_summary_candidates(app, theme, surface)
    } else {
        composer_disclosure_summary_candidates(app, dock, theme, surface)
    };
    let max_width = usize::from(area.width);

    if active_live_composer && area.width < theme.live_shell.breakpoints.minimum.width {
        hint_candidates.retain(|candidate| {
            let text = candidate
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .to_ascii_lowercase();
            !text.contains("history") && !text.contains("quit")
        });
    }

    if active_live_composer {
        let best_fit = summary_candidates
            .iter()
            .enumerate()
            .flat_map(|(summary_idx, summary_spans)| {
                hint_candidates
                    .iter()
                    .enumerate()
                    .filter_map(move |(hint_idx, hint_spans)| {
                        let combined = inline_disclosure_cluster(
                            summary_spans.as_slice(),
                            hint_spans.as_slice(),
                            surface,
                        );
                        let width = spans_width(&combined);
                        (width <= max_width).then_some((summary_idx + (hint_idx * 2), combined))
                    })
            })
            .min_by_key(|(score, _)| *score)
            .map(|(_, spans)| spans);

        let render_spans = best_fit.or_else(|| {
            hint_candidates.iter().find_map(|hint_spans| {
                (spans_width(hint_spans) <= max_width).then(|| hint_spans.clone())
            })
        });

        if let Some(spans) = render_spans {
            frame.render_widget(Paragraph::new(Line::from(spans)).style(base), area);
            return;
        }
    }

    let best_fit = summary_candidates
        .iter()
        .enumerate()
        .filter_map(|(summary_idx, summary_spans)| {
            let summary_width = spans_width(summary_spans);
            (summary_width <= max_width).then_some((summary_idx, summary_spans, summary_width))
        })
        .flat_map(|(summary_idx, summary_spans, summary_width)| {
            hint_candidates
                .iter()
                .enumerate()
                .filter_map(move |(hint_idx, hint_spans)| {
                    let hint_width = spans_width(hint_spans);
                    if hint_width > max_width {
                        return None;
                    }

                    let summary_gap =
                        usize::from(!summary_spans.is_empty() && !hint_spans.is_empty()) * 2;
                    (summary_width
                        .saturating_add(hint_width)
                        .saturating_add(summary_gap)
                        <= max_width)
                        .then_some((
                            summary_idx + (hint_idx * 2),
                            summary_spans,
                            hint_spans,
                            hint_width,
                        ))
                })
        })
        .min_by_key(|(score, _, _, _)| *score);

    if let Some((_, summary_spans, hint_spans, hint_width)) = best_fit {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(u16::try_from(hint_width).unwrap_or(u16::MAX)),
            ])
            .split(area);
        if !summary_spans.is_empty() && columns[0].width > 0 {
            frame.render_widget(
                Paragraph::new(Line::from(summary_spans.to_vec())).style(base),
                columns[0],
            );
        }
        if !hint_spans.is_empty() && columns[1].width > 0 {
            frame.render_widget(
                Paragraph::new(Line::from(hint_spans.to_vec()))
                    .style(base)
                    .alignment(Alignment::Right),
                columns[1],
            );
        }
        return;
    }

    if let Some(summary_spans) = summary_candidates.first().cloned() {
        frame.render_widget(Paragraph::new(Line::from(summary_spans)).style(base), area);
    }
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

fn inline_disclosure_cluster(
    summary_spans: &[Span<'static>],
    hint_spans: &[Span<'static>],
    surface: Color,
) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(summary_spans.len() + hint_spans.len() + 1);
    spans.extend(summary_spans.iter().cloned());
    if !summary_spans.is_empty() && !hint_spans.is_empty() {
        spans.push(Span::styled("  ", Style::default().bg(surface)));
    }
    spans.extend(hint_spans.iter().cloned());
    spans
}

fn disclosure_segment(
    text: impl Into<String>,
    tone: DisclosureTone,
    theme: &Theme,
    surface: Color,
) -> Span<'static> {
    Span::styled(
        text.into(),
        Style::default()
            .fg(disclosure_color(tone, theme, composer_input_accent(theme)))
            .bg(surface),
    )
}

fn context_budget_segment(
    text: impl Into<String>,
    budget: &ContextBudget,
    theme: &Theme,
    surface: Color,
) -> Span<'static> {
    Span::styled(
        text.into(),
        Style::default().fg(budget.tone().color(theme)).bg(surface),
    )
}

fn disclosure_separator(theme: &Theme, surface: Color) -> Span<'static> {
    disclosure_segment("  ·  ", DisclosureTone::Tertiary, theme, surface)
}

fn disclosure_keycap(binding: &str, theme: &Theme, surface: Color) -> Span<'static> {
    Span::styled(
        binding.to_string(),
        Style::default()
            .fg(theme.text.primary)
            .bg(surface)
            .add_modifier(Modifier::BOLD),
    )
}

fn disclosure_shortcut(
    binding: &str,
    label: &str,
    theme: &Theme,
    surface: Color,
) -> Vec<Span<'static>> {
    vec![
        disclosure_keycap(binding, theme, surface),
        disclosure_segment(
            format!(" {label}"),
            DisclosureTone::Secondary,
            theme,
            surface,
        ),
    ]
}

fn shortcut_binding(app: &AppState, action: Action) -> Option<String> {
    app.keymap.get_binding_strs(action).into_iter().next()
}

fn compose_shortcut_row(
    shortcuts: &[(&str, &str)],
    theme: &Theme,
    surface: Color,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, (binding, label)) in shortcuts.iter().enumerate() {
        if index > 0 {
            spans.push(disclosure_separator(theme, surface));
        }
        spans.extend(disclosure_shortcut(binding, label, theme, surface));
    }
    spans
}

fn compose_interrupt_shortcut_row(theme: &Theme, surface: Color) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            "ctrl+c",
            Style::default()
                .fg(theme.text.primary)
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        ),
        disclosure_segment(" interrupt", DisclosureTone::Secondary, theme, surface),
    ]
}

fn monitor_still_running_row(
    count: usize,
    animation_phase: usize,
    theme: &Theme,
    surface: Color,
) -> Vec<Span<'static>> {
    let frame = super::ui_transcript_style::monitor_pulse_frame(animation_phase);
    let noun = if count == 1 { "task" } else { "tasks" };
    vec![
        Span::styled(
            format!("{frame} "),
            Style::default().fg(theme.text.accent).bg(surface),
        ),
        Span::styled(
            format!("{count} background {noun} still running"),
            Style::default().fg(theme.text.secondary).bg(surface),
        ),
    ]
}

fn starting_session_seed_row(
    animation_phase: usize,
    theme: &Theme,
    surface: Color,
) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            format!(
                "{} ",
                super::ui_transcript_style::transcript_streaming_spinner_frame(animation_phase)
            ),
            Style::default().fg(theme.text.accent).bg(surface),
        ),
        Span::styled(
            "Starting session…",
            Style::default().fg(theme.text.secondary).bg(surface),
        ),
    ]
}

fn live_freeze_shortcut_disclosure_row(
    app: &AppState,
    theme: &Theme,
    surface: Color,
    compact: bool,
) -> Vec<Span<'static>> {
    let bold = Style::default()
        .fg(theme.terminal_colors.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let normal = Style::default()
        .fg(theme.terminal_colors.secondary)
        .bg(surface);
    let dim = Style::default()
        .fg(theme.terminal_colors.secondary)
        .bg(surface)
        .add_modifier(Modifier::DIM);

    let mode_key = freeze_preferred_binding(app, Action::VariantCycle, "Shift+Tab");
    let help_key = freeze_preferred_binding(app, Action::Help, "Ctrl+x");
    let active_turn = app.active_turn_in_progress();
    let mut spans = vec![
        Span::styled(
            freeze_preferred_binding(app, Action::SubmitPrompt, "Enter"),
            bold,
        ),
        Span::styled(if active_turn { ":queue" } else { ":send" }, normal),
        Span::styled("  │  ", dim),
    ];
    if !app.composer.prompt_buffer.is_empty() && (active_turn || compact) {
        spans.extend([
            Span::styled(
                freeze_preferred_binding(app, Action::InsertNewline, "Alt+Enter"),
                bold,
            ),
            Span::styled(":newline", normal),
            Span::styled("  │  ", dim),
        ]);
    }
    if !compact {
        spans.extend([
            Span::styled(mode_key, bold),
            Span::styled(":mode", normal),
            Span::styled("  │  ", dim),
        ]);
    }
    if active_turn {
        spans.extend([
            Span::styled("Ctrl+c", bold),
            Span::styled(":cancel", normal),
            Span::styled("  │  ", dim),
        ]);
    }
    spans.extend([
        Span::styled(help_key, bold),
        Span::styled(":shortcuts", normal),
    ]);
    spans
}

fn live_freeze_primary_shortcut_disclosure_row(
    app: &AppState,
    theme: &Theme,
    surface: Color,
) -> Vec<Span<'static>> {
    let bold = Style::default()
        .fg(theme.terminal_colors.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let normal = Style::default()
        .fg(theme.terminal_colors.secondary)
        .bg(surface);
    let dim = normal.add_modifier(Modifier::DIM);
    let active_turn = app.active_turn_in_progress();
    vec![
        Span::styled(
            freeze_preferred_binding(app, Action::SubmitPrompt, "Enter"),
            bold,
        ),
        Span::styled(if active_turn { ":queue" } else { ":send" }, normal),
        Span::styled("  │  ", dim),
        Span::styled(
            freeze_preferred_binding(app, Action::InsertNewline, "Alt+Enter"),
            bold,
        ),
        Span::styled(":newline", normal),
        Span::styled("  │  ", dim),
        Span::styled(freeze_preferred_binding(app, Action::Help, "Ctrl+x"), bold),
        Span::styled(":shortcuts", normal),
    ]
}

fn freeze_preferred_binding(app: &AppState, action: Action, freeze_label: &str) -> String {
    let bindings = app.keymap.get_binding_strs(action);
    if bindings.iter().any(|binding| binding == freeze_label) {
        return freeze_label.to_string();
    }
    bindings
        .into_iter()
        .next()
        .filter(|binding| binding != "-")
        .unwrap_or_else(|| freeze_label.to_string())
}

fn composer_disclosure_hint_candidates(
    app: &AppState,
    dock: &crate::view_model::ControlDockViewModel,
    theme: &Theme,
    surface: Color,
) -> Vec<Vec<Span<'static>>> {
    if app.completed_session_shell_active() {
        let focus = shortcut_binding(app, Action::FocusNext).unwrap_or_else(|| "Tab".to_string());
        let commands =
            shortcut_binding(app, Action::Palette).unwrap_or_else(|| "Ctrl+p".to_string());
        let quit = shortcut_binding(app, Action::Quit).unwrap_or_else(|| "q".to_string());
        return vec![
            compose_shortcut_row(
                &[
                    (focus.as_str(), "focus"),
                    (commands.as_str(), "commands"),
                    (quit.as_str(), "quit"),
                ],
                theme,
                surface,
            ),
            compose_shortcut_row(
                &[(commands.as_str(), "commands"), (quit.as_str(), "quit")],
                theme,
                surface,
            ),
            compose_shortcut_row(&[(quit.as_str(), "quit")], theme, surface),
        ];
    }

    if dock.composer_disabled {
        let commands =
            shortcut_binding(app, Action::Palette).unwrap_or_else(|| "Ctrl+p".to_string());
        let quit = shortcut_binding(app, Action::Quit).unwrap_or_else(|| "q".to_string());
        return vec![
            compose_shortcut_row(
                &[(commands.as_str(), "commands"), (quit.as_str(), "quit")],
                theme,
                surface,
            ),
            compose_shortcut_row(&[(quit.as_str(), "quit")], theme, surface),
        ];
    }

    if app.interrupt_hint_visible() {
        return vec![compose_interrupt_shortcut_row(theme, surface)];
    }

    let commands = shortcut_binding(app, Action::Palette).unwrap_or_else(|| "Ctrl+p".to_string());
    vec![
        compose_shortcut_row(&[(commands.as_str(), "commands")], theme, surface),
        Vec::new(),
    ]
}

fn composer_disclosure_summary_candidates(
    app: &AppState,
    dock: &crate::view_model::ControlDockViewModel,
    theme: &Theme,
    surface: Color,
) -> Vec<Vec<Span<'static>>> {
    let runtime_state = app.runtime_state();
    let runtime_tone = runtime_kind_disclosure_tone(runtime_state.kind);
    let mut candidates = Vec::new();
    let short_runtime_summary = runtime_state
        .summary
        .split(" · ")
        .next()
        .unwrap_or(runtime_state.summary.as_str())
        .to_string();

    candidates.push(vec![disclosure_segment(
        runtime_state.summary.clone(),
        runtime_tone,
        theme,
        surface,
    )]);

    candidates.push(vec![disclosure_segment(
        short_runtime_summary,
        runtime_tone,
        theme,
        surface,
    )]);

    if let Some(segment) = dock.summary_segment.as_ref() {
        let segment_tone = control_dock_summary_tone_to_disclosure_tone(segment.tone);
        candidates.push(vec![disclosure_segment(
            format!("{}  ·  {}", runtime_state.summary, segment.text),
            segment_tone,
            theme,
            surface,
        )]);
        candidates.push(vec![disclosure_segment(
            segment.text.clone(),
            segment_tone,
            theme,
            surface,
        )]);
    }

    candidates.push(Vec::new());

    candidates
}

pub(super) fn startup_disclosure_candidates(
    theme: &Theme,
    surface: Color,
    palette: &str,
    newline: &str,
) -> Vec<Vec<Span<'static>>> {
    let key = Style::default()
        .fg(theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let text = Style::default().fg(theme.text.secondary).bg(surface);
    let variants = "ctrl+t";
    let agents = "tab";
    let commands = palette.to_ascii_lowercase();
    let newline = newline.to_ascii_lowercase();

    vec![
        vec![
            Span::styled(variants.to_string(), key),
            Span::styled(" variants  ", text),
            Span::styled(agents.to_string(), key),
            Span::styled(" agents  ", text),
            Span::styled(commands.clone(), key),
            Span::styled(" commands", text),
        ],
        vec![
            Span::styled(commands.clone(), key),
            Span::styled(" commands  ", text),
            Span::styled(newline.clone(), key),
            Span::styled(" newline", text),
        ],
        vec![Span::styled(commands, key), Span::styled(" commands", text)],
    ]
}

fn startup_disclosure_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

pub(super) fn replay_read_only_shortcut_hints(app: &AppState) -> String {
    [
        app.keymap
            .get_binding_label(Action::Help, "shortcuts")
            .to_ascii_lowercase(),
        app.keymap
            .get_binding_label(Action::FocusNext, "focus")
            .to_ascii_lowercase(),
        app.keymap
            .get_binding_label(Action::Quit, "quit")
            .to_ascii_lowercase(),
    ]
    .join("  ·  ")
}

pub(super) fn completed_session_status_summary(
    app: &AppState,
    state: &crate::app::RuntimeState,
) -> Option<String> {
    if !app.completed_session_shell_active() || app.replay_mode {
        return None;
    }

    Some(match state.kind {
        RuntimeStateKind::Failure => {
            "run failed · inspect transcript · session shell preserved".to_string()
        }
        _ => "run finished · session shell preserved".to_string(),
    })
}

#[cfg(test)]
mod monitor_pulse_tests {
    use super::{monitor_still_running_row, starting_session_seed_row};
    use crate::theme::Theme;
    use ratatui::style::Color;
}
