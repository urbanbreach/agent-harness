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
const QUESTION_MIN_VISIBLE_OPTION_ROWS: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PermissionDockMeasure {
    pub content_width: u16,
    pub detail_rows: u16,
    pub visible_detail_rows: u16,
    pub detail_truncated: bool,
    pub option_rows: u16,
    pub editor_rows: u16,
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
    pub description_cap: u16,
    pub preview_cap: u16,
    pub editor_lines: Vec<String>,
    pub editor_cursor: Option<(u16, u16)>,
    pub option_rows: u16,
    pub scrollbar_rows: u16,
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
    pub scrollbar: Option<(Rect, Rect)>,
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
    let detail_rows =
        u16::try_from(permission_detail_lines(permission, content_width, u16::MAX).len())
            .unwrap_or(u16::MAX);
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
    let editor_rows = app
        .permission_feedback(&permission.permission_id)
        .filter(|feedback| feedback.editing)
        .map_or(1, |feedback| {
            u16::try_from(
                feedback
                    .editor_viewport(
                        content_width.saturating_sub(6).max(1),
                        (screen_height / 3).clamp(3, 15),
                    )
                    .0
                    .len(),
            )
            .unwrap_or(u16::MAX)
        });
    let total = TOP_PADDING_ROWS
        .saturating_add(TITLE_ROWS)
        .saturating_add(visible_detail_rows)
        .saturating_add(OPTIONS_GAP_ROWS)
        .saturating_add(option_rows)
        .saturating_add(editor_rows.saturating_sub(1))
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
        editor_rows,
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
        .saturating_add(measure.editor_rows.saturating_sub(1))
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
    if detail.is_empty() || max_rows == 0 {
        return Vec::new();
    }
    let width = usize::from(content_width.max(1));
    let mut rows = Vec::new();
    for source in detail.split('\n') {
        let mut row = String::new();
        let mut used = 0usize;
        for grapheme in source.graphemes(true) {
            let cells = grapheme.width();
            if used.saturating_add(cells) > width && !row.is_empty() {
                rows.push(std::mem::take(&mut row));
                if rows.len() == usize::from(max_rows) {
                    return rows;
                }
                used = 0;
            }
            row.push_str(grapheme);
            used = used.saturating_add(cells);
        }
        rows.push(row);
        if rows.len() == usize::from(max_rows) {
            break;
        }
    }
    rows
}

pub(crate) fn question_dock_measure(
    app: &AppState,
    width: u16,
    screen: Rect,
    permission: &ActivePermissionView,
) -> QuestionDockMeasure {
    let padding = ACCENT_WIDTH + CONTENT_LEFT_PADDING + CONTENT_RIGHT_PADDING;
    let content_width = width.saturating_sub(padding);
    let mut measure = question_content_measure_with_editor_width(
        app,
        content_width,
        content_width
            .saturating_add(CONTENT_RIGHT_PADDING)
            .saturating_sub(8),
        screen.height,
        permission,
    );
    // The native scrollbar counts choices plus the freeform row at frame width,
    // independently of the inset option viewport used for painting and scrolling.
    measure.scrollbar_rows = question_content_measure(
        app,
        screen.width.saturating_sub(padding),
        screen.height,
        permission,
    )
    .scrollbar_rows;
    measure
}

/// Measure already-inset Question content without applying dock padding again.
pub(crate) fn question_content_measure(
    app: &AppState,
    content_width: u16,
    screen_height: u16,
    permission: &ActivePermissionView,
) -> QuestionDockMeasure {
    question_content_measure_with_editor_width(
        app,
        content_width,
        content_width.saturating_sub(8),
        screen_height,
        permission,
    )
}

