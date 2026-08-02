use super::*;
use crate::UnwrapOrAbort;

pub(crate) fn render_live_buffer(app: &app::AppState, width: u16, height: u16) -> String {
    use ratatui::{backend::TestBackend, Terminal};

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, app))
        .unwrap_or_abort();
    format!("{:?}", terminal.backend().buffer())
}

pub(crate) fn render_live_cells(
    app: &app::AppState,
    width: u16,
    height: u16,
) -> ratatui::buffer::Buffer {
    use ratatui::{backend::TestBackend, Terminal};

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, app))
        .unwrap_or_abort();
    terminal.backend().buffer().clone()
}

pub(crate) fn row_text_and_palette(
    buffer: &ratatui::buffer::Buffer,
    width: u16,
    needle: &str,
) -> Option<(
    String,
    Vec<ratatui::style::Color>,
    Vec<ratatui::style::Color>,
)> {
    buffer.content.chunks(width as usize).find_map(|row| {
        let text = row.iter().map(|cell| cell.symbol()).collect::<String>();
        text.contains(needle).then(|| {
            (
                text,
                row.iter().map(|cell| cell.fg).collect::<Vec<_>>(),
                row.iter().map(|cell| cell.bg).collect::<Vec<_>>(),
            )
        })
    })
}

pub(crate) fn row_at(
    buffer: &ratatui::buffer::Buffer,
    width: u16,
    row_index: usize,
) -> Option<(
    String,
    Vec<ratatui::style::Color>,
    Vec<ratatui::style::Color>,
)> {
    buffer
        .content
        .chunks(width as usize)
        .nth(row_index)
        .map(|row| {
            (
                row.iter().map(|cell| cell.symbol()).collect::<String>(),
                row.iter().map(|cell| cell.fg).collect::<Vec<_>>(),
                row.iter().map(|cell| cell.bg).collect::<Vec<_>>(),
            )
        })
}

pub(crate) fn assert_selected_overlay_row_uses_highlight(
    app: &app::AppState,
    width: u16,
    height: u16,
    needle: &str,
    expected_bg: ratatui::style::Color,
) {
    let buffer = render_live_cells(app, width, height);
    let (row, fgs, bgs) = row_text_and_palette(&buffer, width, needle)
        .unwrap_or_else(|| panic!("missing selected overlay row {needle:?}"));
    let start_byte = row.find(needle).unwrap_or_abort();
    let start = row[..start_byte].chars().count();
    let end = start + needle.chars().count();

    assert!(
        fgs[start..end]
            .iter()
            .all(|color| *color == ratatui::style::Color::Rgb(0x0B, 0x0E, 0x14)),
        "selected overlay row should use inverse foreground for {needle:?}\n{row}"
    );
    assert!(
        bgs[start..end].iter().all(|color| *color == expected_bg),
        "selected overlay row should use the expected background for {needle:?}\n{row}"
    );
}

pub(crate) fn assert_row_segment_palette(
    buffer: &ratatui::buffer::Buffer,
    width: u16,
    needle: &str,
    expected_fg: ratatui::style::Color,
    expected_bg: ratatui::style::Color,
) {
    let (row, fgs, bgs) = row_text_and_palette(buffer, width, needle)
        .unwrap_or_else(|| panic!("missing row for {needle:?}"));
    let start_byte = row.find(needle).unwrap_or_abort();
    let start = row[..start_byte].chars().count();
    let end = start + needle.chars().count();

    assert!(
        fgs[start..end].iter().all(|color| *color == expected_fg),
        "row should use the expected helper foreground for {needle:?}\n{row}"
    );
    assert!(
        bgs[start..end].iter().all(|color| *color == expected_bg),
        "row should use the expected helper background for {needle:?}\n{row}"
    );
}

