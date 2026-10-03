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
            .without_env("HOME")
            .without_env("XDG_CONFIG_HOME")
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
