//! The pending band above the editor: what is waiting to go, in two groups.
use super::{AppState, PendingQueueDisplay};

const MAX_BAND_ROWS: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PendingBandRow {
    Heading(&'static str),
    Item(String),
    /// Entries that did not fit.
    More(usize),
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Rows of the band, at most `max_rows`: each non-empty group is a heading and its entries.
pub(crate) fn band_rows(display: &PendingQueueDisplay, max_rows: usize) -> Vec<PendingBandRow> {
    let mut rows = Vec::new();
    for (heading, items) in [
        ("Steering", &display.steering),
        ("After yield", &display.after_yield),
    ] {
        if items.is_empty() {
            continue;
        }
        rows.push(PendingBandRow::Heading(heading));
        rows.extend(
            items
                .iter()
                .map(|item| PendingBandRow::Item(one_line(item))),
        );
    }
    if rows.len() > max_rows && max_rows > 0 {
        let kept = max_rows - 1;
        let hidden = rows[kept..]
            .iter()
            .filter(|row| matches!(row, PendingBandRow::Item(_)))
            .count();
        rows.truncate(kept);
        rows.push(PendingBandRow::More(hidden));
    }
    rows
}

impl AppState {
    /// Rows the band may use on a terminal of this height.
    pub(crate) fn pending_band_capacity(terminal_height: u16) -> usize {
        usize::from(terminal_height / 5).min(MAX_BAND_ROWS)
    }

    pub(crate) fn pending_band(&self, max_rows: usize) -> Vec<PendingBandRow> {
        band_rows(&self.pending_queue_display(), max_rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(steering: &[&str], after: &[&str]) -> PendingQueueDisplay {
        PendingQueueDisplay {
            steering: steering.iter().map(|text| (*text).to_owned()).collect(),
            after_yield: after.iter().map(|text| (*text).to_owned()).collect(),
        }
    }

    #[test]
    fn groups_are_steering_then_after_yield_and_overflow_is_counted() {
        use PendingBandRow::{Heading, Item, More};
        for (display, max_rows, expected) in [
            (display(&[], &[]), 6, vec![]),
            (
                display(&["a\nb"], &["c"]),
                6,
                vec![
                    Heading("Steering"),
                    Item("a b".into()),
                    Heading("After yield"),
                    Item("c".into()),
                ],
            ),
            (
                display(&[], &["only"]),
                6,
                vec![Heading("After yield"), Item("only".into())],
            ),
            (
                display(&["a", "b", "c"], &["d"]),
                4,
                vec![
                    Heading("Steering"),
                    Item("a".into()),
                    Item("b".into()),
                    More(2),
                ],
            ),
        ] {
            assert_eq!(band_rows(&display, max_rows), expected);
        }
    }
}