pub(crate) fn assert_row_segment_background(
    buffer: &ratatui::buffer::Buffer,
    width: u16,
    needle: &str,
    expected_bg: ratatui::style::Color,
) {
    let (row, _, bgs) = row_text_and_palette(buffer, width, needle)
        .unwrap_or_else(|| panic!("missing row for {needle:?}"));
    let start_byte = row.find(needle).unwrap_or_abort();
    let start = row[..start_byte].chars().count();
    let end = start + needle.chars().count();

    assert!(
        bgs[start..end].iter().all(|color| *color == expected_bg),
        "row should use the expected helper background for {needle:?}\n{row}"
    );
}

pub(crate) fn assert_alphanumeric_row_palette(
    buffer: &ratatui::buffer::Buffer,
    width: u16,
    row_index: usize,
    expected_fg: ratatui::style::Color,
    expected_bg: ratatui::style::Color,
    label: &str,
) {
    let (row, fgs, bgs) = row_at(buffer, width, row_index)
        .unwrap_or_else(|| panic!("missing row {row_index} for {label}"));
    let semantic_columns = row
        .chars()
        .enumerate()
        .filter_map(|(index, ch)| ch.is_alphanumeric().then_some(index))
        .collect::<Vec<_>>();

    assert!(
        !semantic_columns.is_empty(),
        "{label} row should contain semantic content\n{row}"
    );
    assert!(
        semantic_columns
            .iter()
            .all(|index| fgs[*index] == expected_fg),
        "{label} row should use the expected foreground palette\n{row}"
    );
    assert!(
        semantic_columns
            .iter()
            .all(|index| bgs[*index] == expected_bg),
        "{label} row should use the expected background palette\n{row}"
    );
}

pub(crate) fn render_live_screen(app: &app::AppState, width: u16, height: u16) -> String {
    let debug = render_live_buffer(app, width, height);
    let mut in_content = false;
    let mut rows = Vec::new();

    for line in debug.lines() {
        if line.trim() == "content: [" {
            in_content = true;
            continue;
        }
        if in_content && line.trim() == "]," {
            break;
        }
        if !in_content {
            continue;
        }

        let trimmed = line.trim();
        if let Some(content) = trimmed
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix("\","))
        {
            rows.push(content.to_string());
        }
    }

    rows.join("\n")
}

pub(crate) fn assert_operator_sidebar_expanded(
    app: &app::AppState,
    modified_files_heading: &str,
    expected_marker: &str,
    compact_width: u16,
) {
    let plan = layout::FrameLayoutPlan::for_app(app, ratatui::layout::Rect::new(0, 0, 160, 30));
    let sidebar_text = operator_sidebar_text(app);
    let rendered = render_live_lines(app, 160, 30);

    if let Some(sidebar) = plan.operator_sidebar {
        assert_eq!(
            sidebar.width, compact_width,
            "persistent operator rail width should stay fixed"
        );
        assert_eq!(plan.wheel_hit_areas.overlay, Some(sidebar));
        assert!(rendered.contains(expected_marker));
    }
    assert!(sidebar_text.contains("▼ MCP"));
    assert!(sidebar_text.contains("▼ LSP"));
    assert!(sidebar_text.contains(modified_files_heading));
    assert!(sidebar_text.contains(expected_marker));
}

pub(crate) fn assert_markers_in_order(text: &str, markers: &[&str]) {
    let mut search_from = 0usize;
    for marker in markers {
        let relative = text[search_from..]
            .find(marker)
            .unwrap_or_else(|| panic!("missing marker {marker:?} in\n{text}"));
        search_from += relative;
    }
}

pub(crate) fn assert_live_shell_geometry(width: u16, height: u16) {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }

    let rendered = render_live_lines(&app, width, height);
    assert_live_shell_frame_invariants(&rendered, width, height);

    let lines = rendered.lines().collect::<Vec<_>>();
    assert_live_shell_composer_progressive_disclosure(&lines, None, "Shift+Tab:mode");
}

