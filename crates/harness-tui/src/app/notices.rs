//! Transcript notices: dim status rows, handoff success blocks and padded `Warning:` /
//! `Error:` rows that maintenance and runtime notices leave in the conversation.
use super::AppState;
use harness_core::event::NoticeSeverity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TranscriptNoticeKind {
    /// Dim status text. A status that is still the last transcript row is replaced by the
    /// next one instead of stacking.
    Status,
    /// Accent success block with a leading spacer.
    Success,
    Warning,
    Error,
}

impl From<NoticeSeverity> for TranscriptNoticeKind {
    fn from(severity: NoticeSeverity) -> Self {
        match severity {
            NoticeSeverity::Info => Self::Status,
            NoticeSeverity::Warning => Self::Warning,
            NoticeSeverity::Error => Self::Error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptNotice {
    /// Highest durable event sequence seen when the notice arrived; the notice renders
    /// after the turn that ends at or before it.
    pub(crate) anchor_seq: u64,
    pub(crate) kind: TranscriptNoticeKind,
    pub(crate) text: String,
}

const MAX_NOTICES: usize = 64;

#[derive(Default)]
pub(crate) struct TranscriptNotices {
    rows: Vec<TranscriptNotice>,
    /// The last row is still the last transcript child: nothing was appended after it.
    last_is_tail: bool,
}

impl TranscriptNotices {
    pub(crate) fn rows(&self) -> &[TranscriptNotice] {
        &self.rows
    }

    pub(crate) fn push(&mut self, anchor_seq: u64, kind: TranscriptNoticeKind, text: String) {
        let text = crate::text::strip_ansi_escapes(&text);
        if text.trim().is_empty() {
            return;
        }
        if kind == TranscriptNoticeKind::Status
            && self.last_is_tail
            && let Some(last) = self
                .rows
                .last_mut()
                .filter(|last| last.kind == TranscriptNoticeKind::Status)
        {
            last.anchor_seq = anchor_seq;
            last.text = text;
            return;
        }
        if self.rows.len() == MAX_NOTICES {
            self.rows.remove(0);
        }
        self.rows.push(TranscriptNotice {
            anchor_seq,
            kind,
            text,
        });
        self.last_is_tail = true;
    }

    /// Anything else landing in the transcript ends status coalescing.
    pub(crate) fn note_transcript_child(&mut self) {
        self.last_is_tail = false;
    }

    pub(crate) fn clear(&mut self) {
        self.rows.clear();
        self.last_is_tail = false;
    }
}

impl AppState {
    pub(crate) fn transcript_notices(&self) -> &[TranscriptNotice] {
        self.notices.rows()
    }

    pub(crate) fn push_transcript_notice(&mut self, kind: TranscriptNoticeKind, text: String) {
        let anchor = self.events().last().map_or(0, |event| event.seq);
        self.notices.push(anchor, kind, text);
        self.bump_transcript_render_epoch();
        self.sync_transcript_state(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_statuses_coalesce_until_another_row_lands() {
        let mut notices = TranscriptNotices::default();
        notices.push(1, TranscriptNoticeKind::Status, "first".into());
        notices.push(2, TranscriptNoticeKind::Status, "second".into());
        assert_eq!(notices.rows().len(), 1);
        assert_eq!(notices.rows()[0].text, "second");
        notices.push(3, TranscriptNoticeKind::Error, "boom".into());
        notices.push(3, TranscriptNoticeKind::Status, "after".into());
        assert_eq!(notices.rows().len(), 3);
        notices.note_transcript_child();
        notices.push(4, TranscriptNoticeKind::Status, "later".into());
        assert_eq!(notices.rows().len(), 4);
    }
}