fn question_content_measure_with_editor_width(
    app: &AppState,
    content_width: u16,
    editor_width: u16,
    screen_height: u16,
    permission: &ActivePermissionView,
) -> QuestionDockMeasure {
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
    let mut description_cap = if fullscreen { u16::MAX } else { 5 };
    let mut preview_cap = if fullscreen { u16::MAX } else { 6 };
    let mut editor_lines = Vec::new();
    let mut editor_cursor = None;
    let mut custom_rows = 0;
    let cap = if fullscreen {
        screen_height
    } else {
        question_height_cap(screen_height)
    };

    if let Some(prompt) = prompts.get(tab) {
        let (question_label, question_description) =
            split_question_label_description(&prompt.question);
        let label_rows = wrapped_row_count(question_label, content_width).max(1);
        let description_rows = wrapped_row_count(question_description, content_width);
        let selected = app.question_prompt_selection(&permission.permission_id);
        let preview = prompt
            .options
            .get(selected)
            .and_then(|option| option.preview.as_deref());
        let preview_rows = preview.map_or(0, |text| wrapped_row_count(text, content_width));
        let label_gap = u16::from(!question_description.is_empty() || !prompt.options.is_empty());
        let fixed_chrome = label_rows.saturating_add(label_gap).saturating_add(1);
        let min_options = QUESTION_MIN_VISIBLE_OPTION_ROWS + u16::from(prompt.custom);
        let fixed_rows = QUESTION_TOP_PADDING_ROWS
            .saturating_add(fixed_chrome)
            .saturating_add(min_options);
        if !fullscreen
            && fixed_rows
                .saturating_add(description_rows.min(description_cap))
                .saturating_add(u16::from(preview_rows > 0))
                .saturating_add(preview_rows.min(preview_cap))
                > cap
        {
            let budget = cap.saturating_sub(fixed_rows);
            description_cap = budget.min(5).min(description_rows);
            let remaining = budget.saturating_sub(description_cap);
            preview_cap = remaining
                .saturating_sub(u16::from(preview.is_some() && remaining > 0))
                .min(6);
        }
        let visible_preview = preview_rows.min(preview_cap);
        chrome_rows = fixed_chrome
            .saturating_add(description_rows.min(description_cap))
            .saturating_add(u16::from(visible_preview > 0))
            .saturating_add(visible_preview);
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
            custom_rows = 1;
            sticky_rows = 1;
            if app.question_prompt_editing(&permission.permission_id) {
                let (lines, cursor) = question_editor_viewport(
                    &app.question_prompt.answer_buffer,
                    app.question_prompt.answer_cursor,
                    editor_width.max(1),
                    (screen_height / 3).clamp(3, 15),
                );
                sticky_rows = u16::try_from(lines.len()).unwrap_or(u16::MAX);
                editor_lines = lines;
                editor_cursor = Some(cursor);
            }
        }
        if let Some(error) = app.question_answer_error(&permission.permission_id) {
            sticky_rows = sticky_rows
                .saturating_add(1)
                .saturating_add(wrapped_row_count(error, content_width).max(1));
        }
    }

    let source_chrome_rows = chrome_rows;
    // Cap chrome + choices + one freeform row, not the growing editor or footer.
    let view_height = QUESTION_TOP_PADDING_ROWS
        .saturating_add(chrome_rows)
        .saturating_add(option_rows)
        .saturating_add(custom_rows)
        .min(cap);
    let available_dock_height = screen_height.saturating_sub(QUESTION_OUTER_FOOTER_ROWS);
    let desired_dock_height = view_height
        .saturating_add(sticky_rows.saturating_sub(custom_rows))
        .saturating_add(
            QUESTION_BODY_GAP_ROWS + QUESTION_FOOTER_ROWS + QUESTION_BOTTOM_PADDING_ROWS,
        );
    let dock_height = desired_dock_height.min(available_dock_height);
    chrome_rows = chrome_rows.min(
        dock_height.saturating_sub(
            QUESTION_TOP_PADDING_ROWS
                .saturating_add(sticky_rows)
                .saturating_add(QUESTION_BODY_GAP_ROWS)
                .saturating_add(QUESTION_FOOTER_ROWS)
                .saturating_add(QUESTION_BOTTOM_PADDING_ROWS),
        ),
    );
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
    let max_scroll = option_rows
        .saturating_add(u16::from(!editor_lines.is_empty()))
        .saturating_sub(body_viewport_rows);
    let stored_scroll = app.question_prompt_scroll(&permission.permission_id, tab);
    let scroll_offset = if stored_scroll == QUESTION_AUTO_SCROLL {
        let cursor_bottom = selected_range
            .map(|(_, bottom)| bottom)
            .unwrap_or(option_rows.saturating_add(custom_rows));
        cursor_bottom
            .saturating_sub(body_viewport_rows)
            .min(max_scroll)
    } else {
        stored_scroll.min(max_scroll)
    };

    QuestionDockMeasure {
        content_width,
        status_height,
        dock_height,
        source_chrome_rows,
        chrome_rows,
        description_cap,
        preview_cap,
        editor_lines,
        editor_cursor,
        option_rows,
        scrollbar_rows: option_rows.saturating_add(custom_rows),
        body_viewport_rows,
        sticky_rows,
        scroll_offset,
        max_scroll,
        option_ranges,
        selected_range,
    }
}

