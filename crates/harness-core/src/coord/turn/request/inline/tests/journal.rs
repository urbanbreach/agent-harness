//! The inline savings journal.
use crate::coord::context::archive::inline_cache::SavingsState;
use crate::coord::turn::request::inline::savings::{
    dedup, path, read, record, total_saved, Saving,
};
use std::path::Path;

type Test = Result<(), Box<dyn std::error::Error>>;

fn saving(id: &str, tokens: u64) -> Saving {
    Saving {
        tool_call_id: id.into(),
        saved_tokens: tokens,
    }
}

fn lines(dir: &Path) -> Result<Vec<String>, std::io::Error> {
    Ok(std::fs::read_to_string(path(dir))?
        .lines()
        .map(str::to_owned)
        .collect())
}

#[test]
fn a_batch_appends_attributed_lines_in_field_order_with_one_timestamp() -> Test {
    let dir = tempfile::tempdir()?;
    let state = SavingsState::default();
    record(
        &state,
        dir.path(),
        Some("session-1"),
        "anthropic",
        "claude",
        &[saving("t1", 100), saving("t2", 0), saving("t3", 7)],
        1_234,
    );
    assert_eq!(
        lines(dir.path())?,
        [
            r#"{"ts":1234,"session":"session-1","provider":"anthropic","model":"claude","toolCallId":"t1","savedTokens":100}"#,
            r#"{"ts":1234,"session":"session-1","provider":"anthropic","model":"claude","toolCallId":"t3","savedTokens":7}"#,
        ]
    );
    Ok(())
}

#[test]
fn an_id_is_recorded_once_for_the_recorder_lifetime_whatever_the_session() -> Test {
    let dir = tempfile::tempdir()?;
    let state = SavingsState::default();
    record(
        &state,
        dir.path(),
        Some("a"),
        "p",
        "m",
        &[saving("t1", 5)],
        1,
    );
    record(
        &state,
        dir.path(),
        Some("b"),
        "q",
        "n",
        &[saving("t1", 9)],
        2,
    );
    assert_eq!(lines(dir.path())?.len(), 1);
    Ok(())
}

#[test]
fn a_missing_session_or_nonpositive_saving_does_not_consume_the_id() -> Test {
    let dir = tempfile::tempdir()?;
    let state = SavingsState::default();
    record(&state, dir.path(), None, "p", "m", &[saving("t1", 5)], 1);
    record(
        &state,
        dir.path(),
        Some(""),
        "p",
        "m",
        &[saving("t1", 5)],
        1,
    );
    record(
        &state,
        dir.path(),
        Some("s"),
        "p",
        "m",
        &[saving("t2", 0)],
        1,
    );
    assert!(!path(dir.path()).exists());
    record(
        &state,
        dir.path(),
        Some("s"),
        "p",
        "m",
        &[saving("t1", 5), saving("t2", 6)],
        2,
    );
    assert_eq!(lines(dir.path())?.len(), 2);
    Ok(())
}

#[test]
fn a_failed_write_still_consumes_the_id_and_the_directory_flag_waits_for_success() -> Test {
    let root = tempfile::tempdir()?;
    let blocker = root.path().join("file");
    std::fs::write(&blocker, "x")?;
    let state = SavingsState::default();
    record(
        &state,
        &blocker.join("data"),
        Some("s"),
        "p",
        "m",
        &[saving("t1", 5)],
        1,
    );
    assert!(state.inner.lock().is_ok_and(|inner| !inner.dir_ready));
    let dir = root.path().join("data");
    record(
        &state,
        &dir,
        Some("s"),
        "p",
        "m",
        &[saving("t1", 5), saving("t2", 5)],
        2,
    );
    assert_eq!(lines(&dir)?.len(), 1);
    assert!(state.inner.lock().is_ok_and(|inner| inner.dir_ready));
    Ok(())
}

#[test]
fn the_reader_skips_blank_and_malformed_lines_and_reports_other_errors() -> Test {
    let dir = tempfile::tempdir()?;
    assert!(read(dir.path())?.is_empty());
    std::fs::write(
        path(dir.path()),
        "{\"session\":\"s\",\"toolCallId\":\"t\",\"savedTokens\":3}\n\nnot json\n42\n",
    )?;
    assert_eq!(read(dir.path())?.len(), 2);
    let broken = tempfile::tempdir()?;
    std::fs::create_dir(path(broken.path()))?;
    assert!(read(broken.path()).is_err());
    Ok(())
}

#[test]
fn aggregates_count_a_session_and_call_id_once_across_recorders() -> Test {
    let dir = tempfile::tempdir()?;
    for _ in 0..2 {
        let state = SavingsState::default();
        record(
            &state,
            dir.path(),
            Some("s"),
            "p",
            "m",
            &[saving("t1", 40)],
            1,
        );
    }
    let state = SavingsState::default();
    record(
        &state,
        dir.path(),
        Some("other"),
        "p",
        "m",
        &[saving("t1", 2)],
        1,
    );
    let entries = read(dir.path())?;
    assert_eq!(entries.len(), 3);
    assert_eq!(total_saved(&dedup(entries)), 42);
    Ok(())
}
