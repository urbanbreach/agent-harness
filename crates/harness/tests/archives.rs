use harness::{run, CliDeps, CliIo};
use harness_core::{event::EventV1, session::AssistantPart, store::read_events};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read},
    path::Path,
};

fn invoke(root: &Path, args: &[&str]) -> (i32, String) {
    let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
    let result = run(
        ["harness", "--session-dir", "sessions"]
            .into_iter()
            .chain(args.iter().copied()),
        &mut CliIo::new(&mut input, &mut output, &mut errors),
        CliDeps::real()
            .with_current_dir(root.into())
            .with_env("HARNESS_HOME", root.join("data").to_string_lossy())
            .without_env("HOME")
            .without_env("HARNESS_CONFIG")
            .without_env("HARNESS_TUI_CONFIG")
            .without_env("HARNESS_CONFIG_CONTENT")
            .with_env("OPENAI_API_KEY", "opaque-archive-key"),
    );
    (result.code, String::from_utf8_lossy(&errors).into_owned())
}
fn members(path: &Path) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn std::error::Error>> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(path)?));
    let mut files = BTreeMap::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().into_owned();
        let mut content = Vec::new();
        entry.read_to_end(&mut content)?;
        assert!(files.insert(path, content).is_none());
    }
    Ok(files)
}

#[test]
fn archives_keep_session_exports_private_and_do_not_publish_unsafe_workspace_files(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let (code, error) = invoke(
        root.path(),
        &[
            "prompt",
            "--mock",
            "--text",
            "Hello",
            "--session-id",
            "saved",
        ],
    );
    assert_eq!(code, 0, "{error}");
    fs::write(root.path().join("main.rs"), "fn main() {}\n")?;
    fs::write(root.path().join(".gitignore"), "target/\n")?;
    fs::create_dir(root.path().join("target"))?;
    fs::write(
        root.path().join("target/ignored"),
        "not part of the package",
    )?;
    let source = root.path().join("sessions/saved/events.jsonl");
    let journal = legacy_journal(&source)?;
    fs::write(&source, &journal)?;
    fs::write(
        root.path().join("sessions/saved/raw-provider.json"),
        "unclassified provider payload",
    )?;
    let archive = root.path().join("bundle.tar.gz");
    let wrap = ["wrap", "--output", "bundle.tar.gz"];
    let (code, error) = invoke(root.path(), &wrap);
    assert_eq!(code, 0, "{error}");
    let files = members(&archive)?;
    assert_eq!(
        files.get("main.rs").map(Vec::as_slice),
        Some(b"fn main() {}\n".as_slice())
    );
    assert_eq!(
        files.keys().map(String::as_str).collect::<Vec<_>>(),
        [".gitignore", "main.rs"]
    );
    let (code, error) = invoke(
        root.path(),
        &["trace", "saved", "--output", "bundle.tar.gz", "--json"],
    );
    assert_eq!(code, 0, "{error}");
    let trace = members(&archive)?;
    assert!(trace.contains_key("events.jsonl"));
    assert!(trace.contains_key("meta.json"));
    assert!(trace.contains_key("support.json"));
    assert!(!trace.contains_key("raw-provider.json"));
    assert!(trace
        .values()
        .all(|bytes| !String::from_utf8_lossy(bytes).contains("private legacy reasoning")));
    let (code, error) = invoke(root.path(), &[&wrap[..], &["--with-sessions"]].concat());
    assert_eq!(code, 0, "{error}");
    let files = members(&archive)?;
    assert!(files.contains_key("sessions/saved/events.jsonl"));
    assert!(!files.contains_key("sessions/saved/raw-provider.json"));
    let previous = fs::read(&archive)?;
    fs::write(root.path().join("private.txt"), "opaque-archive-key")?;
    assert_eq!(invoke(root.path(), &wrap).0, 1);
    assert_eq!(fs::read(&archive)?, previous);
    fs::remove_file(root.path().join("private.txt"))?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.path().join("main.rs"), root.path().join("link"))?;
        assert_eq!(invoke(root.path(), &wrap).0, 1);
        assert_eq!(fs::read(&archive)?, previous);
    }
    assert_eq!(fs::read_to_string(source)?, journal);
    Ok(())
}

fn legacy_journal(source: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut events = read_events(source)?;
    for event in &mut events {
        if let EventV1::ProviderRequestFinished(finished) = &mut event.payload {
            finished.metadata.get_or_insert_default().thinking =
                Some(harness_core::event::ProviderThinkingMetadata {
                    summary: Some("private legacy reasoning summary".into()),
                    signature: Some("private legacy reasoning signature".into()),
                    summary_digest: None,
                });
        }
        if let EventV1::AssistantMessageFinished(message) = &mut event.payload {
            message.parts.push(AssistantPart::Reasoning {
                text: "private legacy reasoning".into(),
            });
        }
    }
    let journal = events
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n")
        + "\n";
    Ok(journal)
}

#[test]
fn trace_archive_never_contains_native_compaction_payloads(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::event::{
        ArtifactWrittenEvent, EventV1, NativeCompactionMeta, NativePayloadRef,
        SessionCompactionEvent, NATIVE_ARTIFACT_KIND,
    };
    let root = tempfile::tempdir()?;
    let (code, error) = invoke(
        root.path(),
        &[
            "prompt",
            "--mock",
            "--text",
            "Hello",
            "--session-id",
            "saved",
        ],
    );
    assert_eq!(code, 0, "{error}");
    let session = root.path().join("sessions/saved");
    let payload = b"opaque native compaction ciphertext";
    let digest = blake3::hash(payload).to_hex().to_string();
    let path = format!("artifacts/native-{digest}.bin");
    fs::create_dir_all(session.join("artifacts"))?;
    fs::write(session.join(&path), payload)?;
    let mut events = read_events(&session.join("events.jsonl"))?;
    let template = events.last().cloned().ok_or("empty journal")?;
    let mut artifact = template.clone();
    artifact.seq += 1;
    artifact.event_id = "native-artifact".into();
    artifact.payload = EventV1::ArtifactWritten(ArtifactWrittenEvent {
        path: path.clone(),
        digest: digest.clone(),
        bytes: payload.len() as u64,
        tool_call_id: None,
        tool_metadata: None,
        metadata: BTreeMap::from([("kind".into(), NATIVE_ARTIFACT_KIND.into())]),
    });
    let mut compaction = template;
    compaction.seq += 2;
    compaction.event_id = "native-compaction".into();
    compaction.payload = EventV1::SessionCompaction(SessionCompactionEvent {
        summary: "summary".into(),
        first_kept_event_seq: compaction.seq,
        native: Some(NativeCompactionMeta {
            lane: "openai_v1".into(),
            provider_id: Some("openai".into()),
            model_id: Some("gpt".into()),
            api: Some("responses".into()),
            payload: Some(NativePayloadRef {
                path: path.clone(),
                digest: digest.clone(),
                bytes: payload.len() as u64,
            }),
        }),
        ..Default::default()
    });
    events.extend([artifact, compaction]);
    let journal = events
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n")
        + "\n";
    fs::write(session.join("events.jsonl"), journal)?;
    let (code, error) = invoke(
        root.path(),
        &["trace", "saved", "--output", "bundle.tar.gz", "--json"],
    );
    assert_eq!(code, 0, "{error}");
    let files = members(&root.path().join("bundle.tar.gz"))?;
    assert!(files.keys().all(|name| !name.contains("native-")));
    for bytes in files.values() {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains("opaque native compaction ciphertext"));
        assert!(!text.contains(&path));
        assert!(!text.contains(&digest));
    }
    Ok(())
}
