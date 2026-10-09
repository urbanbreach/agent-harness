use super::{compaction_tests::answer, tests::CountTool, *};
use crate::{
    clock::FakeClock,
    config::{HookLifecycleEvent as Hook, LifecycleHookConfig},
    redact::DefaultRedactor,
    tool::ToolRegistry,
};
use harness_providers::mock::MockProvider;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

fn hook(event: Hook, script: &str) -> LifecycleHookConfig {
    LifecycleHookConfig {
        id: Some(event.as_str().into()),
        event,
        command: vec!["/bin/sh".into(), "-c".into(), script.into()],
        cwd: None,
        timeout_ms: 1000,
        critical: true,
        env: BTreeMap::new(),
    }
}

#[tokio::test]
async fn hooks_run_in_order_with_context_and_redacted_completion_evidence(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(CountTool(Arc::clone(&calls))));
    let provider = Arc::new(MockProvider::script([answer("ready")]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.permission_policy = PermissionPolicy::allow_all();
    config.tool_registry = Arc::new(registry);
    config.provider = provider;
    config.hook_runtime_config.shell_allowlist.executables = vec!["/bin/sh".into()];
    let events = [
        Hook::RunStarted,
        Hook::AgentTurnStarted,
        Hook::ProviderRequestStarted,
        Hook::ProviderRequestFinished,
        Hook::AgentTurnFinished,
        Hook::ToolCallStarted,
        Hook::ToolCallFinished,
        Hook::RunFinished,
    ];
    config.hook_runtime_config.hooks.lifecycle = events.map(|event| {
        let mut hook = hook(event, "test -d \"$HARNESS_HOOK_ARTIFACTS_DIR\" || exit 9; printf '%s\\n' \"$HARNESS_HOOK_EVENT\" >> hooks.log; printf '%s' \"$HOOK_SECRET\"; printf '%s\\n' \"$HARNESS_HOOK_CONTEXT_JSON\" >> contexts.jsonl");
        hook.env.insert("HOOK_SECRET".into(), "credential-for-hook-redaction".into());
        hook
    }).into();
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("hooks", temp.path()).await?;
    assert_eq!(
        std::fs::read_to_string(temp.path().join("hooks.log"))?,
        "run_started\n"
    );
    let agent = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    let task = coordinator
        .request_agent_turn(handle::system(), agent.clone(), "start")
        .await?;
    super::history_tests::settled(&coordinator, &task).await?;
    coordinator
        .execute_agent_tool_call(handle::system(), None, "count", json!({}))
        .await?;
    coordinator.stop_run().await?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read_to_string(temp.path().join("hooks.log"))?,
        events.map(|e| format!("{}\n", e.as_str())).concat()
    );
    let contexts: Vec<serde_json::Value> =
        std::fs::read_to_string(temp.path().join("contexts.jsonl"))?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
    assert_eq!(contexts[2]["agent_id"], agent);
    assert!(contexts[2]["request_id"]
        .as_str()
        .is_some_and(|id| id.starts_with("provider-")));
    assert_eq!(contexts[5]["tool_id"], "count");
    let events = crate::store::read_events(&run.events_path)?;
    let tool = events
        .iter()
        .find_map(|e| match &e.payload {
            EventV1::ToolCallFinished(e) => e.metadata.as_ref(),
            _ => None,
        })
        .ok_or("missing tool evidence")?;
    assert_eq!(tool.hook_executions.len(), 2);
    assert!(tool
        .hook_executions
        .iter()
        .all(|h| h.status == HookExecutionStatus::Succeeded));
    assert!(!std::fs::read_to_string(&run.events_path)?.contains("credential-for-hook-redaction"));
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "resumed")
        .await?;
    resumed.stop_run().await?;
    let expected = events
        .iter()
        .filter(|e| matches!(e.payload, EventV1::RunStarted(_)))
        .count();
    assert_eq!(expected, 1);
    let log = std::fs::read_to_string(temp.path().join("hooks.log"))?;
    assert_eq!(log.lines().count(), contexts.len() + 2);
    assert!(log.ends_with("run_started\nrun_finished\n"));
    Ok(())
}

