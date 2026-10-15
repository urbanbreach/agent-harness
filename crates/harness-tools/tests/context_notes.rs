use harness_core::{
    agent::AgentProfile,
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle},
    event::{ActorKind, EventActor, EventV1},
    perm::{PermissionAction, PermissionPolicy, PermissionRule},
    redact::DefaultRedactor,
};
use serde_json::{json, Value};
use std::sync::Arc;

struct Session {
    _root: tempfile::TempDir,
    handle: CoordinatorHandle,
    actor: EventActor,
    read_only: EventActor,
}

async fn session(enabled: bool) -> Result<Session, Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.permission_policy = PermissionPolicy::allow_all();
    config.compaction.notes_windows = enabled;
    let mut profile = AgentProfile::fallback("default");
    profile.toolset = ["context_notes", "new_context", "read", "grep"]
        .map(String::from)
        .to_vec();
    let mut read_only = profile.clone();
    read_only.name = "read_only".into();
    read_only.permission_ruleset = vec![PermissionRule {
        permission: "edit".into(),
        pattern: "*".into(),
        action: PermissionAction::Deny,
    }];
    config.agent_profiles.insert("read_only".into(), read_only);
    config.agent_profiles.insert("default".into(), profile);
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    handle.start_run("notes", root.path()).await?;
    let user = EventActor::new(ActorKind::User, None);
    let agent = handle
        .spawn_agent_idle(user.clone(), "default", None)
        .await?;
    let limited = handle.spawn_agent_idle(user, "read_only", None).await?;
    Ok(Session {
        _root: root,
        handle,
        actor: EventActor::new(ActorKind::Worker, Some(agent)),
        read_only: EventActor::new(ActorKind::Worker, Some(limited)),
    })
}

impl Session {
    async fn run(&self, tool: &str, args: Value) -> Result<(String, Value), String> {
        let result = self
            .handle
            .execute_agent_tool_call(self.actor.clone(), None, tool, args)
            .await?;
        Ok((
            result.display_text,
            result.structured_json.unwrap_or(Value::Null),
        ))
    }

    async fn notes_events(&self) -> Result<usize, Box<dyn std::error::Error>> {
        let path = self.handle.run_info().await?.events_path;
        Ok(harness_core::store::read_events(&path)?
            .iter()
            .filter(|event| matches!(event.payload, EventV1::ContextNotes(_)))
            .count())
    }
}

#[tokio::test]
async fn notebook_revisions_follow_the_documented_read_replace_and_clear_contract(
) -> Result<(), Box<dyn std::error::Error>> {
    let s = session(true).await?;
    let (text, details) = s.run("context_notes", json!({})).await?;
    assert_eq!(text, "No context notes are stored for this session branch.");
    assert_eq!(details, json!({"text": ""}));

    let (text, saved) = s
        .run("context_notes", json!({"text": "Retain this notebook."}))
        .await?;
    assert_eq!(text, "Context notes saved.");
    assert_eq!(saved["text"], "Retain this notebook.");
    assert_eq!(saved["bytes"], 21);
    let (text, read) = s.run("context_notes", json!({})).await?;
    assert_eq!(text, "Retain this notebook.");
    assert_eq!(read["entry_id"], saved["entry_id"]);
    assert!(read.get("bytes").is_none());

    // 5,462 euro signs are 16,386 bytes: over the limit, and nothing changes.
    let error = s
        .run("context_notes", json!({"text": "€".repeat(5_462)}))
        .await
        .err()
        .ok_or("oversized notes were accepted")?;
    assert!(error.contains(
        "Context notes are 16386 bytes; the limit is 16384 UTF-8 bytes. Shorten the notebook and use history://current/full to recover raw detail."
    ));
    assert_eq!(s.notes_events().await?, 1);
    assert_eq!(
        s.run("context_notes", json!({})).await?.0,
        "Retain this notebook."
    );

    // The limit itself is inclusive.
    s.run("context_notes", json!({"text": "x".repeat(16_384)}))
        .await?;

    // An empty revision clears the notebook, and reads back as empty text rather than the
    // missing-notebook sentence.
    s.run("context_notes", json!({"text": ""})).await?;
    let (text, cleared) = s.run("context_notes", json!({})).await?;
    assert_eq!(text, "");
    assert!(cleared["entry_id"].is_string());
    Ok(())
}

