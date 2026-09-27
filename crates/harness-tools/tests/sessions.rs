use harness_core::{
    agent::AgentProfile,
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor},
    perm::{PermissionAction, PermissionPolicy, PermissionRule},
    redact::DefaultRedactor,
};
use serde_json::{json, Value};
use std::{fs, sync::Arc};

#[tokio::test]
async fn session_tools_project_bounded_redacted_history_without_changing_it(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let sessions = root.path().join("configured-sessions");
    let source = sessions.join("source");
    fs::create_dir_all(&source)?;
    let payloads = [
        json!({"event_type":"run_started","data":{"run_name":"Searchable session","workspace_root":root.path()}}),
        json!({"event_type":"agent_spawned","data":{"agent_id":"a","profile":"default","parent_agent_id":null}}),
        json!({"event_type":"user_message_submitted","data":{"request_id":"q","text":"Need café support with opaque-session-secret"}}),
        json!({"event_type":"tool_call_requested","data":{"tool_call_id":"todo","tool_id":"todowrite","args_summary":"RAW_ARGUMENTS","args_digest":"digest"}}),
        json!({"event_type":"tool_call_finished","data":{"tool_call_id":"todo","status":"succeeded","output_summary":format!("{}archive-tail-needle", "x".repeat(9000)),"output_json":{"todos":[{"content":"Inspect café","status":"pending","priority":"medium"}]}}}),
        json!({"event_type":"assistant_message_finished","data":{"request_id":"q","text_digest":"digest","text":"Café answer","parts":[{"kind":"text","text":"Café answer"},{"kind":"reasoning","text":"PRIVATE_REASONING"}],"tool_call_count":0}}),
        json!({"event_type":"run_finished","data":{"summary":"done"}}),
    ];
    let mut bytes = String::new();
    for (i, payload) in payloads.into_iter().enumerate() {
        bytes.push_str(&json!({"schema_version":1,"event_id":format!("e-{i}"),"seq":i+1,"run_id":"source","mono_ms":i,"actor":{"kind":"user"},"payload":payload}).to_string());
        bytes.push('\n');
    }
    fs::write(source.join("events.jsonl"), &bytes)?;
    let forbidden = sessions.join("forbidden");
    fs::create_dir(&forbidden)?;
    fs::write(
        forbidden.join("events.jsonl"),
        bytes
            .replace("\"source\"", "\"forbidden\"")
            .replace("Searchable session", "Forbidden session"),
    )?;
    let mut config = CoordinatorConfig::new(sessions);
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "session_read".into(),
            pattern: "forbidden".into(),
            action: PermissionAction::Deny,
        },
    ])?;
    config.secret_values = vec!["opaque-session-secret".into()];
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    let provider = Arc::new(harness_providers::mock::MockProvider::script([]));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    let mut profile = AgentProfile::fallback("default");
    profile.toolset = [
        "session_list",
        "session_read",
        "session_search",
        "session_info",
    ]
    .map(str::to_owned)
    .to_vec();
    config.agent_profiles.insert("default".into(), profile);
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    handle.start_run("inspection", root.path()).await?;
    let agent = handle
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(agent));
    for args in [
        json!({"session":"forbidden"}),
        json!({"path":"forbidden"}),
        json!({"session":forbidden}),
    ] {
        assert!(handle
            .execute_agent_tool_call(actor.clone(), None, "session_read", args)
            .await
            .is_err_and(|e| e.contains("permission denied")));
    }
    let list = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "session_list",
            json!({"filter":"Searchable","status":"finished"}),
        )
        .await?
        .structured_json
        .ok_or("missing list")?;
    assert_eq!(
        list["sessions"].as_array().ok_or("missing sessions")?.len(),
        1
    );
    assert_eq!(list["sessions"][0]["run_id"], "source");
    let read = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "session_read",
            json!({"session":"source","event_limit":2,"from_end":true,"include_todos":true}),
        )
        .await?
        .structured_json
        .ok_or("missing read")?;
    assert_eq!(read["returned_event_count"], 2);
    assert_eq!(read["events"][0]["seq"], 7);
    assert_eq!(read["events"][1]["seq"], 6);
    assert_eq!(read["messages"][0]["text"], "Café answer");
    assert_eq!(read["todos"][0]["content"], "Inspect café");
    assert!(read["truncated"].as_bool().unwrap_or_default());
    for query in [
        "café",
        "archive-tail-needle",
        "opaque-session-secret",
        "PRIVATE_REASONING",
        "RAW_ARGUMENTS",
    ] {
        let result = handle
            .execute_agent_tool_call(
                actor.clone(),
                None,
                "session_search",
                json!({"session":"source","query":query,"limit":1,"context_limit":5}),
            )
            .await?
            .structured_json
            .ok_or("missing search")?;
        if query == "café" {
            assert_eq!(result["returned_count"], 1);
            assert!(result["truncated"].as_bool().unwrap_or_default());
        } else if query == "archive-tail-needle" {
            assert_eq!(result["returned_count"], 1);
            assert!(result["matches"][0]["excerpt"]
                .as_str()
                .is_some_and(|s| s.contains(query)));
        } else {
            assert_eq!(result["returned_count"], 0);
        }
    }
    let info = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "session_info",
            json!({"session":"source"}),
        )
        .await?
        .structured_json
        .ok_or("missing info")?;
    assert_eq!(info["catalog"]["run_id"], "source");
    assert_eq!(info["event_counts"]["total_events"], 7);
    for args in [
        json!({"session":"../source"}),
        json!({"session":"missing"}),
        json!({"session":"source","unexpected":true}),
    ] {
        assert!(handle
            .execute_agent_tool_call(actor.clone(), None, "session_read", args)
            .await
            .is_err());
    }
    let all = handle
        .execute_agent_tool_call(
            actor,
            None,
            "session_read",
            json!({"session":"source","event_limit":1000}),
        )
        .await?;
    let value = all.structured_json.unwrap_or(Value::Null);
    assert_eq!(value["effective_event_limit"], 200);
    assert!(![
        "opaque-session-secret",
        "PRIVATE_REASONING",
        "RAW_ARGUMENTS"
    ]
    .iter()
    .any(|s| all.display_text.contains(s) || value.to_string().contains(s)));
    let large = source.parent().ok_or("missing session root")?.join("large");
    fs::create_dir(&large)?;
    fs::File::create(large.join("events.jsonl"))?.set_len(64 * 1024 * 1024 + 1)?;
    let system = EventActor::new(ActorKind::User, None);
    assert!(handle
        .execute_agent_tool_call(
            system.clone(),
            None,
            "session_info",
            json!({"session":"large"})
        )
        .await
        .is_err_and(|e| e.contains("64 MiB")));
    let broken = source
        .parent()
        .ok_or("missing session root")?
        .join("broken");
    fs::create_dir(&broken)?;
    fs::write(broken.join("events.jsonl"), "malformed journal\n")?;
    let listing = handle
        .execute_agent_tool_call(system.clone(), None, "session_list", json!({}))
        .await?
        .structured_json
        .ok_or("missing list")?;
    assert_eq!(
        listing["errors"]
            .as_array()
            .ok_or("missing scan failures")?
            .len(),
        2
    );
    #[cfg(unix)]
    {
        let link = source
            .parent()
            .ok_or("missing session root")?
            .join("linked");
        std::os::unix::fs::symlink(&source, &link)?;
        assert!(handle
            .execute_agent_tool_call(system, None, "session_read", json!({"session":"linked"}))
            .await
            .is_err());
    }
    handle.stop_run().await?;
    assert_eq!(provider.call_count(), 0);
    assert_eq!(fs::read_to_string(source.join("events.jsonl"))?, bytes);
    assert_eq!(fs::read_dir(&source)?.count(), 1);
    Ok(())
}
