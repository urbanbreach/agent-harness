#[path = "../src/terminal_title/mod.rs"]
mod terminal_title;

use std::io::{self, Write};

use terminal_title::{
    sanitize_title, TitleActivity, TitlePhase, TitleState, TitleWriteError, TitleWriter,
};

#[test]
fn activities_produce_distinct_titles() {
    let activities = [
        TitleActivity::Idle,
        TitleActivity::Streaming,
        TitleActivity::ToolRunning,
        TitleActivity::AwaitingPermission,
        TitleActivity::AwaitingQuestion,
        TitleActivity::Recovering,
        TitleActivity::Failed,
        TitleActivity::Completed,
    ];
    let titles = activities
        .iter()
        .map(|activity| {
            let mut state = TitleState::new();
            state.set_activity(*activity);
            state.current_title("session")
        })
        .collect::<Vec<_>>();
    assert_eq!(titles.len(), 8);
    assert_eq!(
        titles
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        8
    );
}

#[test]
fn activity_changes_set_attention_or_steady_phase() {
    let mut state = TitleState::new();
    state.set_activity(TitleActivity::AwaitingPermission);
    assert_eq!(state.phase, TitlePhase::ActionRequired(0));
    state.set_activity(TitleActivity::Streaming);
    assert_eq!(state.phase, TitlePhase::Steady);
    state.set_activity(TitleActivity::AwaitingQuestion);
    assert!(state.current_title("session").ends_with(" ⚠"));
}

#[test]
fn attention_ticks_decrement_and_wrap_at_eight() {
    let mut state = TitleState::new();
    state.set_activity(TitleActivity::AwaitingQuestion);
    let mut counters = Vec::new();
    for _ in 0..9 {
        state.tick();
        if let TitlePhase::ActionRequired(counter) = state.phase {
            counters.push(counter);
        }
    }
    assert_eq!(counters, [7, 6, 5, 4, 3, 2, 1, 0, 7]);
}

#[test]
fn should_emit_deduplicates_candidates() {
    let mut state = TitleState::new();
    assert!(state.should_emit("first"));
    state.last_emitted = Some("first".to_string());
    assert!(!state.should_emit("first"));
    assert!(state.should_emit("second"));
}

#[test]
fn sanitization_strips_controls_sequences_and_truncates_by_character() {
    let raw = "  a\0b\t\x1b]0;evil\x07\x1b[31m中\x1b[0m  ".to_string();
    assert_eq!(sanitize_title(&raw), "ab 中");
    assert_eq!(sanitize_title(&"界".repeat(201)).chars().count(), 200);
    assert!(!sanitize_title(&"界".repeat(201)).is_empty());
}

#[test]
fn traced_sanitization_reports_stripped_content() {
    let (title, report) = terminal_title::sanitize::sanitize_title_traced("a\0\x1b]x\x07\x1b[31mb");
    assert_eq!(title, "ab");
    assert_eq!(report.control_chars_stripped, 1);
    assert_eq!(report.osc_sequences_stripped, 1);
    assert_eq!(report.csi_sequences_stripped, 1);
    assert!(!report.truncated);
    assert_eq!(report.original_len, 12);
    assert_eq!(report.sanitized_len, 2);
}

#[test]
fn writer_emits_sanitized_osc_and_reset() {
    let mut writer = TitleWriter::new();
    let mut output = Vec::new();
    assert_eq!(
        writer.write_title("hello\x1b]evil\x07", &mut output),
        Ok(true)
    );
    assert_eq!(output, b"\x1b]2;hello\x07");
    assert_eq!(writer.reset(&mut output), Ok(true));
    assert_eq!(output, b"\x1b]2;hello\x07\x1b]2;\x07");
    assert_eq!(writer.reset(&mut output), Ok(false));
}

#[test]
fn writer_suspend_blocks_until_resumed() {
    let mut writer = TitleWriter::new();
    let mut output = Vec::new();
    writer.suspend();
    assert_eq!(writer.write_title("blocked", &mut output), Ok(false));
    writer.resume();
    assert_eq!(writer.write_title("allowed", &mut output), Ok(true));
}

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn writer_returns_io_error() {
    let mut writer = TitleWriter::new();
    let error = writer.write_title("title", &mut FailingWriter);
    assert_eq!(error, Err(TitleWriteError::IoError("closed".to_string())));
}
