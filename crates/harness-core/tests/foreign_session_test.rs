//! Foreign session discovery + events.jsonl replay-import tests.

use std::fs;

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent, RunStartedEvent,
    SCHEMA_VERSION,
};
use harness_core::foreign_session::{
    discover_foreign_sessions, import_foreign_session_as_replay, refuse_import_into_active_session,
    summarize_discover_candidates, ForeignAgentKind, ForeignSessionCandidate, ForeignSessionError,
};
use harness_core::proj::SessionModeSource;
use harness_core::UnwrapOrAbort;
use tempfile::tempdir;

fn sample_envelope(seq: u64, run_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq}"),
        seq,
        run_id: run_id.into(),
        mono_ms: seq * 10,
        ts: None,
        actor: EventActor::new(ActorKind::System, None),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn write_events_jsonl(path: &std::path::Path, events: &[EventEnvelopeV1]) {
    let mut body = String::new();
    for event in events {
        body.push_str(&serde_json::to_string(event).unwrap_or_abort());
        body.push('\n');
    }
    fs::write(path, body).unwrap_or_abort();
}

#[test]
fn discover_valid_foreign_session_without_touching_active() {
    // arrange
    // act
    // assert
    // Given: scan root with a valid foreign-looking session + a separate active session
    let root = tempdir().unwrap();
    let scan = root.path().join("foreign-root");
    let active = root.path().join("active-harness-session");
    fs::create_dir_all(&scan).unwrap();
    fs::create_dir_all(&active).unwrap();
    let foreign = scan.join("codex-run-1");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(
        foreign.join("session.json"),
        r#"{"id":"abc","title":"demo"}"#,
    )
    .unwrap();
    let active_marker = active.join("events.jsonl");
    fs::write(&active_marker, r#"{"seq":1}"#).unwrap();
    let active_before = fs::read(&active_marker).unwrap();

    // When
    let found = discover_foreign_sessions(&scan).unwrap();
    let refuse = refuse_import_into_active_session(&foreign, &active);

    // Then
    assert_eq!(found.len(), 1);
    assert!(found[0].is_discoverable());
    assert_eq!(found[0].path(), foreign.as_path());
    assert!(matches!(
        &found[0],
        ForeignSessionCandidate::Discoverable {
            kind: ForeignAgentKind::Codex,
            marker,
            ..
        } if marker == "session.json"
    ));
    assert!(matches!(
        refuse,
        Err(ForeignSessionError::ImportIntoActiveForbidden { .. })
    ));
    assert_eq!(
        fs::read(&active_marker).unwrap(),
        active_before,
        "active session must not be mutated"
    );
}

#[test]
fn reject_corrupt_foreign_session_markers() {
    // Given: session-like dir with empty / invalid markers
    let root = tempdir().unwrap();
    let scan = root.path().join("foreign-root");
    fs::create_dir_all(&scan).unwrap();
    let empty = scan.join("claude-empty");
    fs::create_dir_all(&empty).unwrap();
    fs::write(empty.join("events.jsonl"), b"").unwrap();
    let bad_json = scan.join("claude-bad");
    fs::create_dir_all(&bad_json).unwrap();
    fs::write(bad_json.join("session.json"), b"not-json{").unwrap();

    // When
    let found = discover_foreign_sessions(&scan).unwrap();

    // Then
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(ForeignSessionCandidate::is_corrupt));
    for candidate in &found {
        match candidate {
            ForeignSessionCandidate::Corrupt { kind, reason, .. } => {
                assert_eq!(*kind, ForeignAgentKind::Claude);
                assert!(!reason.is_empty());
            }
            other => panic!("expected Corrupt, got {other:?}"),
        }
    }
}

#[test]
fn reject_non_session_directories() {
    // arrange
    // act
    // assert
    // Given: ordinary directory without markers
    let root = tempdir().unwrap();
    let scan = root.path().join("foreign-root");
    let plain = scan.join("notes");
    fs::create_dir_all(&plain).unwrap();
    fs::write(plain.join("readme.txt"), "hello").unwrap();

    // When
    let found = discover_foreign_sessions(&scan).unwrap();

    // Then
    assert_eq!(found.len(), 1);
    assert!(found[0].is_rejected());
}

#[test]
fn missing_scan_root_fails_closed() {
    // arrange
    // act
    // assert
    let root = tempdir().unwrap();
    let missing = root.path().join("nope");
    let err = discover_foreign_sessions(&missing).unwrap_err();
    assert!(matches!(
        err,
        ForeignSessionError::ScanRootNotDirectory { .. }
    ));
}