#[tokio::test]
async fn compaction_hooks_cover_request_failure_write_and_apply_without_replaying_history(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(MockProvider::script([
        answer("old ".repeat(2000)),
        answer("recent"),
        answer("bad summary"),
        answer(super::compaction_tests::SUMMARY),
    ]));
    config.compaction.keep_recent_tokens = 1;
    config.compaction.reserve_tokens = 0;
    config.compaction.suppress_auto_compaction = true;
    config.hook_runtime_config.shell_allowlist.executables = vec!["/bin/sh".into()];
    config.hook_runtime_config.hooks.lifecycle = [
        Hook::CompactionRequested,
        Hook::CompactionWritten,
        Hook::CompactionApplied,
        Hook::CompactionFailed,
    ]
    .map(|event| {
        hook(
            event,
            "printf '%s\\n' \"$HARNESS_HOOK_EVENT\" >> compaction.log",
        )
    })
    .into();
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("compaction hooks", temp.path())
        .await?;
    let agent = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    for text in ["old request", "recent request"] {
        let turn = coordinator
            .request_agent_turn(handle::system(), agent.clone(), text)
            .await?;
        super::history_tests::settled(&coordinator, &turn).await?;
    }
    assert!(coordinator
        .compact_agent_context(agent.clone(), None, "manual")
        .await
        .is_err());
    coordinator
        .compact_agent_context(agent, None, "manual")
        .await?;
    coordinator.stop_run().await?;
    assert_eq!(std::fs::read_to_string(temp.path().join("compaction.log"))?, "compaction_requested\ncompaction_failed\ncompaction_requested\ncompaction_written\ncompaction_applied\n");
    Ok(())
}

#[tokio::test]
async fn hook_policy_suppression_and_timeout_fail_closed_without_failing_noncritical_tools(
) -> Result<(), Box<dyn std::error::Error>> {
    for case in [
        "unlisted",
        "outside",
        "timeout",
        "failure",
        "suppressed",
        "large",
    ] {
        let temp = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        std::os::unix::fs::symlink(outside.path(), temp.path().join("outside"))?;
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(CountTool(Arc::clone(&calls))));
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.tool_registry = Arc::new(registry);
        config.permission_policy = PermissionPolicy::allow_all();
        let script = match case {
            "timeout" => "sleep 30 & wait",
            "failure" => "printf '%s' \"$HOOK_SECRET\"; exit 1",
            "large" => "head -c 1048576 /dev/zero | tr '\\000' x",
            _ => "touch unexpected",
        };
        let mut check = hook(Hook::ToolCallStarted, script);
        check.critical = false;
        if case == "timeout" {
            check.timeout_ms = 40;
        }
        if case == "outside" {
            check.cwd = Some("outside".into());
        }
        check
            .env
            .insert("HOOK_SECRET".into(), "credential-for-hook-redaction".into());
        config.hook_runtime_config.hooks.lifecycle = vec![check];
        config.hook_runtime_config.suppress_execution = case == "suppressed";
        if case != "unlisted" {
            config.hook_runtime_config.shell_allowlist.executables = vec!["/bin/sh".into()];
        }
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator.start_run(case, temp.path()).await?;
        coordinator
            .execute_agent_tool_call(handle::system(), None, "count", json!({}))
            .await?;
        coordinator.stop_run().await?;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!temp.path().join("unexpected").exists());
        assert!(!outside.path().join("unexpected").exists());
        let events = crate::store::read_events(&run.events_path)?;
        let receipt = events
            .iter()
            .find_map(|e| match &e.payload {
                EventV1::ToolCallFinished(e) => e.metadata.as_ref()?.hook_executions.first(),
                _ => None,
            })
            .ok_or("missing hook receipt")?;
        assert_eq!(
            receipt.status,
            match case {
                "unlisted" | "outside" => HookExecutionStatus::Blocked,
                "suppressed" => HookExecutionStatus::Skipped,
                "large" => HookExecutionStatus::Succeeded,
                _ => HookExecutionStatus::Failed,
            },
            "{case}"
        );
        assert!(receipt
            .output_summary
            .as_ref()
            .is_some_and(|s| s.len() <= 1024));
        assert!(
            !std::fs::read_to_string(&run.events_path)?.contains("credential-for-hook-redaction")
        );
    }
    Ok(())
}

