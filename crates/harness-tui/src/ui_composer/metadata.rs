use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComposerMetadataTone {
    Accent,
    Primary,
    Secondary,
}

pub(crate) fn composer_metadata_line(
    app: &AppState,
    dock: &crate::view_model::ControlDockViewModel,
    semantic_label: Option<&'static str>,
    _disclosure_visible: bool,
    max_width: usize,
    theme: &Theme,
    surface: Color,
) -> Line<'static> {
    if let Some(label) = semantic_label {
        return Line::from(Span::styled(
            label,
            Style::default().fg(composer_input_muted(theme)).bg(surface),
        ));
    }
    let candidates = composer_metadata_candidates(app, dock);
    let segments = candidates
        .iter()
        .find(|segments| composer_metadata_segments_width(segments) <= max_width)
        .cloned()
        .unwrap_or_else(|| {
            vec![(
                truncate_plain_text(&composer_metadata_text(app, dock, max_width), max_width),
                ComposerMetadataTone::Secondary,
            )]
        });

    Line::from(
        segments
            .into_iter()
            .map(|(text, tone)| {
                Span::styled(
                    text,
                    Style::default()
                        .fg(composer_metadata_color(tone, theme))
                        .bg(surface),
                )
            })
            .collect::<Vec<_>>(),
    )
}

fn composer_metadata_color(tone: ComposerMetadataTone, theme: &Theme) -> Color {
    match tone {
        ComposerMetadataTone::Accent => composer_input_accent(theme),
        ComposerMetadataTone::Primary => composer_input_text(theme),
        ComposerMetadataTone::Secondary => composer_input_muted(theme),
    }
}

fn composer_metadata_segments_width(segments: &[(String, ComposerMetadataTone)]) -> usize {
    segments
        .iter()
        .map(|(text, _)| text.chars().count())
        .sum::<usize>()
}

pub(crate) fn composer_metadata_candidates(
    app: &AppState,
    dock: &crate::view_model::ControlDockViewModel,
) -> Vec<Vec<(String, ComposerMetadataTone)>> {
    let model = app.current_model_base_label().to_string();
    let source = app.current_source_label();
    let tail = app
        .current_model_reasoning_label()
        .map(str::to_string)
        .or_else(|| {
            (has_trimmed_content(&dock.runtime_badge)
                && dock.runtime_kind != RuntimeStateKind::Ready
                && dock.runtime_kind != RuntimeStateKind::Success)
                .then(|| dock.runtime_badge.to_ascii_lowercase())
        });
    let queue_indicator =
        (app.queued_prompt_count > 0).then(|| format!("queued {}", app.queued_prompt_count));

    let mut full = Vec::new();
    if !model.is_empty() && model != "-" {
        if !full.is_empty() {
            full.push((" ".to_string(), ComposerMetadataTone::Secondary));
        }
        full.push((model.clone(), ComposerMetadataTone::Primary));
    }
    if let Some(source) = source.clone() {
        if !full.is_empty() {
            full.push((" ".to_string(), ComposerMetadataTone::Secondary));
        }
        full.push((source, ComposerMetadataTone::Secondary));
    }
    if let Some(tail) = tail.as_ref() {
        if !full.is_empty() {
            full.push((" · ".to_string(), ComposerMetadataTone::Secondary));
        }
        full.push((tail.clone(), ComposerMetadataTone::Accent));
    }
    if let Some(queue) = queue_indicator.as_ref() {
        if !full.is_empty() {
            full.push((" · ".to_string(), ComposerMetadataTone::Secondary));
        }
        full.push((queue.clone(), ComposerMetadataTone::Accent));
    }

    let mut compact = Vec::new();
    if !model.is_empty() && model != "-" {
        if !compact.is_empty() {
            compact.push((" ".to_string(), ComposerMetadataTone::Secondary));
        }
        compact.push((model, ComposerMetadataTone::Primary));
    }
    if let Some(queue) = queue_indicator.as_ref() {
        if !compact.is_empty() {
            compact.push((" · ".to_string(), ComposerMetadataTone::Secondary));
        }
        compact.push((queue.clone(), ComposerMetadataTone::Accent));
    }

    let queue_only = queue_indicator
        .as_ref()
        .map(|queue| vec![(queue.clone(), ComposerMetadataTone::Accent)]);

    let mut candidates = vec![full, compact];
    if let Some(queue_candidate) = queue_only {
        candidates.push(queue_candidate);
    }
    candidates.push(
        source
            .map(|source| vec![(source, ComposerMetadataTone::Secondary)])
            .unwrap_or_default(),
    );
    candidates.push(vec![(
        dock.primary_summary.clone(),
        ComposerMetadataTone::Secondary,
    )]);
    candidates
}

fn composer_metadata_text(
    app: &AppState,
    dock: &crate::view_model::ControlDockViewModel,
    max_width: usize,
) -> String {
    if max_width == 0 {
        return String::new();
    }

    best_fit_text(
        &[
            app.launch_mode_label().map(str::to_string),
            Some(dock.primary_summary.clone()),
            Some(app.current_model_label().to_string()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>(),
        max_width,
    )
}

fn best_fit_text(options: &[String], max_width: usize) -> String {
    options
        .iter()
        .find(|option| option.chars().count() <= max_width)
        .cloned()
        .unwrap_or_else(|| {
            truncate_plain_text(options.first().map(String::as_str).unwrap_or(""), max_width)
        })
}
