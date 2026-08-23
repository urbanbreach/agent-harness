use std::borrow::Cow;

use ratatui::layout::Rect;
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

use crate::app::permissions::PermissionModalStage;
use crate::app::{ActivePermissionView, AppState};

const ACCENT_WIDTH: u16 = 1;
const CONTENT_LEFT_PADDING: u16 = 2;
const CONTENT_RIGHT_PADDING: u16 = 2;
const TOP_PADDING_ROWS: u16 = 1;
const TITLE_ROWS: u16 = 1;
const OPTIONS_GAP_ROWS: u16 = 1;
const FOOTER_ROWS: u16 = 1;
const COLLAPSED_DETAIL_ROWS: u16 = 5;
pub(crate) const QUESTION_AUTO_SCROLL: u16 = u16::MAX;
pub(crate) const QUESTION_OUTER_FOOTER_ROWS: u16 = 3;
const QUESTION_TOP_PADDING_ROWS: u16 = 1;
const QUESTION_BOTTOM_PADDING_ROWS: u16 = 1;
const QUESTION_BODY_GAP_ROWS: u16 = 1;
const QUESTION_FOOTER_ROWS: u16 = 1;
const QUESTION_LABEL_GAP_ROWS: u16 = 2;
const QUESTION_MIN_VISIBLE_OPTION_ROWS: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PermissionDockMeasure {
    pub content_width: u16,
    pub detail_rows: u16,
    pub visible_detail_rows: u16,
    pub detail_truncated: bool,
    pub option_rows: u16,
    pub expanded: bool,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PermissionDockGeometry {
    pub rail: Rect,
    pub content: Rect,
    pub title: Rect,
    pub detail: Rect,
    pub options: Rect,
    pub footer: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuestionRowRange {
    pub index: usize,
    pub start: u16,
    pub end: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QuestionDockMeasure {
    pub content_width: u16,
    pub status_height: u16,
    pub dock_height: u16,
    pub source_chrome_rows: u16,
    pub chrome_rows: u16,
    pub option_rows: u16,
    pub body_viewport_rows: u16,
    pub sticky_rows: u16,
    pub scroll_offset: u16,
    pub max_scroll: u16,
    pub option_ranges: Vec<QuestionRowRange>,
    pub selected_range: Option<(u16, u16)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuestionDockGeometry {
    pub rail: Rect,
    pub content: Rect,
    pub chrome: Rect,
    pub options: Rect,
    pub sticky: Rect,
    pub footer: Rect,
    pub scrollbar: Option<Rect>,
}

pub(crate) fn permission_dock_measure(
    app: &AppState,
    width: u16,
    screen_height: u16,
    permission: &ActivePermissionView,
) -> PermissionDockMeasure {
    let content_width = width.saturating_sub(
        ACCENT_WIDTH
            .saturating_add(CONTENT_LEFT_PADDING)
            .saturating_add(CONTENT_RIGHT_PADDING),
    );
    let detail = permission_detail_text(permission);
    let detail_rows = wrapped_row_count(detail.as_ref(), content_width);
    let expanded = app.permission_detail_expanded(&permission.permission_id);
    let detail_truncated = !expanded && detail_rows > COLLAPSED_DETAIL_ROWS;
    let visible_detail_rows = if detail_truncated {
        COLLAPSED_DETAIL_ROWS
    } else {
        detail_rows
    };
    let option_rows = match app.permission_modal_stage(&permission.permission_id) {
        PermissionModalStage::Decision => 4,
        PermissionModalStage::AlwaysConfirm => 2,
    };
    let total = TOP_PADDING_ROWS
        .saturating_add(TITLE_ROWS)
        .saturating_add(visible_detail_rows)
        .saturating_add(OPTIONS_GAP_ROWS)
        .saturating_add(option_rows)
        .saturating_add(FOOTER_ROWS);
    let height = if expanded {
        total.min(screen_height)
    } else {
        total.min(collapsed_height_cap(screen_height))
    };

    PermissionDockMeasure {
        content_width,
        detail_rows,
        visible_detail_rows,
        detail_truncated,
        option_rows,
        expanded,
        height,
    }
}

pub(crate) fn permission_dock_geometry(
    area: Rect,
    measure: PermissionDockMeasure,
) -> PermissionDockGeometry {
    let rail = Rect::new(area.x, area.y, ACCENT_WIDTH.min(area.width), area.height);
    let content = Rect::new(
        area.x
            .saturating_add(ACCENT_WIDTH)
            .saturating_add(CONTENT_LEFT_PADDING),
        area.y,
        area.width.saturating_sub(
            ACCENT_WIDTH
                .saturating_add(CONTENT_LEFT_PADDING)
                .saturating_add(CONTENT_RIGHT_PADDING),
        ),
        area.height,
    );
    let footer = Rect::new(
        content.x,
        area.bottom().saturating_sub(FOOTER_ROWS),
        content.width,
        FOOTER_ROWS.min(area.height),
    );
    let options_height = measure
        .option_rows
        .min(area.height.saturating_sub(FOOTER_ROWS));
    let options = Rect::new(
        content.x,
        footer.y.saturating_sub(options_height),
        content.width,
        options_height,
    );
    let title_y = area.y.saturating_add(TOP_PADDING_ROWS.min(area.height));
    let title = Rect::new(
        content.x,
        title_y,
        content.width,
        TITLE_ROWS.min(options.y.saturating_sub(title_y)),
    );
    let detail_y = title.y.saturating_add(title.height);
    let detail_height = options
        .y
        .saturating_sub(OPTIONS_GAP_ROWS)
        .saturating_sub(detail_y);
    let detail = Rect::new(content.x, detail_y, content.width, detail_height);

    PermissionDockGeometry {
        rail,
        content,
        title,
        detail,
        options,
        footer,
    }
}

pub(crate) fn permission_detail_lines(
    permission: &ActivePermissionView,
    content_width: u16,
    max_rows: u16,
) -> Vec<String> {
    let detail = permission_detail_text(permission);
    wrap_text(detail.as_ref(), content_width, max_rows)
}

pub(crate) fn question_dock_measure(
    app: &AppState,
    width: u16,
    screen_height: u16,
    permission: &ActivePermissionView,
) -> QuestionDockMeasure {
    let content_width = width.saturating_sub(
        ACCENT_WIDTH
            .saturating_add(CONTENT_LEFT_PADDING)
            .saturating_add(CONTENT_RIGHT_PADDING),
    );
    let prompts = permission.question_prompts.as_deref().unwrap_or(&[]);
    let tab = app
        .question_prompt_tab(&permission.permission_id)
        .min(prompts.len().saturating_sub(1));
    let fullscreen = app.question_prompt_fullscreen(&permission.permission_id);
    let mut chrome_rows = 0u16;
    let mut option_rows = 0u16;
    let mut option_ranges = Vec::new();
    let mut selected_range = None;
    let mut sticky_rows = 0u16;

    if let Some(prompt) = prompts.get(tab) {
        let (question_label, question_description) =
            split_question_label_description(&prompt.question);
        chrome_rows =
            chrome_rows.saturating_add(wrapped_row_count(question_label, content_width).max(1));
        if !question_description.is_empty() {
            let rows = wrapped_row_count(question_description, content_width);
            chrome_rows = chrome_rows.saturating_add(1).saturating_add(if fullscreen {
                rows
            } else {
                rows.min(4)
            });
        }
        let selected = app.question_prompt_selection(&permission.permission_id);
        if let Some(preview) = prompt
            .options
            .get(selected)
            .and_then(|option| option.preview.as_deref())
            .filter(|preview| !preview.is_empty())
        {
            let rows = wrapped_row_count(preview, content_width);
            chrome_rows = chrome_rows.saturating_add(1).saturating_add(if fullscreen {
                rows
            } else {
                rows.min(3)
            });
        }
        chrome_rows = chrome_rows.saturating_add(QUESTION_LABEL_GAP_ROWS);
        let label_width = question_label_column_width(&prompt.options, usize::from(content_width));
        for (index, option) in prompt.options.iter().enumerate() {
            let start = option_rows;
            let normalized_label = normalize_question_label(&option.label);
            let rows = if index == selected {
                if display_width(&normalized_label) > label_width {
                    let stacked_width = content_width.saturating_sub(6).max(1);
                    wrapped_row_count(&normalized_label, stacked_width)
                        .saturating_add(wrapped_row_count(&option.description, stacked_width))
                        .max(1)
                } else if !option.description.is_empty() {
                    let description_width = content_width
                        .saturating_sub(u16::try_from(label_width).unwrap_or(u16::MAX))
                        .saturating_sub(8)
                        .max(1);
                    wrapped_row_count(&option.description, description_width).max(1)
                } else {
                    wrapped_row_count(
                        &question_option_visual(index, option, label_width, prompt.multiple),
                        content_width,
                    )
                    .max(1)
                }
            } else {
                1
            };
            option_rows = option_rows.saturating_add(rows);
            let range = QuestionRowRange {
                index,
                start,
                end: option_rows,
            };
            if index == selected {
                selected_range = Some((range.start, range.end));
            }
            option_ranges.push(range);
        }
        if prompt.custom {
            sticky_rows = 1;
        }
        if let Some(error) = app.question_answer_error(&permission.permission_id) {
            sticky_rows = sticky_rows
                .saturating_add(1)
                .saturating_add(wrapped_row_count(error, content_width).max(1));
        }
    }

    let source_chrome_rows = chrome_rows;
    let available_dock_height = screen_height.saturating_sub(QUESTION_OUTER_FOOTER_ROWS);
    let embedded_cap =
        question_height_cap(screen_height).saturating_add(QUESTION_OUTER_FOOTER_ROWS);
    let fixed_rows = QUESTION_TOP_PADDING_ROWS
        .saturating_add(sticky_rows)
        .saturating_add(QUESTION_BODY_GAP_ROWS)
        .saturating_add(QUESTION_FOOTER_ROWS)
        .saturating_add(QUESTION_BOTTOM_PADDING_ROWS)
        .saturating_add(option_rows.min(QUESTION_MIN_VISIBLE_OPTION_ROWS));
    if !fullscreen {
        chrome_rows = chrome_rows.min(embedded_cap.saturating_sub(fixed_rows));
    }
    let desired_dock_height = QUESTION_TOP_PADDING_ROWS
        .saturating_add(chrome_rows)
        .saturating_add(option_rows)
        .saturating_add(sticky_rows)
        .saturating_add(QUESTION_BODY_GAP_ROWS)
        .saturating_add(QUESTION_FOOTER_ROWS)
        .saturating_add(QUESTION_BOTTOM_PADDING_ROWS);
    let cap = if fullscreen || fixed_rows >= embedded_cap {
        available_dock_height
    } else {
        embedded_cap
    };
    let dock_height = desired_dock_height.min(cap).min(available_dock_height);
    let status_height = dock_height
        .saturating_add(QUESTION_OUTER_FOOTER_ROWS)
        .min(screen_height);
    let body_viewport_rows = dock_height.saturating_sub(
        QUESTION_TOP_PADDING_ROWS
            .saturating_add(chrome_rows)
            .saturating_add(sticky_rows)
            .saturating_add(QUESTION_BODY_GAP_ROWS)
            .saturating_add(QUESTION_FOOTER_ROWS)
            .saturating_add(QUESTION_BOTTOM_PADDING_ROWS),
    );
    let max_scroll = option_rows.saturating_sub(body_viewport_rows);
    let stored_scroll = app.question_prompt_scroll(&permission.permission_id, tab);
    let scroll_offset = if stored_scroll == QUESTION_AUTO_SCROLL {
        selected_range
            .map(|(_, bottom)| bottom.saturating_sub(body_viewport_rows).min(max_scroll))
            .unwrap_or(0)
    } else {
        stored_scroll.min(max_scroll)
    };

    QuestionDockMeasure {
        content_width,
        status_height,
        dock_height,
        source_chrome_rows,
        chrome_rows,
        option_rows,
        body_viewport_rows,
        sticky_rows,
        scroll_offset,
        max_scroll,
        option_ranges,
        selected_range,
    }
}

pub(crate) fn question_dock_geometry(
    area: Rect,
    measure: &QuestionDockMeasure,
) -> QuestionDockGeometry {
    let rail = Rect::new(area.x, area.y, ACCENT_WIDTH.min(area.width), area.height);
    let content = Rect::new(
        area.x
            .saturating_add(ACCENT_WIDTH)
            .saturating_add(CONTENT_LEFT_PADDING),
        area.y
            .saturating_add(QUESTION_TOP_PADDING_ROWS.min(area.height)),
        area.width.saturating_sub(
            ACCENT_WIDTH
                .saturating_add(CONTENT_LEFT_PADDING)
                .saturating_add(CONTENT_RIGHT_PADDING),
        ),
        area.height
            .saturating_sub(QUESTION_TOP_PADDING_ROWS.saturating_add(QUESTION_BOTTOM_PADDING_ROWS)),
    );
    let footer = Rect::new(
        content.x,
        content.bottom().saturating_sub(QUESTION_FOOTER_ROWS),
        content.width,
        QUESTION_FOOTER_ROWS.min(content.height),
    );
    let sticky = Rect::new(
        content.x,
        footer
            .y
            .saturating_sub(QUESTION_BODY_GAP_ROWS)
            .saturating_sub(measure.sticky_rows),
        content.width,
        measure
            .sticky_rows
            .min(content.height.saturating_sub(QUESTION_FOOTER_ROWS)),
    );
    let chrome = Rect::new(
        content.x,
        content.y,
        content.width,
        measure.chrome_rows.min(sticky.y.saturating_sub(content.y)),
    );
    let options = Rect::new(
        content.x,
        chrome.bottom(),
        content.width,
        sticky.y.saturating_sub(chrome.bottom()),
    );
    let scrollbar = (measure.max_scroll > 0 && options.height > 0 && area.width > 0).then_some(
        Rect::new(area.right().saturating_sub(1), options.y, 1, options.height),
    );
    QuestionDockGeometry {
        rail,
        content,
        chrome,
        options,
        sticky,
        footer,
        scrollbar,
    }
}

pub(crate) fn question_option_visual(
    index: usize,
    option: &crate::app::QuestionOptionView,
    label_width: usize,
    multiple: bool,
) -> String {
    let shortcut = crate::app::permissions::question_option_shortcut_label(index).unwrap_or(' ');
    let marker = if multiple { "[ ]" } else { "(○)" };
    let prefix = format!("{shortcut} {marker} ");
    if option.description.is_empty() {
        return format!("{prefix}{}", normalize_question_label(&option.label));
    }
    let label = normalize_question_label(&option.label);
    let padding = label_width.saturating_sub(display_width(&label));
    format!(
        "{prefix}{}{}  {}",
        label,
        " ".repeat(padding),
        option.description
    )
}

pub(crate) fn question_label_column_width(
    options: &[crate::app::QuestionOptionView],
    content_width: usize,
) -> usize {
    options
        .iter()
        .map(|option| display_width(&normalize_question_label(&option.label)))
        .max()
        .unwrap_or(0)
        .min(content_width.saturating_mul(3) / 5)
}

fn normalize_question_label(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn split_question_label_description(question: &str) -> (&str, &str) {
    question
        .split_once("\n\n")
        .map_or((question.trim(), ""), |(label, description)| {
            (label.trim(), description.trim())
        })
}

fn permission_detail_text(permission: &ActivePermissionView) -> Cow<'_, str> {
    let summary = permission.summary.trim();
    if summary.is_empty() {
        return Cow::Borrowed("");
    }
    serde_json::from_str::<serde_json::Value>(summary)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .map_or(Cow::Borrowed(summary), Cow::Owned)
}

fn collapsed_height_cap(screen_height: u16) -> u16 {
    let half = u32::from(screen_height) / 2;
    let eighty_percent = u32::from(screen_height).saturating_mul(80) / 100;
    u16::try_from(half.max(10).min(eighty_percent)).unwrap_or(u16::MAX)
}

fn question_height_cap(screen_height: u16) -> u16 {
    let third = u32::from(screen_height).saturating_mul(33) / 100;
    let eighty_percent = u32::from(screen_height).saturating_mul(80) / 100;
    u16::try_from(third.max(8).min(eighty_percent)).unwrap_or(u16::MAX)
}

fn display_width(text: &str) -> usize {
    text.graphemes(true)
        .map(|grapheme| grapheme.width().max(1))
        .sum()
}

fn wrapped_row_count(text: &str, width: u16) -> u16 {
    if text.is_empty() {
        return 0;
    }
    u16::try_from(wrap_text(text, width, u16::MAX).len()).unwrap_or(u16::MAX)
}

fn wrap_text(text: &str, width: u16, max_rows: u16) -> Vec<String> {
    if text.is_empty() || max_rows == 0 {
        return Vec::new();
    }
    let width = width.max(1);
    let mut rows = Vec::new();
    for source_line in text.split('\n') {
        let clusters = source_line
            .graphemes(true)
            .map(|grapheme| {
                (
                    grapheme,
                    u16::try_from(grapheme.width().max(1)).unwrap_or(u16::MAX),
                )
            })
            .collect::<Vec<_>>();
        if clusters.is_empty() {
            rows.push(String::new());
            continue;
        }
        let mut start = 0usize;
        while start < clusters.len() {
            let mut used = 0u16;
            let mut fit_end = start;
            while fit_end < clusters.len() && used.saturating_add(clusters[fit_end].1) <= width {
                used = used.saturating_add(clusters[fit_end].1);
                fit_end += 1;
            }
            if fit_end == clusters.len() {
                rows.push(question_clusters_to_string(&clusters[start..]));
                break;
            }
            let end = clusters[start..fit_end]
                .iter()
                .rposition(|(cluster, _)| cluster.chars().all(char::is_whitespace))
                .map(|offset| start + offset)
                .filter(|end| *end > start)
                .unwrap_or(fit_end.max(start.saturating_add(1)));
            rows.push(question_clusters_to_string(&clusters[start..end]));
            start = if clusters
                .get(end)
                .is_some_and(|(cluster, _)| cluster.chars().all(char::is_whitespace))
            {
                end.saturating_add(1)
            } else {
                end
            };
            if rows.len() >= usize::from(max_rows) {
                return rows;
            }
        }
        if rows.len() >= usize::from(max_rows) {
            return rows;
        }
    }
    rows
}

fn question_clusters_to_string(clusters: &[(&str, u16)]) -> String {
    clusters
        .iter()
        .fold(String::new(), |mut text, (cluster, _)| {
            text.push_str(cluster);
            text
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_label_column_is_capped_at_three_fifths_of_content_width() {
        // arrange
        let options = vec![crate::app::QuestionOptionView {
            label: "A label that would otherwise consume the description column".to_string(),
            description: "Description".to_string(),
            preview: None,
        }];

        // act
        let width = question_label_column_width(&options, 20);

        // assert
        assert_eq!(width, 12);
    }
}
