use super::super::{
    compaction_tests::{answer, SUMMARY},
    handle, *,
};
use crate::{
    clock::FakeClock,
    config::{HookLifecycleEvent as Hook, LifecycleHookConfig},
    event::SessionCompactionEvent,
    redact::DefaultRedactor,
};
use harness_providers::mock::MockProvider;
use std::path::Path;

fn hook(event: Hook, script: &str, critical: bool) -> LifecycleHookConfig {
    LifecycleHookConfig {
        id: None,
        event,
        command: vec!["/bin/sh".into(), "-c".into(), script.into()],
        cwd: None,
        timeout_ms: Some(2000),
        critical,
        env: BTreeMap::new(),
    }
}

struct Ran {
    result: Result<ManualCompactionOutcome, CoordinatorError>,
    saved: Vec<SessionCompactionEvent>,
    requests: Vec<String>,
}

/// Two settled turns, then one manual `/compact` with `hooks` configured. `custom` is written
/// as `custom.json` once the sequence of the retained request is known.
async fn compact_with(
    hooks: Vec<LifecycleHookConfig>,
    custom: impl FnOnce(u64) -> String,
    root: &Path,
) -> Result<Ran, Box<dyn std::error::Error>> {
    let provider = Arc::new(MockProvider::script([
        answer("old ".repeat(2000)),
        answer("recent"),
        answer(SUMMARY),
        answer("Short"),
        answer("Spare"),
    ]));
    let mut config = CoordinatorConfig::new(root.join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.compaction.keep_recent_tokens = 100;
    config.compaction.reserve_tokens = Some(0);
    config.compaction.enabled = false;
    config.compaction.method_order = vec![harness_compaction::CompactionMethod::Soft];
    config.hook_runtime_config.shell_allowlist.executables = vec!["/bin/sh".into()];
    config.hook_runtime_config.hooks.lifecycle = hooks;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("compaction hooks", root).await?;
    let agent = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    for text in ["old request", "recent request"] {
        let turn = coordinator
            .request_agent_turn(handle::system(), agent.clone(), text)
            .await?;
        super::super::compaction_tests::settled(&coordinator, &turn).await?;
    }
    let submitted: Vec<u64> = crate::store::read_events(&run.events_path)?
        .iter()
        .filter(|e| matches!(e.payload, EventV1::UserMessageSubmitted(_)))
        .map(|e| e.seq)
        .collect();
    std::fs::write(
        root.join("custom.json"),
        custom(submitted.last().copied().unwrap_or(0)),
    )?;
    let result = coordinator
        .compact_agent_context(agent, None, "manual")
        .await;
    coordinator.stop_run().await?;
    let saved = crate::store::read_events(&run.events_path)?
        .into_iter()
        .filter_map(|e| match e.payload {
            EventV1::SessionCompaction(saved) => Some(saved),
            _ => None,
        })
        .collect();
    let requests = provider
        .captured_requests()
        .await
        .iter()
        .flat_map(|r| r.messages.iter().map(|m| m.content.clone()))
        .collect();
    Ok(Ran {
        result,
        saved,
        requests,
    })
}

const VETO: &str = r#"printf '{"cancel":true}'"#;
const CUSTOM: &str = "cat custom.json";

#[tokio::test]
async fn before_hook_results_replace_wholesale_veto_stops_the_chain_and_failures_do_not_veto() {
    #[derive(Debug, PartialEq)]
    enum Expect {
        Cancelled,
        Engine,
        Custom,
    }
    let before = |script: &str, critical: bool| hook(Hook::CompactionRequested, script, critical);
    let cases = [
        ("veto", vec![before(VETO, true)], Expect::Cancelled),
        (
            "veto beats a custom result in the same object",
            vec![before(
                r#"printf '{"cancel":true,"compaction":{"summary":"x","first_kept_seq":1,"tokens_before":1}}'"#,
                true,
            )],
            Expect::Cancelled,
        ),
        ("custom", vec![before(CUSTOM, true)], Expect::Custom),
        (
            "a later empty object displaces an earlier custom result",
            vec![before(CUSTOM, true), before("printf '{}'", true)],
            Expect::Engine,
        ),
        (
            "plain output does not displace it",
            vec![before(CUSTOM, true), before("echo noted", true)],
            Expect::Custom,
        ),
        (
            "a failing handler contributes nothing",
            vec![before("exit 1", false), before(CUSTOM, true)],
            Expect::Custom,
        ),
        (
            "a failing handler does not veto",
            vec![before("exit 1", false)],
            Expect::Engine,
        ),
        (
            "a veto stops later handlers",
            vec![before(VETO, true), before("touch second_ran", true)],
            Expect::Cancelled,
        ),
    ];
    for (name, hooks, expect) in cases {
        let temp = tempfile::tempdir().expect("temp dir");
        let ran = compact_with(
            hooks,
            |seq| {
                format!(
                    r#"{{"compaction":{{"summary":"hook summary","first_kept_seq":{seq},"tokens_before":7}}}}"#
                )
            },
            temp.path(),
        )
        .await
        .expect(name);
        let got = match (&ran.result, ran.saved.as_slice()) {
            (Err(CoordinatorError::CompactionCancelled { .. }), []) => Expect::Cancelled,
            (Ok(ManualCompactionOutcome::Compacted { .. }), [saved]) if saved.from_hook => {
                assert_eq!(
                    (saved.summary.as_str(), saved.tokens_before),
                    ("hook summary", 7),
                    "{name}"
                );
                Expect::Custom
            }
            (Ok(ManualCompactionOutcome::Compacted { .. }), [saved]) if !saved.from_hook => {
                Expect::Engine
            }
            other => panic!("{name}: unexpected {other:?}"),
        };
        assert_eq!(got, expect, "{name}");
        assert!(
            !temp.path().join("second_ran").exists(),
            "{name}: chain continued after a veto"
        );
    }
}

#[tokio::test]
async fn summarizing_hook_context_and_prompt_reach_the_summary_request() {
    let temp = tempfile::tempdir().expect("temp dir");
    let ran = compact_with(
        vec![hook(
            Hook::CompactionSummarizing,
            r#"printf '{"context":["recalled fact one","recalled fact two"],"prompt":"HOOK BASE PROMPT"}'"#,
            true,
        )],
        |_| String::new(),
        temp.path(),
    )
    .await
    .expect("compaction runs");
    assert!(ran.result.is_ok(), "{:?}", ran.result);
    let request = ran
        .requests
        .iter()
        .find(|text| text.contains("HOOK BASE PROMPT"))
        .expect("the hook prompt reaches a summary request");
    let one = request
        .find("- recalled fact one")
        .expect("first context line");
    let two = request
        .find("- recalled fact two")
        .expect("second context line");
    assert!(one < two, "context keeps hook order");
}

#[tokio::test]
async fn after_hook_runs_once_the_entry_is_saved_and_cannot_undo_it() {
    let temp = tempfile::tempdir().expect("temp dir");
    let saw = r#"grep -q session_compaction "$HARNESS_HOOK_ARTIFACTS_DIR/../events.jsonl" && touch saw_entry; exit 1"#;
    let ran = compact_with(
        vec![hook(Hook::CompactionApplied, saw, true)],
        |_| String::new(),
        temp.path(),
    )
    .await
    .expect("compaction runs");
    assert!(
        matches!(ran.result, Ok(ManualCompactionOutcome::Compacted { .. })),
        "{:?}",
        ran.result
    );
    assert_eq!(ran.saved.len(), 1);
    assert!(temp.path().join("saw_entry").exists());
}