#[test]
fn import_events_jsonl_creates_replay_only_session_without_mutating_source() {
    // arrange
    // act
    // assert
    // Given: foreign dir with harness-compatible events.jsonl + empty dest store
    let root = tempdir().unwrap();
    let foreign = root.path().join("foreign-events");
    let dest = root.path().join("harness-sessions");
    fs::create_dir_all(&foreign).unwrap();
    let source_events = vec![
        sample_envelope(
            1,
            "foreign-run",
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/tmp/ws".into(),
            }),
        ),
        sample_envelope(
            2,
            "foreign-run",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".into(),
            }),
        ),
    ];
    let source_path = foreign.join("events.jsonl");
    write_events_jsonl(&source_path, &source_events);
    let source_before = fs::read(&source_path).unwrap();

    // When
    let imported = import_foreign_session_as_replay(&foreign, &dest).unwrap();

    // Then
    assert_eq!(imported.event_count, 2);
    assert_eq!(imported.format, "events_jsonl_v1");
    assert_eq!(imported.mode_source, SessionModeSource::ReplayOnly);
    assert!(imported.run_dir.join("events.jsonl").is_file());
    assert!(imported.run_dir.join("meta.json").is_file());
    assert_eq!(fs::read(&source_path).unwrap(), source_before);

    let body = fs::read_to_string(imported.run_dir.join("events.jsonl")).unwrap();
    assert_eq!(body.lines().count(), 2);
    for (idx, line) in body.lines().enumerate() {
        let event: EventEnvelopeV1 = serde_json::from_str(line).unwrap();
        assert_eq!(event.run_id.as_str(), imported.run_id);
        assert_eq!(event.seq, (idx as u64) + 1);
    }
    let meta: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(imported.run_dir.join("meta.json")).unwrap())
            .unwrap();
    assert_eq!(meta["mode_source"], "replay_only");
    assert_eq!(meta["foreign_import"]["format"], "events_jsonl_v1");
}

#[test]
fn import_unknown_marker_fails_closed() {
    // arrange
    // act
    // assert
    // Given: discoverable foreign session with session.json only
    let root = tempdir().unwrap();
    let foreign = root.path().join("codex-session");
    let dest = root.path().join("harness-sessions");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("session.json"), r#"{"id":"x"}"#).unwrap();

    // When
    let err = import_foreign_session_as_replay(&foreign, &dest).unwrap_err();

    // Then
    assert!(matches!(err, ForeignSessionError::UnsupportedFormat { .. }));
    assert!(!dest.exists() || fs::read_dir(&dest).unwrap().next().is_none());
}

#[test]
fn import_non_envelope_jsonl_fails_closed() {
    // arrange
    // act
    // assert
    // Given: events.jsonl with generic JSON objects (not harness envelopes)
    let root = tempdir().unwrap();
    let foreign = root.path().join("generic-jsonl");
    let dest = root.path().join("harness-sessions");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(
        foreign.join("events.jsonl"),
        r#"{"role":"user","text":"hello"}
"#,
    )
    .unwrap();

    // When
    let err = import_foreign_session_as_replay(&foreign, &dest).unwrap_err();

    // Then
    assert!(matches!(err, ForeignSessionError::SourceParse { .. }));
}

