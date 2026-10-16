use super::*;
use harness_core::event::{ActorKind, EventActor, SessionTitleUpdatedEvent, SCHEMA_VERSION};
use harness_core::store::{EventEnvelopeWithoutSeqV1, EventStore};

#[test]
fn state_documents_are_lazy_and_names_cannot_escape_storage() -> Result<(), String> {
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    for name in [
        "",
        "../outside",
        "a/b",
        "Uppercase",
        "a.json",
        "1state",
        &"a".repeat(33),
    ] {
        assert!(
            state_path(directory.path(), name).is_err(),
            "accepted {name:?}"
        );
    }
    let path = state_path(directory.path(), "workspace-2")?;
    assert_eq!(read_json(&path)?, None);
    assert!(!directory.path().join("gui").exists());
    write_json(&path, &json!({"tabs": ["one"]}))?;
    write_json(&path, &json!({"tabs": ["two"]}))?;
    assert_eq!(read_json(&path)?, Some(json!({"tabs": ["two"]})));
    Ok(())
}

#[test]
fn idle_rename_appends_without_rewriting_history() -> Result<(), String> {
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let journal =
        Journal::open(directory.path(), "saved-run", false).map_err(|error| error.to_string())?;
    let original = journal
        .append(EventEnvelopeWithoutSeqV1 {
            schema_version: SCHEMA_VERSION,
            event_id: "original-title".into(),
            run_id: "saved-run".into(),
            mono_ms: 10,
            ts: None,
            actor: EventActor::new(ActorKind::System, None),
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload: EventV1::SessionTitleUpdated(SessionTitleUpdatedEvent {
                title: "Original".into(),
            }),
        })
        .map_err(|error| error.to_string())?;
    drop(journal);
    let path = directory.path().join("saved-run");
    let credential = "opaque-configured-credential";
    let config = harness_core::config::load_config_from_str(&format!(
            r#"{{"provider":{{"local":{{"type":"openai_compatible","apiKey":"{credential}","models":{{"default":{{}}}}}}}},"model":"local/default"}}"#
        ))
        .map_err(|error| error.to_string())?;
    let deps =
        harness::CliDeps::real().with_env("HARNESS_HOME", directory.path().to_string_lossy());
    rename_idle(
        &path,
        format!("Renamed {credential} sk-1234567890abcdef"),
        &config,
        &deps,
    )?;
    let events = read_bounded_history(&path)?;
    assert_eq!(events.len(), 2);
    assert_eq!(events.first(), Some(&original));
    let renamed = events.get(1).ok_or("missing title update")?;
    assert_eq!(renamed.seq, original.seq + 1);
    assert_eq!(
        renamed.payload,
        EventV1::SessionTitleUpdated(SessionTitleUpdatedEvent {
            title: "Renamed [REDACTED] [REDACTED_API_KEY]".into(),
        })
    );
    let persisted =
        std::fs::read_to_string(path.join("events.jsonl")).map_err(|error| error.to_string())?;
    assert!(!persisted.contains(credential));
    assert!(!persisted.contains("sk-1234567890abcdef"));
    Ok(())
}

#[test]
fn search_excerpt_keeps_unicode_matches_in_context() {
    let text = format!("{}İstanbul needle after", "界".repeat(300));
    let excerpt = snippet(&text, "needle");
    assert!(excerpt.is_some_and(|value| value.contains("needle") && value.starts_with("...")));
}
