use super::*;

#[tokio::test]
async fn directory_instructions_respect_shared_and_agent_read_policy_without_prompting(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::perm::{PermissionAction, PermissionRule};
    for role in [false, true] {
        for action in [PermissionAction::Deny, PermissionAction::Ask] {
            let temp = tempfile::tempdir()?;
            std::fs::create_dir(temp.path().join("sub"))?;
            std::fs::write(temp.path().join("sub/file"), "data")?;
            std::fs::write(temp.path().join("sub/AGENTS.md"), "FORBIDDEN_INSTRUCTIONS")?;
            let provider = Arc::new(MockProvider::script([
                touch("read", json!(["sub/file"])),
                crate::coord::compaction_tests::answer("done"),
            ]));
            let mut config = configuration(temp.path(), &provider);
            let policy = PermissionPolicy::from_rules(vec![
                PermissionRule {
                    permission: "*".into(),
                    pattern: "*".into(),
                    action: PermissionAction::Allow,
                },
                PermissionRule {
                    permission: "read".into(),
                    pattern: "sub/AGENTS.md".into(),
                    action,
                },
            ])?;
            if role {
                config
                    .agent_profiles
                    .get_mut("default")
                    .ok_or("missing profile")?
                    .permission_ruleset = vec![PermissionRule {
                    permission: "read".into(),
                    pattern: "sub/AGENTS.md".into(),
                    action,
                }];
            } else {
                config.permission_policy = policy;
            }
            let coordinator = spawn(config);
            let run = coordinator
                .start_run("instruction policy", temp.path())
                .await?;
            let agent = coordinator
                .spawn_agent_idle(
                    EventActor::new(ActorKind::Supervisor, None),
                    "default",
                    None,
                )
                .await?;
            turn(&coordinator, &agent).await?;
            assert!(recorded(&run.events_path)?.is_empty());
            assert!(!crate::store::read_events(&run.events_path)?
                .iter()
                .any(|e| matches!(e.payload, EventV1::PermissionRequested(_))));
            let requests = provider.captured_requests().await;
            assert_eq!(requests.len(), 2);
            assert!(!requests[1]
                .messages
                .iter()
                .any(|m| m.content.contains("FORBIDDEN_INSTRUCTIONS")));
            coordinator.stop_run().await?;
        }
    }
    Ok(())
}