#[tokio::test]
async fn critical_hook_vetoes_the_stage_without_stranding_tasks_or_recording_a_grant(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::perm::{PermissionAction, PermissionGrantScope, PermissionRule};
    for stage in [
        Hook::RunStarted,
        Hook::AgentTurnStarted,
        Hook::ProviderRequestStarted,
        Hook::ProviderRequestFinished,
        Hook::AgentTurnFinished,
        Hook::ToolCallStarted,
        Hook::ToolCallFinished,
        Hook::PermissionRequested,
        Hook::PermissionResolved,
        Hook::SubagentSpawned,
        Hook::SubagentFinished,
        Hook::RunFinished,
        Hook::RunFailed,
    ] {
        let temp = tempfile::tempdir()?;
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(CountTool(Arc::clone(&calls))));
        let provider = Arc::new(MockProvider::default());
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.run_id_override = Some("hooks".into());
        config.tool_registry = Arc::new(registry);
        config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
        config.permission_policy = PermissionPolicy::allow_all();
        if matches!(stage, Hook::PermissionRequested | Hook::PermissionResolved) {
            config.permission_policy = PermissionPolicy::from_rules(vec![PermissionRule {
                permission: "*".into(),
                pattern: "*".into(),
                action: PermissionAction::Ask,
            }])?;
        }
        config.hook_runtime_config.shell_allowlist.executables = vec!["/bin/sh".into()];
        config.hook_runtime_config.hooks.lifecycle = vec![hook(stage, "exit 7")];
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let started = coordinator.start_run("hooks", temp.path()).await;
        if stage == Hook::RunStarted {
            assert!(started.is_err());
            assert!(coordinator.run_info().await.is_err());
            let events =
                crate::store::read_events(&temp.path().join("sessions/hooks/events.jsonl"))?;
            assert!(matches!(
                events.last().map(|e| &e.payload),
                Some(EventV1::RunFailed(_))
            ));
            continue;
        }
        let run = started?;
        let parent = coordinator
            .spawn_agent_idle(handle::system(), "default", None)
            .await?;
        if stage == Hook::SubagentSpawned {
            assert!(coordinator
                .spawn_agent_idle(handle::system(), "default", Some(parent))
                .await
                .is_err());
        } else if matches!(
            stage,
            Hook::ToolCallStarted
                | Hook::ToolCallFinished
                | Hook::PermissionRequested
                | Hook::PermissionResolved
        ) {
            let worker = coordinator.clone();
            let mut events = coordinator.subscribe_new_events().await?;
            let tool = tokio::spawn(async move {
                worker
                    .execute_agent_tool_call(handle::system(), None, "count", json!({}))
                    .await
            });
            if stage == Hook::PermissionResolved {
                let permission = next_permission(&mut events).await?;
                coordinator
                    .resolve_permission_with_grant_scope(
                        permission,
                        PermissionDecision::Allow,
                        None,
                        Some(PermissionGrantScope::Workspace),
                    )
                    .await?;
            }
            assert!(
                tokio::time::timeout(std::time::Duration::from_secs(2), tool)
                    .await??
                    .is_err(),
                "{stage:?}"
            );
            assert_eq!(
                calls.load(Ordering::SeqCst),
                usize::from(stage == Hook::ToolCallFinished),
                "{stage:?}"
            );
        } else if !matches!(stage, Hook::RunFinished | Hook::RunFailed) {
            let agent = if stage == Hook::SubagentFinished {
                coordinator
                    .spawn_agent_idle(handle::system(), "default", Some(parent))
                    .await?
            } else {
                parent
            };
            let task = coordinator
                .request_agent_turn(handle::system(), agent, "check")
                .await?;
            assert!(
                super::history_tests::settled(&coordinator, &task)
                    .await
                    .is_err(),
                "{stage:?}"
            );
            assert_eq!(
                provider.call_count(),
                usize::from(!matches!(
                    stage,
                    Hook::AgentTurnStarted | Hook::ProviderRequestStarted
                )),
                "{stage:?}"
            );
        }
        let stop = if stage == Hook::RunFailed {
            coordinator.fail_run("requested failure").await
        } else {
            coordinator.stop_run().await
        };
        assert_eq!(
            stop.is_err(),
            matches!(stage, Hook::RunFinished | Hook::RunFailed),
            "{stage:?}: {stop:?}"
        );
        let events = crate::store::read_events(&run.events_path)?;
        if stage == Hook::ProviderRequestFinished {
            assert!(events
                .iter()
                .any(|e| matches!(e.payload, EventV1::ProviderRequestFinished(_))));
        }
        assert!(!events
            .iter()
            .any(|e| matches!(e.payload, EventV1::PermissionGrantRecorded(_))));
        assert!(!temp.path().join(".harness/permission-grants.json").exists());
        assert_eq!(
            matches!(
                events.last().map(|e| &e.payload),
                Some(EventV1::RunFailed(_))
            ),
            matches!(stage, Hook::RunFinished | Hook::RunFailed)
        );
    }
    Ok(())
}

async fn next_permission(
    events: &mut crate::store::EventStream,
) -> Result<String, Box<dyn std::error::Error>> {
    use tokio_stream::StreamExt;
    Ok(
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while let Some(event) = events.next().await {
                if let EventV1::PermissionRequested(e) = event?.payload {
                    return Ok::<_, EventStoreError>(e.permission_id);
                }
            }
            Err(EventStoreError::Invalid("missing permission"))
        })
        .await??,
    )
}
