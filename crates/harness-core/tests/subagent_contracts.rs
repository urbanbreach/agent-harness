use harness_core::subagent::{
    FinalizedStateFidelity, LifecycleEffect, LifecycleState, LifecycleTransition,
    SendSubagentMessageDelivery, SendSubagentMessageInput, SendSubagentMessageOutput,
    SpawnSubagentInput, SubagentAncestry, SubagentAttemptKey, SubagentCommandRequest, SubagentId,
    WaitCommandsOrSubagentsInput,
};
use serde_json::json;

#[test]
fn native_wire_contracts_preserve_omission_aliases_and_tagged_outcomes() -> serde_json::Result<()> {
    let schema = serde_json::to_value(schemars::schema_for!(SpawnSubagentInput))?;
    assert_eq!(schema["required"], json!(["prompt", "description"]));
    assert_eq!(schema["properties"]["background"]["default"], true);

    spawn_wire_contract()?;
    output_wire_contract()?;
    message_wire_contract()?;
    ancestry_and_command_contract()?;
    Ok(())
}

fn spawn_wire_contract() -> serde_json::Result<()> {
    let cases = [
        (None, true),
        (Some(json!(null)), false),
        (Some(json!(" YES ")), true),
        (Some(json!(0)), false),
    ];
    for (background, expected_background) in cases {
        let mut input = json!({"prompt": "", "description": ""});
        if let Some(background) = background {
            input["background"] = background;
        }
        let parsed = serde_json::from_value::<SpawnSubagentInput>(input)?;
        assert_eq!(parsed.background, expected_background);
        assert_eq!(parsed.subagent_type, "task");
        assert!(!parsed.subagent_type_specified);
    }

    let invalid_background = serde_json::from_value::<SpawnSubagentInput>(json!({
        "prompt": "p",
        "description": "d",
        "background": "on"
    }));
    assert!(invalid_background.is_err());

    let explicit = serde_json::from_value::<SpawnSubagentInput>(json!({
        "prompt": "p",
        "description": "d",
        "subagent_type": "task",
        "capability_mode": "read-only",
        "persona": "ignored",
        "workspace": "wire-only",
        "task_id": "injected"
    }))?;
    assert!(explicit.subagent_type_specified);
    assert_eq!(explicit.workspace.as_deref(), Some("wire-only"));
    let serialized = serde_json::to_value(explicit)?;
    assert_eq!(serialized["subagent_type"], "task");
    assert_eq!(serialized["task_id"], "injected");
    assert!(serialized.get("capability_mode").is_none());
    assert!(serialized.get("persona").is_none());

    let sentinel = serde_json::from_value::<SpawnSubagentInput>(json!({
        "prompt": "p",
        "description": "d",
        "subagent_type": " Undefined "
    }))?;
    assert!(!sentinel.subagent_type_specified);
    let value = serde_json::to_value(sentinel)?;
    assert!(value.get("subagent_type").is_none());
    Ok(())
}