// Cursor position is metadata, never a glyph inserted into the answer text.
pub(crate) fn question_editor_viewport(
    text: &str,
    cursor: usize,
    width: u16,
    height: u16,
) -> (Vec<String>, (u16, u16)) {
    let mut rows = Vec::new();
    let mut cursor_position = (0, 0);
    let mut char_start = 0;
    for logical in text.split('\n') {
        let wrapped = if logical.is_empty() {
            vec![String::new()]
        } else {
            wrap_text(logical, width, u16::MAX)
        };
        let mut remaining = logical;
        for line in wrapped {
            let consumed = line.chars().count();
            if cursor >= char_start && cursor <= char_start + consumed {
                let column = line
                    .chars()
                    .take(cursor - char_start)
                    .collect::<String>()
                    .width();
                cursor_position = (rows.len(), column);
            }
            remaining = &remaining[line.len()..];
            char_start += consumed;
            if let Some(separator) = remaining.chars().next().filter(|ch| ch.is_whitespace()) {
                remaining = &remaining[separator.len_utf8()..];
                char_start += 1;
            }
            rows.push(line);
        }
        char_start += 1;
    }
    let visible = usize::from(height).min(rows.len());
    let start = (cursor_position.0 + 1)
        .saturating_sub(visible)
        .min(rows.len().saturating_sub(visible));
    let cursor = (
        u16::try_from(cursor_position.0.saturating_sub(start)).unwrap_or(u16::MAX),
        u16::try_from(cursor_position.1)
            .unwrap_or(u16::MAX)
            .min(width.saturating_sub(1)),
    );
    (rows.into_iter().skip(start).take(visible).collect(), cursor)
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
    let sticky_bottom = footer
        .y
        .saturating_sub(QUESTION_BODY_GAP_ROWS)
        .max(content.y);
    let sticky_height = measure
        .sticky_rows
        .min(sticky_bottom.saturating_sub(content.y));
    let sticky = Rect::new(
        content.x,
        sticky_bottom.saturating_sub(sticky_height),
        if measure.editor_lines.is_empty() {
            content.width
        } else {
            area.right().saturating_sub(content.x)
        },
        sticky_height,
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
    let scrollbar =
        (measure.scrollbar_rows > options.height && options.height > 0 && area.width > 0)
            .then_some(Rect::new(
                area.right().saturating_sub(1),
                options.y,
                1,
                options.height,
            ));
    QuestionDockGeometry {
        rail,
        content,
        chrome,
        options,
        sticky,
        footer,
        scrollbar: scrollbar.map(|track| {
            // Native tui-scrollbar uses eighth-cell metrics, then paints every
            // partially covered cell as a full, background-filled block.
            let track_units = u32::from(track.height) * 8;
            let thumb_units = (track_units * u32::from(track.height)
                / u32::from(measure.scrollbar_rows.max(1)))
            .max(8)
            .min(track_units);
            let max_offset = measure.scrollbar_rows.saturating_sub(track.height).max(1);
            let start = (track_units - thumb_units)
                * u32::from(measure.scroll_offset.min(max_offset))
                / u32::from(max_offset);
            // Both cell bounds are at most track.height, which is already a u16.
            let top = u16::try_from(start / 8).unwrap_or(track.height);
            let bottom = u16::try_from((start + thumb_units).div_ceil(8)).unwrap_or(track.height);
            let thumb = Rect::new(track.x, track.y + top, track.width, bottom - top);
            (track, thumb)
        }),
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
    // Edit review uses the file in the title and any human description. The
    // coordinator's serialized request is metadata, not a description to paint.
    let is_edit = ["edit_fs", "edit", "write", "fs.write"]
        .iter()
        .any(|kind| permission.kind.eq_ignore_ascii_case(kind));
    let parsed = serde_json::from_str::<serde_json::Value>(summary).ok();
    if is_edit && (summary.starts_with("tool=") || parsed.is_some()) {
        return Cow::Borrowed("");
    }
    parsed
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
    fn question_editor_wraps_unicode_whitespace_and_tracks_the_visible_cursor() {
        for separator in [' ', '\t', '\u{2003}'] {
            let text = format!("甲乙{separator}丙");
            let (lines, cursor) = question_editor_viewport(&text, text.chars().count(), 4, 3);
            assert_eq!(lines, ["甲乙", "丙"]);
            assert_eq!(cursor, (1, 2));
        }
        let text = "first\nsecond\nthird\nfourth";
        let (lines, cursor) = question_editor_viewport(text, text.chars().count(), 10, 2);
        assert_eq!(lines, ["third", "fourth"]);
        assert_eq!(cursor, (1, 6));
        let (lines, cursor) = question_editor_viewport(text, 2, 10, 2);
        assert_eq!(lines, ["first", "second"]);
        assert_eq!(cursor, (0, 2));
    }

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
