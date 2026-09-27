use harness::{run, CliDeps, CliIo};
use harness_core::{
    event::EventV1,
    store::{read_events, Journal},
};
use serde_json::{json, Value};
use std::{fs, io::Cursor, path::Path};

fn invoke(root: &Path, args: &[&str]) -> (i32, Vec<u8>, String) {
    let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
    let result = run(
        ["harness", "--session-dir", "sessions"]
            .into_iter()
            .chain(args.iter().copied()),
        &mut CliIo::new(&mut input, &mut output, &mut errors),
        CliDeps::real().with_current_dir(root.into()),
    );
    (
        result.code,
        output,
        String::from_utf8_lossy(&errors).into_owned(),
    )
}

#[test]
fn saved_snapshot_rewind_validates_before_writes_and_keeps_the_journal_unchanged(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let (code, _, error) = invoke(
        root.path(),
        &[
            "prompt",
            "--mock",
            "--text",
            "Remember this",
            "--session-id",
            "saved",
        ],
    );
    assert_eq!(code, 0, "{error}");
    let journal = root.path().join("sessions/saved/events.jsonl");
    let original = fs::read(&journal)?;
    let events = read_events(&journal)?;
    let cutoff = events
        .iter()
        .find(|e| matches!(e.payload, EventV1::UserMessageSubmitted(_)))
        .ok_or("user event")?
        .seq
        .to_string();
    fs::write(root.path().join("note.txt"), "after")?;
    fs::write(
        root.path().join("snapshot.json"),
        json!([
            {"path":"note.txt","content":"before"}, {"path":"nested/new.txt","content":"new file"}
        ])
        .to_string(),
    )?;
    let args = ["sessions", "rewind", "saved", "--cutoff", &cutoff, "--json"];
    let (code, output, error) = invoke(root.path(), &[&args[..], &["--dry-run"]].concat());
    assert_eq!(code, 0, "{error}");
    let plan: Value = serde_json::from_slice(&output)?;
    assert_eq!(plan["conversation_message_count"], 1);
    assert_eq!(fs::read_to_string(root.path().join("note.txt"))?, "after");
    let apply = [
        &args[..],
        &["--workspace", ".", "--snapshot", "snapshot.json"],
    ]
    .concat();
    let owner = Journal::open_existing(&root.path().join("sessions"), "saved", false)?;
    assert_eq!(invoke(root.path(), &apply).0, 1);
    assert_eq!(fs::read_to_string(root.path().join("note.txt"))?, "after");
    drop(owner);
    let (code, output, error) = invoke(root.path(), &apply);
    assert_eq!(code, 0, "{error}");
    let restored: Value = serde_json::from_slice(&output)?;
    assert_eq!(restored["files_restored"], 2);
    assert_eq!(restored["events_append_only"], true);
    assert_eq!(fs::read_to_string(root.path().join("note.txt"))?, "before");
    assert_eq!(
        fs::read_to_string(root.path().join("nested/new.txt"))?,
        "new file"
    );
    let (code, output, error) = invoke(root.path(), &apply);
    assert_eq!(code, 0, "{error}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output)?["files_unchanged"],
        2
    );
    for path in ["../outside", "sessions/saved/events.jsonl"] {
        fs::write(root.path().join("snapshot.json"), json!([
            {"path":"note.txt","content":"must not be written"}, {"path":path,"content":"invalid"}
        ]).to_string())?;
        assert_eq!(invoke(root.path(), &apply).0, 1);
        assert_eq!(fs::read_to_string(root.path().join("note.txt"))?, "before");
    }
    assert_eq!(
        invoke(
            root.path(),
            &[
                "sessions",
                "rewind",
                "saved",
                "--cutoff",
                "999999",
                "--dry-run"
            ]
        )
        .0,
        1
    );
    assert_eq!(fs::read(&journal)?, original);
    Ok(())
}