fn output_wire_contract() -> serde_json::Result<()> {
    let ids_cases = [
        (json!({"task_id": "one"}), vec!["one"]),
        (json!({"task_ids": 17}), vec!["17"]),
        (json!({"task_ids": ["one", 2]}), vec!["one", "2"]),
        (json!({"task_ids": null}), vec![]),
    ];
    for (fields, expected) in ids_cases {
        let parsed = serde_json::from_value::<
            harness_core::subagent::GetCommandOrSubagentOutputInput,
        >(fields)?;
        assert_eq!(parsed.task_ids, expected);
    }
    assert!(
        serde_json::from_value::<harness_core::subagent::GetCommandOrSubagentOutputInput>(
            json!({"task_ids": [true]})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<WaitCommandsOrSubagentsInput>(json!({
            "task_ids": ["one"],
            "mode": "wait_any",
            "timeout_ms": null
        }))
        .is_ok()
    );
    Ok(())
}

fn message_wire_contract() -> serde_json::Result<()> {
    let message = serde_json::from_value::<SendSubagentMessageInput>(json!({
        "subagent_id": "child",
        "text": "hello",
        "queue": true
    }))?;
    assert_eq!(message.delivery(), SendSubagentMessageDelivery::Queue);
    let precedence = serde_json::from_value::<SendSubagentMessageInput>(json!({
        "subagent_id": "child",
        "text": "hello",
        "queue": true,
        "delivery": "interject"
    }))?;
    assert_eq!(
        precedence.delivery(),
        SendSubagentMessageDelivery::Interject
    );
    let message_outcomes = [
        json!({"outcome": "accepted", "message_id": "m1"}),
        json!({"outcome": "not_found_or_not_owned"}),
        json!({"outcome": "not_active_or_finalizing"}),
        json!({"outcome": "saturated", "max_in_flight": 2}),
        json!({"outcome": "quota_exceeded", "kind": "attempt_outbound", "limit": 4}),
        json!({"outcome": "admission_uncertain"}),
        json!({"outcome": "not_accepted_before_deadline"}),
        json!({"outcome": "unsupported"}),
        json!({"outcome": "limit", "max_bytes": 8, "observed_bytes": 9}),
        json!({"outcome": "channel_closed"}),
    ];
    for outcome in message_outcomes {
        assert!(serde_json::from_value::<SendSubagentMessageOutput>(outcome).is_ok());
    }
    let output = serde_json::to_value(SendSubagentMessageOutput::Accepted {
        message_id: "m1".to_owned(),
    })?;
    assert_eq!(output, json!({"outcome": "accepted", "message_id": "m1"}));
    Ok(())
}

fn ancestry_and_command_contract() -> serde_json::Result<()> {
    let ancestry = SubagentAncestry::new(None, Some(SubagentId("origin".to_owned())));
    let value = serde_json::to_value(ancestry)?;
    assert!(value["spawner"].is_null());
    assert_eq!(value["origin"], "origin");
    let fidelity = serde_json::to_value(FinalizedStateFidelity::Redacted);
    assert_eq!(fidelity.ok(), Some(json!("redacted")));

    let commands = [
        (
            SubagentCommandRequest::ExplicitChildKill {
                child_id: SubagentId("child".to_owned()),
            },
            "explicit_child_kill",
        ),
        (
            SubagentCommandRequest::ChildSessionCancel {
                session_id: "child-session".to_owned(),
                descendants: vec![SubagentId("grandchild".to_owned())],
            },
            "child_session_cancel",
        ),
        (
            SubagentCommandRequest::ParentPromptCancel {
                prompt_id: "prompt".to_owned(),
            },
            "parent_prompt_cancel",
        ),
        (
            SubagentCommandRequest::ParentSessionStop {
                session_id: "parent".to_owned(),
            },
            "parent_session_stop",
        ),
        (
            SubagentCommandRequest::WaiterCancel {
                waiter_id: "waiter".to_owned(),
            },
            "waiter_cancel",
        ),
        (SubagentCommandRequest::RootShutdown, "root_shutdown"),
    ];
    for (command, expected) in commands {
        let value = serde_json::to_value(command)?;
        assert_eq!(value["command"], expected);
    }
    Ok(())
}

#[test]
fn lifecycle_fold_rejects_stale_events_and_bounds_attempt_history() {
    fn apply(
        state: &mut LifecycleState,
        transition: LifecycleTransition,
        attempt: Option<&str>,
        sequence: Option<u64>,
    ) -> Option<LifecycleEffect> {
        state.reduce(transition, attempt, sequence).commit(state)
    }

    let mut state = LifecycleState::default();
    let cases = [
        (
            LifecycleTransition::Spawned,
            Some("attempt-a"),
            Some(1),
            Some(LifecycleEffect::Apply),
        ),
        (
            LifecycleTransition::Spawned,
            Some("attempt-a"),
            Some(2),
            None,
        ),
        (
            LifecycleTransition::Spawned,
            Some("attempt-b"),
            Some(3),
            Some(LifecycleEffect::ApplyNewAttempt),
        ),
        (
            LifecycleTransition::Progress,
            Some("attempt-b"),
            Some(3),
            None,
        ),
        (
            LifecycleTransition::Progress,
            Some("attempt-b"),
            Some(4),
            Some(LifecycleEffect::Apply),
        ),
        (
            LifecycleTransition::Progress,
            Some("attempt-b"),
            Some(4),
            None,
        ),
        (
            LifecycleTransition::Progress,
            Some("attempt-a"),
            Some(4),
            None,
        ),
        (
            LifecycleTransition::Finished,
            Some("attempt-a"),
            Some(2),
            Some(LifecycleEffect::RecordOnly),
        ),
        (
            LifecycleTransition::Finished,
            Some("attempt-b"),
            Some(5),
            Some(LifecycleEffect::Apply),
        ),
        (
            LifecycleTransition::Finished,
            Some("attempt-b"),
            Some(6),
            None,
        ),
        (
            LifecycleTransition::Spawned,
            Some("attempt-old"),
            Some(1),
            None,
        ),
    ];
    for (transition, attempt, sequence, expected) in cases {
        assert_eq!(
            apply(&mut state, transition, attempt, sequence),
            expected,
            "transition {transition:?} for {attempt:?}"
        );
    }
    assert_eq!(state.current_attempt_id(), Some("attempt-b"));
    assert!(state.is_finished());

    let mut legacy = LifecycleState::default();
    assert_eq!(
        apply(&mut legacy, LifecycleTransition::Spawned, None, Some(1)),
        Some(LifecycleEffect::Apply)
    );
    assert_eq!(
        apply(&mut legacy, LifecycleTransition::Finished, None, Some(2)),
        Some(LifecycleEffect::Apply)
    );
    assert_eq!(
        apply(&mut legacy, LifecycleTransition::Spawned, None, Some(3)),
        Some(LifecycleEffect::ApplyNewAttempt)
    );
    assert_eq!(
        legacy.current_attempt_key(),
        Some(&SubagentAttemptKey::Legacy)
    );

    let mut pending = LifecycleState::default();
    assert_eq!(
        apply(
            &mut pending,
            LifecycleTransition::Finished,
            Some("attempt-pending"),
            Some(1)
        ),
        Some(LifecycleEffect::AwaitSpawn)
    );
    assert_eq!(
        apply(
            &mut pending,
            LifecycleTransition::Spawned,
            Some("attempt-pending"),
            Some(2)
        ),
        Some(LifecycleEffect::ApplyWithPendingFinish)
    );
    assert!(pending.is_finished());

    let mut orphaned = LifecycleState::default();
    apply(
        &mut orphaned,
        LifecycleTransition::Finished,
        Some("attempt-orphan"),
        None,
    );
    orphaned.forget_unbacked_pending_attempts(|_| false);
    assert!(!orphaned.retains_attempt(&SubagentAttemptKey::Id("attempt-orphan".to_owned())));

    let mut bounded = LifecycleState::default();
    for index in 0..9 {
        let attempt = format!("attempt-{index}");
        assert_eq!(
            apply(
                &mut bounded,
                LifecycleTransition::Spawned,
                Some(&attempt),
                Some(index + 1),
            ),
            Some(if index == 0 {
                LifecycleEffect::Apply
            } else {
                LifecycleEffect::ApplyNewAttempt
            })
        );
    }
    assert!(!bounded.retains_attempt(&SubagentAttemptKey::Id("attempt-0".to_owned())));
    assert_eq!(bounded.current_attempt_id(), Some("attempt-8"));
}
