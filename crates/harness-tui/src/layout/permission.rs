use std::borrow::Cow;

use ratatui::layout::Rect;

use crate::app::permissions::PermissionModalStage;
use crate::app::{ActivePermissionView, AppState};
use crate::terminal::char_display_width;

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
const QUESTION_TABS_ROWS: u16 = 2;
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
    let single = prompts.len() == 1 && prompts.first().is_some_and(|prompt| !prompt.multiple);
    let tab = app
        .question_prompt_tab(&permission.permission_id)
        .min(prompts.len());
    let confirm = !single && tab >= prompts.len();
    let answers = app.question_prompt_answers(&permission.permission_id);
    let mut chrome_rows = if single { 0 } else { QUESTION_TABS_ROWS };
    let mut option_rows = 0u16;
    let mut option_ranges = Vec::new();
    let mut selected_range = None;
    let mut sticky_rows = 0u16;

    if confirm {
        chrome_rows = chrome_rows.saturating_add(1);
        for (index, prompt) in prompts.iter().enumerate() {
            let value = answers
                .get(index)
                .map(|values| values.join(", "))
                .unwrap_or_default();
            option_rows = option_rows.saturating_add(wrapped_row_count(
                &format!("{}: {}", prompt.header, value),
                content_width,
            ));
        }
        if let Some(error) = app.question_answer_error(&permission.permission_id) {
            sticky_rows = 1u16.saturating_add(wrapped_row_count(error, content_width).max(1));
        }
    } else if let Some(prompt) = prompts.get(tab) {
        let question = if prompt.multiple {
            format!("{} (select all that apply)", prompt.question)
        } else {
            prompt.question.clone()
        };
        chrome_rows = chrome_rows
            .saturating_add(wrapped_row_count(&question, content_width).max(1))
            .saturating_add(QUESTION_LABEL_GAP_ROWS);
        let selected = app.question_prompt_selection(&permission.permission_id);
        let label_width = prompt
            .options
            .iter()
            .map(|option| display_width(&option.label))
            .max()
            .unwrap_or(0);
        for (index, option) in prompt.options.iter().enumerate() {
            let start = option_rows;
            let rows = wrapped_row_count(
                &question_option_visual(index, option, label_width, prompt.multiple),
                content_width,
            )
            .max(1);
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
            if selected == prompt.options.len() {
                let custom = app
                    .question_prompt_custom(&permission.permission_id, tab)
                    .unwrap_or_default();
                if app.question_prompt_editing(&permission.permission_id) || !custom.is_empty() {
                    sticky_rows = sticky_rows.saturating_add(1);
                }
            }
        }
        if let Some(error) = app.question_answer_error(&permission.permission_id) {
            sticky_rows = sticky_rows
                .saturating_add(1)
                .saturating_add(wrapped_row_count(error, content_width).max(1));
        }
    }

    let desired_dock_height = QUESTION_TOP_PADDING_ROWS
        .saturating_add(chrome_rows)
        .saturating_add(option_rows)
        .saturating_add(sticky_rows)
        .saturating_add(QUESTION_BODY_GAP_ROWS)
        .saturating_add(QUESTION_FOOTER_ROWS)
        .saturating_add(QUESTION_BOTTOM_PADDING_ROWS);
    let fullscreen = app.question_prompt_fullscreen(&permission.permission_id);
    let available_dock_height = screen_height.saturating_sub(QUESTION_OUTER_FOOTER_ROWS);
    let embedded_cap =
        question_height_cap(screen_height).saturating_add(QUESTION_OUTER_FOOTER_ROWS);
    let minimum_safe_height = QUESTION_TOP_PADDING_ROWS
        .saturating_add(chrome_rows)
        .saturating_add(sticky_rows)
        .saturating_add(QUESTION_BODY_GAP_ROWS)
        .saturating_add(QUESTION_FOOTER_ROWS)
        .saturating_add(QUESTION_BOTTOM_PADDING_ROWS)
        .saturating_add(option_rows.min(QUESTION_MIN_VISIBLE_OPTION_ROWS));
    let cap = if fullscreen || minimum_safe_height > embedded_cap {
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
    let marker = if multiple { "[ ]" } else { "○" };
    let prefix = format!("{} ({marker}) ", index.saturating_add(1));
    if option.description.is_empty() {
        return format!("{prefix}{}", option.label);
    }
    let padding = label_width.saturating_sub(display_width(&option.label));
    format!(
        "{prefix}{}{}  {}",
        option.label,
        " ".repeat(padding),
        option.description
    )
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
    text.chars().map(char_display_width).map(usize::from).sum()
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
        let chars = source_line
            .chars()
            .map(|character| (character, char_display_width(character)))
            .collect::<Vec<_>>();
        if chars.is_empty() {
            rows.push(String::new());
            continue;
        }
        let mut start = 0usize;
        while start < chars.len() {
            let mut used = 0u16;
            let mut fit_end = start;
            while fit_end < chars.len() && used.saturating_add(chars[fit_end].1) <= width {
                used = used.saturating_add(chars[fit_end].1);
                fit_end += 1;
            }
            if fit_end == chars.len() {
                rows.push(
                    chars[start..]
                        .iter()
                        .map(|(character, _)| *character)
                        .collect(),
                );
                break;
            }
            let end = chars[start..fit_end]
                .iter()
                .rposition(|(character, _)| character.is_whitespace())
                .map(|offset| start + offset)
                .filter(|end| *end > start)
                .unwrap_or(fit_end.max(start.saturating_add(1)));
            rows.push(
                chars[start..end]
                    .iter()
                    .map(|(character, _)| *character)
                    .collect(),
            );
            start = if chars
                .get(end)
                .is_some_and(|(character, _)| character.is_whitespace())
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
