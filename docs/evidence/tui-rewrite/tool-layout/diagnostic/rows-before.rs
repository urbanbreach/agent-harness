use super::*;

#[derive(Default)]
struct Row {
    text: String,
    width: usize,
    leading: usize,
    end: Option<usize>,
    last: Option<usize>,
}

impl Row {
    fn cell(&mut self, text: &str) {
        if text == " " {
            if self.leading == self.width {
                self.leading += 1;
            }
        } else if !text.is_empty() {
            self.end = Some(self.width);
        }
        if !text.is_empty() {
            self.last = Some(self.width);
        }
        self.text.push_str(text);
        self.width += 1;
    }

    fn combining(&mut self, text: &str) {
        if let Some(last) = self.last {
            self.text.push_str(text);
            self.leading = self.leading.min(last);
            self.end = Some(last);
        }
    }

    fn finish(&mut self, rows: &mut Vec<SelectionRow>) {
        let row = std::mem::take(self);
        let (start_cell, end_cell) = row.end.map_or((1, 0), |end| (row.leading, end));
        rows.push(SelectionRow {
            line_index: rows.len(),
            text: row.text,
            width: row.width,
            continues_previous: !rows.is_empty(),
            copy_joiner: None,
            start_cell,
            end_cell,
            links: Vec::new(),
        });
    }
}

fn line_rows(line: &Line<'_>, width: usize, rail: Option<&str>) -> Vec<SelectionRow> {
    let mut row = Row::default();
    let mut rows = Vec::new();
    for span in &line.spans {
        for cluster in span.content.graphemes(true) {
            let cells = cluster.width();
            if cells == 0 {
                // The rail replaces the entire first source cell, including combining marks.
                if rail.is_none() || row.last != Some(0) {
                    row.combining(cluster);
                }
                continue;
            }
            if row.width + cells > width {
                row.finish(&mut rows);
            }
            row.cell(if row.width == 0 {
                rail.unwrap_or(cluster)
            } else {
                cluster
            });
            for _ in 1..cells {
                if row.width == width {
                    row.finish(&mut rows);
                }
                row.cell(if row.width == 0 {
                    rail.unwrap_or("")
                } else {
                    ""
                });
            }
            if row.width == width {
                row.finish(&mut rows);
            }
        }
    }
    if rows.is_empty() || row.width > 0 {
        row.finish(&mut rows);
    }
    rows
}

pub(in crate::ui) fn selection_rows_for_rendered_line(
    line: &Line<'_>,
    width: u16,
) -> Vec<SelectionRow> {
    line_rows(line, usize::from(width.max(1)), None)
}

pub(in crate::ui) fn surface_selection_rows(
    lines: &[Line<'static>],
    width: u16,
    show_outer_rail: bool,
    rail_glyph: &str,
) -> Vec<SelectionRow> {
    let width = usize::from(width.max(1));
    let mut rows = Vec::new();
    for line in lines {
        for mut row in line_rows(line, width, show_outer_rail.then_some(rail_glyph)) {
            row.line_index = rows.len();
            row.exclude_prefix(usize::from(show_outer_rail));
            row.pad_to(width);
            rows.push(row);
        }
    }
    if rows.is_empty() {
        let mut row = blank_selection_row();
        row.pad_to(width);
        rows.push(row);
    }
    rows
}

pub(super) fn aligned_selection_rows_for_line(
    line: &Line<'static>,
    width: usize,
    alignment: Alignment,
) -> Vec<SelectionRow> {
    let mut rows = line_rows(line, width.max(1), None);
    if alignment == Alignment::Left {
        return rows;
    }
    for row in &mut rows {
        row.pad_to(width);
        let end = if row.has_content() {
            row.end_cell + 1
        } else {
            0
        };
        if end == 0 || end >= width {
            continue;
        }
        let leading = match alignment {
            Alignment::Center => (width - end) / 2,
            Alignment::Right => width - end,
            Alignment::Left => 0,
        };
        if leading > 0 {
            row.text = format!(
                "{}{}{}",
                " ".repeat(leading),
                row.text.trim_end_matches(' '),
                " ".repeat(width - leading - end)
            );
            row.start_cell += leading;
            row.end_cell += leading;
        }
    }
    rows
}