pub(crate) fn assert_live_shell_contains(
    app: &app::AppState,
    width: u16,
    height: u16,
    markers: &[&str],
) {
    let rendered = render_live_lines(app, width, height);
    assert_live_shell_frame_invariants(&rendered, width, height);

    for marker in markers {
        assert!(
            rendered.contains(marker),
            "expected live shell to contain {marker:?}\n{rendered}"
        );
    }
}

pub(crate) fn runtime_overlay_text(
    app: &app::AppState,
    max_chars: usize,
) -> ui::RuntimeOverlayTextForTest {
    ui::runtime_overlay_text_for_test(app, max_chars).unwrap_or_abort()
}

pub(crate) fn assert_live_shell_document_composer_contract(
    app: &app::AppState,
    width: u16,
    height: u16,
    composer_marker: Option<&str>,
    composer_footer_marker: Option<&str>,
    global_footer_marker: &str,
) {
    let rendered = render_live_lines(app, width, height);
    assert_live_shell_frame_invariants(&rendered, width, height);

    let plan =
        layout::FrameLayoutPlan::for_app(app, ratatui::layout::Rect::new(0, 0, width, height));
    let dock = plan.dock.unwrap_or_abort();
    let composer = dock.composer;
    let dock_width = usize::from(composer.width);
    let lines = rendered.lines().collect::<Vec<_>>();
    let composer_first_row = usize::from(composer.y);
    let composer_last_row =
        composer_first_row.saturating_add(usize::from(composer.height.saturating_sub(1)));
    let disclosure_row = dock.disclosure.map(|band| usize::from(band.y));
    let composer_input_row = match composer_marker {
        Some(marker) => {
            let input_row = (composer_first_row..=composer_last_row)
                .find(|&index| line_has_composer_text(lines[index]))
                .unwrap_or_else(|| panic!("missing composer input row for {marker:?}\n{rendered}"));
            assert!(
                lines[composer_first_row..=composer_last_row]
                    .iter()
                    .any(|line| line.contains(marker)),
                "missing composer marker {marker:?} inside the prompt shell\n{rendered}"
            );
            input_row
        }
        None => {
            let legacy_markers = [
                "Ask Harness to inspect, edit, or explain…",
                "Queue the next turn while this one finishes…",
                "Ask Harness what to retry, inspect, or fix…",
                "Draft preserved locally while recovery completes.",
                "Draft preserved locally — reopen the TUI to reconnect.",
                "Run complete — use ctrl+p commands",
            ];
            assert!(
                !lines[composer_first_row..=composer_last_row]
                    .iter()
                    .any(|line| legacy_markers.iter().any(|marker| line.contains(marker))),
                "live composer should stay blank when no draft is present\n{rendered}"
            );
            composer_first_row
        }
    };
    let global_footer_row = find_line_containing_from(&lines, 0, global_footer_marker)
        .or_else(|| find_line_containing_from(&lines, 0, "Shift+Tab:mode"))
        .or_else(|| find_line_containing_from(&lines, 0, "Ctrl+x:shortcuts"))
        .or_else(|| find_line_containing_from(&lines, 0, "Ctrl+p commands"))
        .or_else(|| find_line_containing_from(&lines, 0, "? commands"))
        .or_else(|| find_line_containing_from(&lines, 0, "q quit"))
        .or_else(|| find_line_containing_from(&lines, 0, "Enter send"))
        .or_else(|| find_line_containing_from(&lines, composer_last_row + 1, "q quit"))
        .unwrap_or_else(|| {
            panic!(
                "missing global footer marker {global_footer_marker:?} for {composer_marker:?}\n{rendered}"
            )
        });
    assert!(
        find_line_containing_in_range(&lines, composer_first_row, composer_input_row, "Composer ·")
            .is_none(),
        "metadata headline row must stay removed\n{rendered}"
    );
    assert!(
        lines[composer_first_row..=composer_last_row]
            .iter()
            .all(|line| line.chars().take(dock_width).count() <= dock_width),
        "composer shell rows must stay within the dock width\n{rendered}"
    );

    match composer_footer_marker {
        Some(marker) => {
            let composer_footer_row =
                find_line_containing_from(&lines, composer_last_row + 1, marker).unwrap_or_else(
                    || {
                        panic!(
                    "missing composer footer marker {marker:?} for {composer_marker:?}\n{rendered}"
                )
                    },
                );
            assert_eq!(
                composer_footer_row,
                composer_last_row + 1,
                "composer hint row should sit directly under the prompt shell\n{rendered}"
            );
            assert!(
                global_footer_row < composer_first_row
                    || global_footer_row <= composer_last_row
                    || global_footer_row >= composer_footer_row,
                "global footer should live above the dock, in the composer metadata row, or below the helper row\n{rendered}"
            );
        }
        None => {
            assert!(
                global_footer_row < composer_first_row
                    || global_footer_row <= composer_last_row
                    || Some(global_footer_row) == disclosure_row
                    || Some(global_footer_row) == disclosure_row.map(|row| row + 1),
                "the global footer should live above the dock, in the composer metadata row, the disclosure row, or directly under it\n{rendered}"
            );
        }
    }
}

