use super::*;
use harness_core::event::RuntimeReminderKind;
use harness_providers::MessageRole;

#[path = "commands/fixture.rs"]
mod fixture;
use fixture::{assert_burst_receipts, CommandGate};

#[tokio::test]
async fn command_completion_notices_deliver_busy_idle_and_leftover_results_without_duplicate_observations(
) -> Result<(), Box<dyn std::error::Error>> {
    for mode in [
        "busy",
        "idle",
        "leftover",
        "disabled",
        "wake_off",
        "failed",
        "timed_out",
        "foreground",
        "polled",
        "waited",
        "killed",
        "shutdown",
        "wake_off_idle",
        "resume_unobserved",
        "resume_observed",
        "hook_failure",
        "burst",
        "native_pending",
        "native_terminal",
    ] {
        let temp = tempfile::tempdir()?;
        std::fs::write(temp.path().join("note"), "fixture")?;
        let listener = tokio::net::UnixListener::bind(temp.path().join("command.sock"))?;
        let provider = Arc::new(CommandGate {
            first: tokio::sync::Semaphore::new(0),
            requests: tokio::sync::Mutex::new(Vec::new()),
            continue_first: !matches!(
                mode,
                "idle"
                    | "leftover"
                    | "wake_off"
                    | "shutdown"
                    | "wake_off_idle"
                    | "resume_unobserved"
                    | "hook_failure"
            ),
            block_followup: mode == "shutdown",
            followup_started: tokio::sync::Notify::new(),
            child: tokio::sync::Semaphore::new(0),
        });
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::<CommandGate>::clone(&provider);
        config.tool_registry = Arc::new(harness_tools::coordinator_registry(
            ShellAllowlist::default(),
        ));
        config.permission_policy = PermissionPolicy::allow_all();
        config.secret_values.push("notice-secret-value".into());
        config.behavior.command_notifications.enabled = mode != "disabled";
        config.behavior.command_notifications.wake_idle =
            !matches!(mode, "wake_off" | "wake_off_idle" | "resume_unobserved");
        if mode == "hook_failure" {
            use harness_core::config::{HookLifecycleEvent, LifecycleHookConfig};
            config.hook_runtime_config.shell_allowlist.executables = vec!["/bin/sh".into()];
            config
                .hook_runtime_config
                .hooks
                .lifecycle
                .push(LifecycleHookConfig {
                    id: Some("fail-command-wake".into()),
                    event: HookLifecycleEvent::AgentTurnStarted,
                    command: vec!["/bin/sh".into(), "-c".into(), "test ! -f fail-wake".into()],
                    cwd: None,
                    timeout_ms: 1000,
                    critical: true,
                    env: Default::default(),
                });
        }
        let mut profile = AgentProfile::fallback("default");
        profile.toolset = vec![
            "bash".into(),
            "read".into(),
            "get_command_or_subagent_output".into(),
            "wait_commands_or_subagents".into(),
        ];
        let native = matches!(mode, "native_pending" | "native_terminal");
        if native {
            profile.toolset.push("spawn_subagent".into());
            config.provider_model_concurrency = 2;
        }
        config.agent_profiles.insert("default".into(), profile);
        let mut coordinator = spawn_coordinator(
            config.clone(),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator
            .start_run("command notices", temp.path())
            .await?;
        let agent = coordinator
            .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
            .await?;
        let mut actor = EventActor::new(ActorKind::Worker, Some(agent.clone()));
        let mut events = coordinator.subscribe_new_events().await?;
        let turn = coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                agent.clone(),
                "launch",
            )
            .await?;
        wait_for(&mut events, |event| matches!(event, EventV1::TaskScheduled(e) if e.task_id.as_str() == turn && e.state == harness_core::event::TaskScheduleState::Started), "turn did not start").await?;
        let mut child_id = String::new();
        if native {
            let output = coordinator.execute_agent_tool_call(actor.clone(), None, "spawn_subagent", json!({"prompt":"native work","description":"Native command owner","subagent_type":"task"})).await?;
            assert!(!output.is_error(), "{}", output.display_text);
            child_id = output
                .structured_json
                .ok_or("missing native spawn result")?["subagent_id"]
                .as_str()
                .ok_or("missing native id")?
                .to_owned();
            actor.agent_id = Some(child_id.clone());
            wait_for(&mut events, |event| matches!(event, EventV1::TaskScheduled(e) if e.queue_key.as_deref() == Some(child_id.as_str()) && e.state == harness_core::event::TaskScheduleState::Started), "native request did not start").await?;
        }
        let foreground = mode == "foreground";
        let short_command = foreground || mode == "failed";
        let command = if mode == "failed" {
            "printf terminal-marker; false".to_owned()
        } else if foreground {
            "printf terminal-marker".to_owned()
        } else {
            format!(
                "{} --exact notifications::commands::command_process_fixture --ignored --nocapture",
                shell_words::quote(&std::env::current_exe()?.to_string_lossy())
            )
        };
        let result = coordinator
            .execute_agent_tool_call(
                actor.clone(),
                None,
                "bash",
                json!({
                    "command":command, "description":"notice fixture", "run_in_background":true,
                    "block_until_ms":if foreground { 2000 } else { 0 },
                    "timeout_ms":if mode == "timed_out" { 1000 } else { 30_000 },
                }),
            )
            .await?;
        assert!(!result.is_error(), "{mode}: {}", result.display_text);
        let data = result.structured_json.ok_or("missing shell result")?;
        let id = data["task_id"]
            .as_str()
            .ok_or("missing command id")?
            .to_owned();
        let mut process = if short_command {
            None
        } else {
            Some(
                tokio::time::timeout(std::time::Duration::from_secs(3), listener.accept())
                    .await??
                    .0,
            )
        };
        let initially_idle = matches!(
            mode,
            "idle" | "wake_off_idle" | "resume_unobserved" | "hook_failure"
        );
        if initially_idle {
            provider.first.add_permits(1);
            wait_for(
                &mut events,
                |event| matches!(event, EventV1::TaskCompleted(e) if e.task_id.as_str() == turn),
                "idle turn did not finish",
            )
            .await?;
        }
        if mode == "native_terminal" {
            provider.child.add_permits(1);
            wait_for(&mut events, |event| matches!(event, EventV1::TaskCancelled(e) if e.task_scope == Some(harness_core::event::TaskTerminalScope::AgentTurn)), "native did not fail").await?;
        }
        if mode == "hook_failure" {
            std::fs::write(temp.path().join("fail-wake"), "fail")?;
        }
        if mode == "killed" {
            coordinator.kill_command(actor.clone(), id.clone()).await?;
        } else if !short_command && mode != "timed_out" {
            use tokio::io::AsyncWriteExt;
            process
                .as_mut()
                .ok_or("missing command connection")?
                .write_all(b"go")
                .await?;
        }
        if !foreground {
            wait_for(
                &mut events,
                |event| {
                    matches!(event, EventV1::TaskCompleted(e) if e.task_id.as_str() == id)
                        || matches!(event, EventV1::TaskCancelled(e) if e.task_id.as_str() == id)
                },
                "command did not finish",
            )
            .await?;
        }
        if native {
            if mode == "native_pending" {
                provider.child.add_permits(1);
                wait_for(&mut events, |event| matches!(event, EventV1::TaskCancelled(e) if e.task_scope == Some(harness_core::event::TaskTerminalScope::AgentTurn)), "native did not fail").await?;
            }
            provider.first.add_permits(1);
            let notice = wait_for(&mut events, |event| matches!(event, EventV1::RuntimeReminder(e) if e.kind == RuntimeReminderKind::CommandCompleted), "parent command notice missing").await?;
            assert_eq!(notice.actor.agent_id.as_deref(), Some(agent.as_str()));
            let EventV1::RuntimeReminder(notice) = notice.payload else {
                return Err("wrong native notice".into());
            };
            assert_eq!(notice.source.as_deref(), Some(id.as_str()));
            wait_for(
                &mut events,
                |event| matches!(event, EventV1::TaskCompleted(e) if e.task_id.as_str() == turn),
                "parent did not settle",
            )
            .await?;
            coordinator.stop_run().await?;
            let durable = harness_core::store::read_events(&run.events_path)?;
            assert_eq!(durable.iter().filter(|event| matches!(&event.payload, EventV1::TaskScheduled(e) if e.queue_key.as_deref() == Some(child_id.as_str()) && e.state == harness_core::event::TaskScheduleState::Started)).count(), 1, "terminal native children must not be reopened by command notices");
            let requests = provider.requests.lock().await;
            assert!(requests
                .last()
                .ok_or("missing parent followup")?
                .messages
                .iter()
                .any(|message| message.content == notice.text));
            continue;
        }
        if mode == "burst" {
            for _ in 0..100 {
                let result = coordinator
                    .execute_agent_tool_call(
                        actor.clone(),
                        None,
                        "bash",
                        json!({"command":"printf burst-marker", "run_in_background":true}),
                    )
                    .await?;
                let next = result.structured_json.ok_or("missing burst command")?["task_id"]
                    .as_str()
                    .ok_or("missing burst id")?
                    .to_owned();
                wait_for(&mut events, |event| matches!(event, EventV1::TaskCompleted(e) if e.task_id.as_str() == next), "burst command did not finish").await?;
            }
            assert!(
                coordinator
                    .subscribe_command(actor.clone(), id.clone())
                    .await?
                    .is_none(),
                "the held terminal snapshot must outlive cache eviction"
            );
            coordinator
                .observe_command_result(actor.clone(), id.clone())
                .await?;
            provider.first.add_permits(1);
            wait_for(
                &mut events,
                |event| matches!(event, EventV1::TaskCompleted(e) if e.task_id.as_str() == turn),
                "burst turn did not settle",
            )
            .await?;
            coordinator.stop_run().await?;
            let durable = harness_core::store::read_events(&run.events_path)?;
            assert_burst_receipts(&durable, &id)?;
            continue;
        }
        if matches!(mode, "polled" | "waited" | "resume_observed") {
            let tool = if mode != "waited" {
                "get_command_or_subagent_output"
            } else {
                "wait_commands_or_subagents"
            };
            let input = if mode == "waited" {
                json!({"task_ids":[id], "mode":"wait_all", "timeout_ms":2000})
            } else {
                json!({"task_ids":[id], "timeout_ms":0})
            };
            let output = coordinator
                .execute_agent_tool_call(actor, None, tool, input)
                .await?;
            assert!(!output.is_error(), "{mode}: {}", output.display_text);
        }
        let mut terminal_turn = turn.clone();
        if mode == "hook_failure" {
            wait_for(
                &mut events,
                |event| matches!(event, EventV1::TaskCancelled(e) if e.task_id.as_str() != id),
                "wake hook did not reject",
            )
            .await?;
            std::fs::remove_file(temp.path().join("fail-wake"))?;
        }
        if matches!(mode, "resume_unobserved" | "resume_observed") {
            tokio::time::timeout(std::time::Duration::from_secs(3), coordinator.stop_run())
                .await??;
            coordinator = spawn_coordinator(
                config.clone(),
                Arc::new(FakeClock::new()),
                Arc::new(DefaultRedactor::default()),
            );
            coordinator
                .resume_run(run.run_id.to_string(), "pending command notice")
                .await?;
            events = coordinator.subscribe_new_events().await?;
        }
        if matches!(
            mode,
            "wake_off_idle" | "resume_unobserved" | "resume_observed" | "hook_failure"
        ) {
            terminal_turn = coordinator
                .request_agent_turn(
                    EventActor::new(ActorKind::User, None),
                    agent.clone(),
                    "next explicit turn",
                )
                .await?;
        }
        if !initially_idle && mode != "resume_observed" {
            provider.first.add_permits(1);
        }
        let expected_notice = matches!(
            mode,
            "busy"
                | "idle"
                | "leftover"
                | "shutdown"
                | "wake_off"
                | "wake_off_idle"
                | "resume_unobserved"
                | "hook_failure"
                | "failed"
                | "timed_out"
        );
        let notice = if expected_notice {
            let event = wait_for(&mut events, |event| matches!(event, EventV1::RuntimeReminder(e) if e.kind == RuntimeReminderKind::CommandCompleted), "command notice missing").await?;
            let EventV1::RuntimeReminder(reminder) = event.payload else {
                return Err("wrong notice".into());
            };
            assert_eq!(reminder.source.as_deref(), Some(id.as_str()));
            if mode == "timed_out" {
                assert!(reminder.text.contains("Status: timed out"));
                assert!(!reminder.text.contains("Exit code:"));
            } else {
                assert!(reminder.text.contains("terminal-marker"));
            }
            if mode == "failed" {
                assert!(reminder.text.contains("Status: failed"));
                assert!(reminder.text.contains("Exit code: 1"));
            }
            assert!(
                reminder.text.len() < 5000,
                "output tail must remain bounded"
            );
            assert!(!reminder.text.contains("notice-secret-value"));
            assert!(reminder
                .text
                .contains(data["output_file"].as_str().ok_or("missing log")?));
            Some(reminder)
        } else {
            None
        };
        let final_turn = notice.as_ref().map_or(terminal_turn.as_str(), |reminder| {
            reminder.request_id.as_str()
        });
        if mode == "shutdown" {
            tokio::time::timeout(
                std::time::Duration::from_secs(3),
                provider.followup_started.notified(),
            )
            .await?;
        } else {
            wait_for(&mut events, |event| matches!(event, EventV1::TaskCompleted(e) if e.task_id.as_str() == final_turn), "final turn did not finish").await?;
        }
        tokio::time::timeout(std::time::Duration::from_secs(3), coordinator.stop_run()).await??;
        let durable = harness_core::store::read_events(&run.events_path)?;
        let reminders: Vec<_> = durable
            .iter()
            .filter_map(|event| match &event.payload {
                EventV1::RuntimeReminder(e) if e.kind == RuntimeReminderKind::CommandCompleted => {
                    Some(e)
                }
                _ => None,
            })
            .collect();
        assert_eq!(reminders.len(), usize::from(expected_notice), "{mode}");
        if mode == "hook_failure" {
            assert_eq!(
                durable
                    .iter()
                    .filter(|event| matches!(event.payload, EventV1::TaskCancelled(_)))
                    .count(),
                1,
                "an unchanged failed wake must not be retried"
            );
        }
        if let Some(notice) = notice {
            let requests = provider.requests.lock().await;
            assert_eq!(
                requests[1]
                    .messages
                    .last()
                    .ok_or("missing next request")?
                    .content,
                notice.text,
                "{mode}"
            );
            if mode == "idle" {
                assert_ne!(notice.request_id.as_str(), turn);
                assert!(!durable.iter().any(|event| matches!(&event.payload, EventV1::UserMessageSubmitted(e) if e.request_id == notice.request_id)));
            }
            if matches!(mode, "leftover" | "wake_off") {
                assert_eq!(
                    notice.request_id.as_str(),
                    turn,
                    "final-request completions must continue the original turn"
                );
            }
            drop(requests);
            if mode == "shutdown" {
                continue;
            }
            let restored = spawn_coordinator(
                config,
                Arc::new(FakeClock::new()),
                Arc::new(DefaultRedactor::default()),
            );
            restored
                .resume_run(run.run_id.to_string(), "notice history")
                .await?;
            let mut restored_events = restored.subscribe_new_events().await?;
            let history_turn = restored
                .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "history")
                .await?;
            wait_for(&mut restored_events, |event| matches!(event, EventV1::TaskCompleted(e) if e.task_id.as_str() == history_turn), "history turn missing").await?;
            let requests = provider.requests.lock().await;
            let messages = &requests.last().ok_or("missing rebuilt request")?.messages;
            assert_eq!(
                messages
                    .iter()
                    .filter(|message| message.role == MessageRole::User
                        && message.content == notice.text)
                    .count(),
                1
            );
            drop(requests);
            restored.stop_run().await?;
        }
    }
    Ok(())
}

/// Executed by the real bash tool as a child test binary, not by the ordinary suite.
#[test]
#[ignore = "subprocess fixture invoked by the command completion test"]
fn command_process_fixture() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;
    let mut socket = std::os::unix::net::UnixStream::connect("command.sock")?;
    let mut release = [0; 2];
    socket.read_exact(&mut release)?;
    println!("{}terminal-marker notice-secret-value", "🙂".repeat(11_000));
    Ok(())
}
