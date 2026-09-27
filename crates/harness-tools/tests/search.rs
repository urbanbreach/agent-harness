use harness_core::{
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor},
    perm::{PermissionAction, PermissionPolicy, PermissionRule},
    redact::DefaultRedactor,
};
use serde_json::json;
use std::{fs, sync::Arc};

#[tokio::test]
async fn workspace_search_preserves_modes_caps_and_path_permissions(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("workspace");
    fs::create_dir(&root)?;
    fs::write(root.join("a.rs"), "before\nneedle.one\nneedle.two\nafter\n")?;
    fs::write(root.join("b.rs"), "needleXone\n")?;
    fs::write(root.join("private.rs"), "needle secret\n")?;
    fs::write(root.join(".env"), "needle opaque-secret\n")?;
    fs::write(root.join("binary"), b"needle\0binary")?;
    fs::write(root.join(".gitignore"), "ignored.rs\n")?;
    fs::write(root.join("ignored.rs"), "needle ignored\n")?;
    let outside = temp.path().join("outside.txt");
    fs::write(&outside, "needle outside")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("link.rs"))?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "read".into(),
            pattern: "private.rs".into(),
            action: PermissionAction::Deny,
        },
        PermissionRule {
            permission: "external_directory".into(),
            pattern: "*".into(),
            action: PermissionAction::Deny,
        },
    ])?;
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("search", &root).await?;
    let actor = || EventActor::new(ActorKind::User, None);
    let glob = coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "glob",
            json!({"pattern":"**/*.rs", "limit":1}),
        )
        .await?;
    let glob = glob.structured_json.ok_or("missing glob metadata")?;
    assert_eq!(glob["paths"].as_array().ok_or("missing paths")?.len(), 1);
    assert_eq!(glob["truncated"], true);
    let matches = coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "grep",
            json!({"pattern":"needle.one", "literal":true, "context":1}),
        )
        .await?;
    assert!(matches.display_text.contains("before") && matches.display_text.contains("needle.two"));
    assert!(!matches.display_text.contains("b.rs") && !matches.display_text.contains("outside"));
    let count = coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "grep",
            json!({"pattern":"needle", "output_mode":"count", "include":"*.rs", "head_limit":1}),
        )
        .await?;
    let count = count.structured_json.ok_or("missing count metadata")?;
    assert_eq!(count["files"].as_array().ok_or("missing files")?.len(), 1);
    assert_eq!(count["files"][0]["count"], 2);
    let file = coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "grep",
            json!({"pattern":"needle", "path":"a.rs", "include":"*.rs", "output_mode":"count"}),
        )
        .await?;
    assert_eq!(
        file.structured_json.ok_or("missing file count")?["files"][0]["count"],
        2
    );
    let all = coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "grep",
            json!({"pattern":"needle", "output_mode":"files_with_matches"}),
        )
        .await?;
    for excluded in ["private.rs", ".env", "link.rs", "binary", "ignored.rs"] {
        assert!(!all.display_text.contains(excluded));
    }
    assert!(coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "grep",
            json!({"pattern":"needle", "path":"../outside.txt"})
        )
        .await
        .is_err());
    assert!(coordinator
        .execute_agent_tool_call(actor(), None, "grep", json!({"pattern":"["}))
        .await
        .is_err());
    coordinator.stop_run().await?;
    Ok(())
}