pub(crate) fn assert_replay_read_only_composer_contract(
    app: &app::AppState,
    width: u16,
    height: u16,
    header_marker: &str,
    hint_marker: &str,
) {
    let rendered = render_live_lines(app, width, height);
    assert_live_shell_frame_invariants(&rendered, width, height);

    let lines = rendered.lines().collect::<Vec<_>>();
    let header_row = find_line_containing(&lines, header_marker).unwrap_or_else(|| {
        panic!("missing replay header marker {header_marker:?} in shell\n{rendered}")
    });
    let composer_row = find_line_containing_from(&lines, header_row + 1, "▎ Replay is read-only.")
        .unwrap_or_else(|| {
            panic!("missing replay read-only body row for header {header_marker:?}\n{rendered}")
        });
    let divider_row = composer_row.saturating_sub(1);
    let hint_row =
        find_line_containing_from(&lines, composer_row + 1, hint_marker).unwrap_or_else(|| {
            panic!(
            "missing replay shortcut row {hint_marker:?} for header {header_marker:?}\n{rendered}"
        )
        });

    assert!(
        header_row < divider_row,
        "replay identity should sit in header context\n{rendered}"
    );
    assert_eq!(
        hint_row,
        composer_row + 1,
        "replay should keep one compact shortcut row under the disabled rail row\n{rendered}"
    );
    assert!(
        find_line_containing_in_range(&lines, divider_row, composer_row, "Replay archive ·")
            .is_none(),
        "replay composer should not render a metadata headline row\n{rendered}"
    );
    assert!(
        !lines[composer_row].contains("run run_fixture"),
        "replay identity should stay out of the disabled rail row\n{rendered}"
    );
}

pub(crate) fn render_live_lines(app: &app::AppState, width: u16, height: u16) -> String {
    use ratatui::{backend::TestBackend, Terminal};

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, app))
        .unwrap_or_abort();

    terminal
        .backend()
        .buffer()
        .content
        .chunks(width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn transcript_turn_group_test_activity(
    request_id: &str,
    status: app::ActivityStatus,
    user_text: Option<&str>,
    transcript_text: &str,
) -> app::ActivityEntry {
    app::ActivityEntry {
        request_id: request_id.to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status,
        user_message: user_text.map(|text| harness_core::event::UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: text.to_string(),
        }),
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: transcript_text.to_string(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        request_started_mono_ms: None,
        revision: 0,
    }
}

pub(crate) fn live_status_strip_row(
    app: &app::AppState,
    width: u16,
    height: u16,
    marker: &str,
) -> String {
    let rendered = render_live_lines(app, width, height);
    let lines = rendered.lines().collect::<Vec<_>>();
    let row = lines
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, line)| line.contains(marker).then_some(index))
        .unwrap_or_abort();
    lines[row].trim().to_string()
}

