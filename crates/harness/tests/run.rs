use harness::{run, CliDeps, CliIo};
use harness_core::{event::EventV1, store::read_events};
use harness_providers::{mock::MockProvider, MessageRole, Provider, ProviderStreamEvent};
use std::{fs, io::Cursor, sync::Arc};

#[test]
fn scenario_runs_are_deterministic_and_denied_edits_do_not_touch_the_workspace(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let mut original = Vec::new();
    for (step, scenario, succeeds) in [
        (0, "golden_path", true),
        (1, "golden_path", true),
        (2, "golden_path_interactive", false),
    ] {
        let (mut input, mut output, mut error) = (Cursor::new("deny\n"), Vec::new(), Vec::new());
        let result = run(
            [
                "harness",
                "--session-dir",
                "sessions",
                "run",
                "--scenario",
                scenario,
                "--deterministic",
                "--print-run-dir",
            ],
            &mut CliIo::new(&mut input, &mut output, &mut error),
            CliDeps::real()
                .with_current_dir(root.path().into())
                .without_env("HOME")
                .without_env("XDG_CONFIG_HOME")
                .without_env("HARNESS_CONFIG")
                .without_env("HARNESS_TUI_CONFIG")
                .without_env("HARNESS_CONFIG_CONTENT"),
        );
        assert_eq!(
            result.code == 0,
            succeeds,
            "{}",
            String::from_utf8_lossy(&error)
        );
        let run_dir = if succeeds {
            std::path::PathBuf::from(String::from_utf8(output)?.trim())
        } else {
            fs::read_dir(root.path().join("sessions"))?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .find(|p| {
                    p.is_dir()
                        && fs::read_to_string(p.join("meta.json"))
                            .is_ok_and(|s| s.contains(scenario))
                })
                .ok_or("denied session missing")?
        };
        let events = read_events(&run_dir.join("events.jsonl"))?;
        let workspace = events
            .iter()
            .find_map(|e| match &e.payload {
                EventV1::RunStarted(e) => Some(std::path::PathBuf::from(&e.workspace_root)),
                _ => None,
            })
            .ok_or("workspace missing")?;
        assert_eq!(workspace.join("demo.txt").exists(), succeeds);
        if succeeds {
            assert_eq!(
                fs::read_to_string(workspace.join("demo.txt"))?,
                "Hello world\n"
            );
            let journal = fs::read(run_dir.join("events.jsonl"))?;
            if step == 0 {
                original = journal;
            } else {
                assert_eq!(journal, original);
            }
        } else {
            assert!(matches!(
                events.last().map(|e| &e.payload),
                Some(EventV1::RunFailed(_))
            ));
        }
        let metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(run_dir.join("meta.json"))?)?;
        assert_eq!(metadata["mode_source"], "scenario_fixture");
        assert!(metadata["created_at"].is_null());
    }
    Ok(())
}

#[tokio::test]
async fn run_combines_inputs_then_continues_the_latest_resumable_session(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("prompt.txt"), "From file\n")?;
    fs::write(root.path().join("reference.rs"), "fn main() {}")?;
    let provider = Arc::new(MockProvider::script((0..3).map(|_| {
        vec![
            ProviderStreamEvent::TextDelta("Done".into()),
            ProviderStreamEvent::Done { usage: None },
        ]
    })));
    let deps = CliDeps::real()
        .with_current_dir(root.path().into())
        .with_provider_override(Arc::clone(&provider) as Arc<dyn Provider>);
    let mut original = Vec::new();
    for (step, args) in [
        vec![
            "From arguments",
            "--prompt-file",
            "prompt.txt",
            "-f",
            "reference.rs",
            "--session-id",
            "selected",
        ],
        vec!["--continue", "Continue"],
        vec![
            "--session",
            "selected",
            "--fork-session",
            "--session-id",
            "fork",
            "Branch",
        ],
    ]
    .into_iter()
    .enumerate()
    {
        let (mut input, mut output, mut error) = (
            Cursor::new(if step == 0 { "From stdin\r\n" } else { "" }),
            Vec::new(),
            Vec::new(),
        );
        let result = run(
            ["harness", "--session-dir", "sessions", "run", "--mock"]
                .into_iter()
                .chain(args),
            &mut CliIo::new(&mut input, &mut output, &mut error),
            deps.clone(),
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&error));
        assert_eq!(String::from_utf8(output)?.trim(), "Done");
        let journal = root.path().join("sessions/selected/events.jsonl");
        if step == 0 {
            let corrupt = root.path().join("sessions/broken");
            fs::create_dir(&corrupt)?;
            fs::write(corrupt.join("events.jsonl"), "broken newer journal")?;
        } else if step == 1 {
            original = fs::read(&journal)?;
        } else {
            assert_eq!(fs::read(&journal)?, original);
            assert!(root.path().join("sessions/fork/events.jsonl").exists());
        }
        assert!(matches!(
            read_events(&journal)?.last().map(|e| &e.payload),
            Some(EventV1::RunFinished(_))
        ));
    }
    let requests = provider.captured_requests().await;
    assert_eq!(requests.len(), 3);
    let prompts: Vec<_> = requests[2]
        .messages
        .iter()
        .filter(|m| m.role == MessageRole::User)
        .map(|m| m.content.as_str())
        .collect();
    assert_eq!(
        prompts,
        [
            "From arguments\nFrom stdin\nFrom file\n@reference.rs",
            "Continue",
            "Branch"
        ]
    );
    Ok(())
}