#[tokio::test]
async fn invalid_notes_arguments_and_ineligible_callers_are_rejected_with_the_exact_errors(
) -> Result<(), Box<dyn std::error::Error>> {
    let s = session(true).await?;
    let error = s
        .run("context_notes", json!({"text": 7}))
        .await
        .err()
        .ok_or("a non-string text was accepted")?;
    assert!(error.contains("context_notes text must be a string."));
    assert!(s.run("context_notes", json!({"other": 1})).await.is_err());
    assert!(s.run("new_context", json!({"focus": "x"})).await.is_err());
    assert_eq!(s.notes_events().await?, 0);

    // Writes need write approval; reads do not.
    let read_only = |args: Value, tool: &'static str| {
        let (handle, actor) = (s.handle.clone(), s.read_only.clone());
        async move {
            handle
                .execute_agent_tool_call(actor, None, tool, args)
                .await
        }
    };
    assert!(read_only(json!({}), "context_notes").await.is_ok());
    for (tool, args) in [
        ("context_notes", json!({"text": "x"})),
        ("new_context", json!({})),
    ] {
        let error = read_only(args, tool)
            .await
            .err()
            .ok_or("write was approved")?;
        assert!(error.contains("permission denied"), "{tool}: {error}");
    }

    // A caller that is not an agent of this session cannot use anyone's notebook.
    let stranger = EventActor::new(ActorKind::Worker, None);
    for tool in ["context_notes", "new_context"] {
        let error = s
            .handle
            .execute_agent_tool_call(stranger.clone(), None, tool, json!({}))
            .await
            .err()
            .ok_or("an unowned caller was accepted")?;
        assert!(
            error.contains("Experimental context management is unavailable for this session."),
            "{tool}: {error}"
        );
    }

    let off = session(false).await?;
    for (tool, args) in [("context_notes", json!({})), ("new_context", json!({}))] {
        let error = off
            .run(tool, args)
            .await
            .err()
            .ok_or("a disabled session answered")?;
        assert!(
            error.contains("Experimental context management is disabled."),
            "{tool}: {error}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn new_context_acknowledges_without_touching_the_journal(
) -> Result<(), Box<dyn std::error::Error>> {
    let s = session(true).await?;
    let before = harness_core::store::read_events(&s.handle.run_info().await?.events_path)?.len();
    let (text, details) = s.run("new_context", json!({})).await?;
    assert_eq!(text, "New context window requested.");
    assert_eq!(details, json!({"requested": true}));
    assert_eq!(s.notes_events().await?, 0);
    // No marker, summary or rewind is recorded, and nothing is rolled over here.
    let events = harness_core::store::read_events(&s.handle.run_info().await?.events_path)?;
    assert!(!events[before..].iter().any(|event| matches!(
        event.payload,
        EventV1::SessionCompaction(_)
            | EventV1::ContextNotes(_)
            | EventV1::BranchSummary(_)
            | EventV1::ConversationRewound(_)
    )));
    Ok(())
}

#[tokio::test]
async fn the_history_route_serves_the_callers_branch_and_rejects_other_routes(
) -> Result<(), Box<dyn std::error::Error>> {
    let s = session(true).await?;
    s.run("context_notes", json!({"text": "Durable detail"}))
        .await?;

    let (text, details) = s
        .run("read", json!({"filePath": "history://current/full"}))
        .await?;
    assert!(text.starts_with("# Current branch — full history\n"));
    assert!(text.contains("## Entry "));
    assert!(text.contains("Durable detail"));
    assert_eq!(details["mime"], "text/markdown");

    // A window of the same rendering, by the read tool's own selectors.
    let (window, _) = s
        .run(
            "read",
            json!({"filePath": "history://current/full", "offset": 3, "limit": 1}),
        )
        .await?;
    assert_eq!(window.lines().next(), text.lines().nth(2));

    let (found, summary) = s
        .run(
            "grep",
            json!({"pattern": "Durable detail", "path": "history://current/full", "literal": true}),
        )
        .await?;
    assert!(found.contains("history://current/full:"));
    assert!(found.contains("Durable detail"));
    assert_eq!(summary["files"][0]["path"], "history://current/full");

    for path in [
        "history://current/other",
        "history://current/full?x=1",
        "history://current/full#a",
        "history://other/full",
    ] {
        let error = s
            .run("read", json!({"filePath": path}))
            .await
            .err()
            .ok_or("an invalid route was served")?;
        assert!(
            error.contains("Invalid history://current route; use exactly history://current/full (selectors may follow it)"),
            "{path}: {error}"
        );
    }

    let stranger = EventActor::new(ActorKind::Worker, None);
    let error = s
        .handle
        .execute_agent_tool_call(
            stranger,
            None,
            "read",
            json!({"filePath": "history://current/full"}),
        )
        .await
        .err()
        .ok_or("an unbound caller read the route")?;
    assert!(error.contains("history://current/full requires a bound live session branch"));

    let off = session(false).await?;
    let error = off
        .run("read", json!({"filePath": "history://current/full"}))
        .await
        .err()
        .ok_or("a disabled session served the route")?;
    assert!(error.contains("history://current/full is available only when"));
    Ok(())
}