pub(crate) fn assert_live_shell_frame_invariants(rendered: &str, width: u16, height: u16) {
    let lines = rendered.lines().collect::<Vec<_>>();
    assert_eq!(
        lines.len(),
        height as usize,
        "row count must match geometry"
    );
    assert!(
        lines
            .iter()
            .all(|line| line.chars().count() == width as usize),
        "every row must preserve the requested width"
    );
}

pub(crate) fn find_line_containing(lines: &[&str], needle: &str) -> Option<usize> {
    lines.iter().position(|line| line.contains(needle))
}

pub(crate) fn find_line_containing_all(lines: &[&str], needles: &[&str]) -> Option<usize> {
    lines
        .iter()
        .position(|line| needles.iter().all(|needle| line.contains(needle)))
}

pub(crate) fn find_last_line_containing(lines: &[&str], needle: &str) -> Option<usize> {
    lines.iter().rposition(|line| line.contains(needle))
}

pub(crate) fn count_lines_containing(lines: &[&str], needle: &str) -> usize {
    lines.iter().filter(|line| line.contains(needle)).count()
}

pub(crate) fn find_line_containing_from(
    lines: &[&str],
    start: usize,
    needle: &str,
) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, line)| line.contains(needle).then_some(index))
}

pub(crate) fn find_line_containing_all_from(
    lines: &[&str],
    start: usize,
    needles: &[&str],
) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, line)| {
            needles
                .iter()
                .all(|needle| line.contains(needle))
                .then_some(index)
        })
}

pub(crate) fn find_line_containing_in_range(
    lines: &[&str],
    start: usize,
    end_exclusive: usize,
    needle: &str,
) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .take(end_exclusive.saturating_sub(start))
        .find_map(|(index, line)| line.contains(needle).then_some(index))
}

pub(crate) fn first_alphanumeric_column(line: &str) -> usize {
    line.chars()
        .position(char::is_alphanumeric)
        .unwrap_or_else(|| panic!("line is missing alphanumeric content: {line:?}"))
}

pub(crate) fn first_non_whitespace_column(line: &str) -> usize {
    line.chars()
        .position(|ch| !ch.is_whitespace())
        .unwrap_or_else(|| panic!("line is missing visible content: {line:?}"))
}

pub(crate) fn live_shell_composer_input_span(lines: &[&str]) -> (usize, usize, usize) {
    let composer_first_row = (0..lines.len())
        .rev()
        .find(|&index| composer_prompt_glyph_line(lines[index]))
        .unwrap_or_abort();
    let composer_input_row = (composer_first_row..lines.len())
        .find(|&index| line_has_composer_text(lines[index]))
        .unwrap_or_abort();
    let footer_row = find_line_containing(lines, "Shift+Tab:mode")
        .or_else(|| find_line_containing(lines, "Ctrl+x:shortcuts"))
        .or_else(|| find_line_containing(lines, "Ctrl+p commands"))
        .or_else(|| find_line_containing(lines, "ctrl+p commands"))
        .or_else(|| find_line_containing(lines, "? commands"))
        .or_else(|| find_last_line_containing(lines, "q quit"));
    let mut composer_last_row = composer_input_row.max(composer_first_row);
    let stop_at = footer_row.unwrap_or(lines.len());
    while composer_last_row + 1 < stop_at
        && composer_body_continuation_line(lines[composer_last_row + 1])
    {
        composer_last_row += 1;
    }

    (composer_first_row, composer_input_row, composer_last_row)
}

fn composer_prompt_glyph_line(line: &str) -> bool {
    let trimmed = line.trim_start().trim_start_matches('│').trim_start();
    trimmed.starts_with('▎') || trimmed.starts_with('❯')
}

fn composer_body_continuation_line(line: &str) -> bool {
    let trimmed = line.trim_start().trim_start_matches('│').trim_start();
    if trimmed.is_empty() {
        return true;
    }
    if footer_or_disclosure_line(line) || composer_prompt_glyph_line(line) {
        return false;
    }
    if trimmed.starts_with('╰') || trimmed.starts_with('╭') || trimmed.starts_with('─') {
        return false;
    }
    trimmed.starts_with("line ")
        || trimmed.starts_with("run a shell")
        || trimmed.chars().any(char::is_alphanumeric)
}