#[test]
fn summarize_discover_candidates_counts_by_status() {
    // arrange
    // act
    // assert
    // Given: mix of discoverable, corrupt, rejected under one scan root
    let root = tempdir().unwrap();
    let scan = root.path().join("foreign-root");
    fs::create_dir_all(&scan).unwrap();

    let good = scan.join("codex-good");
    fs::create_dir_all(&good).unwrap();
    fs::write(good.join("session.json"), r#"{"id":"ok"}"#).unwrap();

    let empty = scan.join("claude-empty");
    fs::create_dir_all(&empty).unwrap();
    fs::write(empty.join("events.jsonl"), b"").unwrap();

    let plain = scan.join("not-a-session");
    fs::create_dir_all(&plain).unwrap();

    // When
    let found = discover_foreign_sessions(&scan).unwrap();
    let summary = summarize_discover_candidates(&found);

    // Then
    assert_eq!(summary.total, found.len());
    assert!(summary.discoverable >= 1);
    assert!(summary.corrupt >= 1);
    assert!(summary.rejected >= 1);
    assert_eq!(
        summary.discoverable + summary.corrupt + summary.rejected,
        summary.total
    );
    // session.json is discoverable but not importable; empty events.jsonl is corrupt
    assert_eq!(summary.importable, 0);
    assert!(!summary.has_importable());
    assert_eq!(summary.discoverable_not_importable, summary.discoverable);
    assert!(summary.one_line().contains("discoverable"));
    assert!(summary.one_line().contains("importable"));
    assert!(summary.one_line().contains("not yet"));
    assert!(summary.one_line().contains("corrupt"));
    assert!(summary.one_line().contains("rejected"));
}

#[test]
fn summarize_discover_candidates_counts_importable_events_jsonl() {
    // arrange
    // act
    // assert
    // Given: discoverable events.jsonl + discoverable session.json under one scan root
    let root = tempdir().unwrap();
    let scan = root.path().join("foreign-root");
    fs::create_dir_all(&scan).unwrap();

    let importable = scan.join("importable-run");
    fs::create_dir_all(&importable).unwrap();
    fs::write(
        importable.join("events.jsonl"),
        r#"{"schema_version":1,"event_id":"evt-1","seq":1,"run_id":"run-src","ts_ms":1,"stream_key":"run:run-src","payload":{"type":"run_started","run_id":"run-src"}}
"#,
    )
    .unwrap();

    let other = scan.join("other-marker");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("session.json"), r#"{"id":"ok"}"#).unwrap();

    // When
    let found = discover_foreign_sessions(&scan).unwrap();
    let summary = summarize_discover_candidates(&found);

    // Then
    assert!(summary.discoverable >= 2);
    assert_eq!(summary.importable, 1);
    assert_eq!(
        summary.discoverable_not_importable,
        summary.discoverable - 1
    );
    assert!(summary.has_importable());
    assert!(found.iter().any(|c| c.is_importable()));
    assert!(found
        .iter()
        .any(|c| c.is_discoverable() && !c.is_importable()));
    assert!(summary.one_line().contains("1 importable"));
}

#[test]
fn multi_source_foreign_scan_discovers_importable_and_corrupt_then_imports_first() {
    // arrange
    // act
    // assert
    // Given: foreign_scan_root with 3 importable events.jsonl sessions + 1 corrupt marker
    let root = tempdir().unwrap();
    let scan = root.path().join("foreign_scan_root");
    let dest = root.path().join("harness-foreign-import-dest");
    fs::create_dir_all(&scan).unwrap();

    let mut first_importable: Option<std::path::PathBuf> = None;
    for (idx, name) in ["foreign-a", "foreign-b", "foreign-c"].iter().enumerate() {
        let session = scan.join(name);
        fs::create_dir_all(&session).unwrap();
        let run_id = format!("run-foreign-{idx}");
        let events = vec![sample_envelope(
            1,
            &run_id,
            EventV1::RunFinished(RunFinishedEvent {
                summary: format!("import-{idx}"),
            }),
        )];
        write_events_jsonl(&session.join("events.jsonl"), &events);
        if first_importable.is_none() {
            first_importable = Some(session);
        }
    }
    let corrupt = scan.join("foreign-corrupt");
    fs::create_dir_all(&corrupt).unwrap();
    fs::write(corrupt.join("events.jsonl"), "{not-valid-json\n").unwrap();

    // When: multi-source discover under foreign_scan_root
    let found = discover_foreign_sessions(&scan).unwrap();
    let summary = summarize_discover_candidates(&found);

    // Then: total>=3 importable>=3 plus corrupt classification
    assert!(
        summary.total >= 4,
        "expected multi-source total: {summary:?}"
    );
    assert!(
        summary.discoverable >= 3 && summary.importable >= 3,
        "expected multi-source importable events.jsonl: {summary:?}"
    );
    assert!(
        summary.corrupt >= 1,
        "expected corrupt events.jsonl handling: {summary:?}"
    );
    assert!(summary.has_importable());
    assert!(found.iter().any(ForeignSessionCandidate::is_corrupt));
    assert_eq!(
        found
            .iter()
            .filter(|candidate| candidate.is_importable())
            .count(),
        3
    );

    // When: first importable is imported into a fresh harness dest
    let import_src = first_importable.expect("first importable path");
    let source_before = fs::read(import_src.join("events.jsonl")).unwrap();
    let imported = import_foreign_session_as_replay(&import_src, &dest).unwrap();

    // Then: replay-only import succeeds; source untouched; corrupt remains non-importable
    assert_eq!(imported.event_count, 1);
    assert_eq!(imported.format, "events_jsonl_v1");
    assert_eq!(imported.mode_source, SessionModeSource::ReplayOnly);
    assert!(imported.run_dir.join("events.jsonl").is_file());
    assert_eq!(
        fs::read(import_src.join("events.jsonl")).unwrap(),
        source_before
    );
    let corrupt_err =
        import_foreign_session_as_replay(&corrupt, &dest.join("must-fail")).unwrap_err();
    assert!(matches!(
        corrupt_err,
        ForeignSessionError::UnsupportedFormat { .. }
    ));
}