fn footer_or_disclosure_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("ctrl+")
        || trimmed.starts_with("Ctrl+")
        || trimmed.starts_with("Shift+Tab")
        || trimmed.contains("q quit")
        || trimmed.contains("Ctrl+p open")
        || trimmed.contains("Ctrl+p commands")
        || trimmed.contains("ctrl+p commands")
        || trimmed.contains("Shift+Tab:mode")
        || trimmed.contains("Ctrl+x:shortcuts")
        || trimmed.contains("? commands")
        || trimmed.contains("? shortcuts")
        || trimmed.contains("h shortcuts")
}

pub(crate) fn line_has_composer_text(line: &str) -> bool {
    let trimmed = line.trim_start().trim_start_matches('│').trim_start();
    if footer_or_disclosure_line(line) {
        return false;
    }
    if trimmed.starts_with('▎') || trimmed.starts_with('❯') {
        return trimmed.chars().skip(1).any(char::is_alphanumeric);
    }
    line.starts_with(' ') && trimmed.chars().any(char::is_alphanumeric)
}

pub(crate) fn assert_live_shell_composer_progressive_disclosure(
    lines: &[&str],
    composer_marker: Option<&str>,
    footer_marker: &str,
) {
    let footer_row = find_line_containing(lines, footer_marker)
        .or_else(|| find_line_containing(lines, "Shift+Tab:mode"))
        .or_else(|| find_line_containing(lines, "Ctrl+x:shortcuts"))
        .or_else(|| find_line_containing(lines, "? commands"))
        .or_else(|| find_line_containing(lines, "Ctrl+p commands"))
        .or_else(|| find_last_line_containing(lines, "q quit"))
        .or_else(|| {
            lines
                .iter()
                .enumerate()
                .rev()
                .find_map(|(index, line)| (!line.trim().is_empty()).then_some(index))
        })
        .unwrap_or_abort();
    let composer_first_row = (0..=footer_row)
        .rev()
        .find(|&index| composer_prompt_glyph_line(lines[index]))
        .unwrap_or_abort();
    let composer_last_row = footer_row.saturating_sub(1);
    let composer_input = match composer_marker {
        Some(marker) => {
            let input_row = (composer_first_row..=composer_last_row)
                .find(|&index| line_has_composer_text(lines[index]))
                .unwrap_or_abort();
            assert!(
                lines[composer_first_row..=composer_last_row]
                    .iter()
                    .any(|line| line.contains(marker)),
                "composer marker should stay inside the prompt shell"
            );
            input_row
        }
        None => composer_first_row,
    };

    assert!(lines[..composer_first_row]
        .iter()
        .any(|line| !line.trim().is_empty()));
    assert!(composer_first_row <= composer_input);
    assert!(composer_input < footer_row);

    if let Some(headline_row) =
        find_line_containing_in_range(lines, composer_first_row, composer_input, "Composer")
    {
        assert!(headline_row < composer_input);
    }

    if let Some(hints_row) =
        find_line_containing_in_range(lines, composer_input + 1, footer_row, "Shift+Tab:mode")
            .or_else(|| {
                find_line_containing_in_range(
                    lines,
                    composer_input + 1,
                    footer_row,
                    "Ctrl+x:shortcuts",
                )
            })
            .or_else(|| {
                find_line_containing_in_range(
                    lines,
                    composer_input + 1,
                    footer_row,
                    "Ctrl+p commands",
                )
            })
            .or_else(|| {
                find_line_containing_in_range(
                    lines,
                    composer_input + 1,
                    footer_row,
                    "ctrl+p commands",
                )
            })
            .or_else(|| {
                find_line_containing_in_range(lines, composer_input + 1, footer_row, "? commands")
            })
    {
        assert!(composer_input < hints_row);
        assert!(hints_row < footer_row);
    }
}
