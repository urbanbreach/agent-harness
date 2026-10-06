use super::*;
use harness_core::config::{
    PermissionMode, SkillCatalog, SkillCatalogDiscovery, SkillsConfig, SubagentDefinition,
    SubagentDefinitionSnapshot, SubagentDiscoveryContext,
};
use harness_providers::ProviderStreamEvent as Stream;
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

#[path = "skill_startup/catalog_matrix.rs"]
mod catalog_matrix;
#[path = "skill_startup/effective_spawner.rs"]
mod effective_spawner;
#[path = "skill_startup/permissions.rs"]
mod permissions;

#[derive(Default)]
struct CountedDiscovery(AtomicUsize);

impl SkillCatalogDiscovery for CountedDiscovery {
    fn discover(
        &self,
        cwd: &Path,
        config: &SkillsConfig,
        project_trusted: Option<bool>,
    ) -> Result<SkillCatalog, harness_core::tool::ToolError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        harness_tools::NativeSkillCatalogDiscovery.discover(cwd, config, project_trusted)
    }
}

struct SkillCapture {
    inner: MockProvider,
    requests: tokio::sync::mpsc::UnboundedSender<CompletionRequest>,
    release: tokio::sync::Semaphore,
}

#[async_trait::async_trait]
impl Provider for SkillCapture {
    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        let _ = self.requests.send(request.clone());
        if let Ok(permit) = self.release.acquire().await {
            permit.forget();
        }
        self.inner.stream_completion(request).await
    }
}

fn write_skill(path: &Path, name: &str, body: &str) -> std::io::Result<()> {
    fs::create_dir_all(path)?;
    fs::write(
        path.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {name} metadata\n---\n{body}\n"),
    )
}

fn settled_completion() -> Stream {
    Stream::DoneWithMetadata {
        usage: Some(harness_providers::CompletionUsage {
            prompt_tokens: 10,
            completion_tokens: 2,
            total_tokens: 12,
        }),
        metadata: Some(harness_providers::ProviderStreamFinishedMetadata {
            usage_complete: Some(true),
            settled_reasoning: Some(vec![]),
            ..Default::default()
        }),
    }
}

fn available_skills(request: &CompletionRequest) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let metadata = request
        .messages
        .iter()
        .filter_map(|message| serde_json::from_str::<Value>(&message.content).ok())
        .find_map(|value| {
            value
                .get("available_skills")
                .and_then(Value::as_array)
                .cloned()
        })
        .ok_or("missing typed skill startup metadata")?;
    Ok(metadata)
}

#[tokio::test]
async fn native_skill_startup_shares_body_free_catalog_with_authorized_tool_loading(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let project = temp.path().join("project");
    fs::create_dir_all(&project)?;
    for args in [
        vec!["init", "--quiet"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "isolated skill fixture",
        ],
    ] {
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new("git")
                .current_dir(&project)
                .args(args)
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        assert!(output.status.success(), "fixture git: {:?}", output.stderr);
    }
    let skill = project.join("parent-only-skills/inherited");
    write_skill(&skill, "inherited", "BEFORE_INVOCATION_BODY")?;
    let skills = SkillsConfig {
        project_roots: vec!["parent-only-skills".into()],
        global_roots: vec![],
        walk_to_git_root: false,
        ..Default::default()
    };
    let (requests, mut captured) = tokio::sync::mpsc::unbounded_channel();
    let provider = Arc::new(SkillCapture {
        inner: MockProvider::script([
            vec![
                Stream::ToolCallComplete {
                    tool_call_id: "load-inherited".into(),
                    function_name: harness_providers::tool_function_name("skill"),
                    arguments_json: json!({"name":"inherited"}).to_string(),
                },
                settled_completion(),
            ],
            vec![Stream::TextDelta("loaded".into()), settled_completion()],
        ]),
        requests,
        release: tokio::sync::Semaphore::new(0),
    });
    let mut definitions = SubagentDefinitionSnapshot::default();
    definitions.cli.insert(
        "skill-reader".into(),
        SubagentDefinition {
            name: "skill-reader".into(),
            description: "Skill reader".into(),
            tools: vec!["Skill".into()],
            inject_default_tools: false,
            ..Default::default()
        },
    );
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.skills = skills.clone();
    let discovery = Arc::new(CountedDiscovery::default());
    config.skill_catalog_discovery = Some(Arc::<CountedDiscovery>::clone(&discovery));
    let mut registry =
        harness_tools::coordinator_registry_with_skills(ShellAllowlist::default(), skills);
    harness_tools::register_subagent_tools(&mut registry, &config.subagents, &definitions, None);
    let mut parent = AgentProfile::fallback("default");
    parent.toolset = registry.tool_ids();
    config.agent_profiles.insert("default".into(), parent);
    config.tool_registry = Arc::new(registry);
    config.subagent_definitions = Some(definitions);
    config.permission_policy = PermissionPolicy::allow_all();
    config.provider = Arc::<SkillCapture>::clone(&provider);
    let restore_config = config.clone();
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("skill startup", &project).await?;
    let parent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(parent));
    let started = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "spawn_subagent",
            json!({"prompt":"load inherited","description":"Skill child","subagent_type":"skill-reader","background":true,"isolation":"worktree"}),
        )
        .await?;
    let child = started.structured_json.ok_or("spawn result")?["subagent_id"]
        .as_str()
        .ok_or("child ID")?
        .to_owned();
    let initial = tokio::time::timeout(Duration::from_secs(5), captured.recv())
        .await?
        .ok_or("initial provider request")?;
    let history = coordinator.subagent_history().await?;
    let execution = history
        .records
        .get(&child)
        .ok_or("child history")?
        .metadata
        .as_ref()
        .map(|metadata| &metadata.context)
        .ok_or("child execution")?;
    assert_ne!(Path::new(&execution.effective_cwd), project.canonicalize()?);
    let system = &initial
        .messages
        .first()
        .ok_or("child system prompt")?
        .content;
    assert!(system.contains(&format!("{}\n", execution.effective_cwd)));
    assert!(!system.contains(&format!("{}\n", project.display())));
    assert!(matches!(
        execution.isolation,
        harness_core::subagent::ResolvedSubagentIsolation::Worktree { .. }
    ));
    let metadata = available_skills(&initial)?;
    let entry = metadata
        .iter()
        .find(|entry| entry["stable_id"] == "skill:project:inherited")
        .ok_or("inherited parent-only metadata")?;
    assert_eq!(entry["description"], "inherited metadata");
    assert_eq!(entry["source_scope"], "project");
    assert_eq!(entry["body_loaded"], false);
    let initial_json = serde_json::to_string(&initial)?;
    assert!(!initial_json.contains("BEFORE_INVOCATION_BODY"));
    assert!(!initial_json.contains(&skill.to_string_lossy().to_string()));
    assert!(initial
        .tools
        .as_ref()
        .is_some_and(|tools| tools.iter().any(|tool| tool.tool_id == "skill")));
    write_skill(&skill, "inherited", "AFTER_INVOCATION_BODY")?;
    provider.release.add_permits(1);
    let after_tool = tokio::time::timeout(Duration::from_secs(5), captured.recv())
        .await?
        .ok_or("post-tool provider request")?;
    let tool_text = after_tool
        .messages
        .iter()
        .filter(|message| message.role == harness_providers::MessageRole::Tool)
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(tool_text.contains("AFTER_INVOCATION_BODY"));
    assert!(!tool_text.contains("BEFORE_INVOCATION_BODY"));
    provider.release.add_permits(1);
    coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "get_command_or_subagent_output",
            json!({"task_ids":[child],"timeout_ms":5000}),
        )
        .await?;
    let run = coordinator.run_info().await?;
    assert!(
        matches!(coordinator.raw_finalized_state(child.clone()).await?, harness_core::subagent::FinalizedStateResult::Available { state } if state.skill_startup.is_some())
    );
    coordinator.stop_run().await?;
    let events = fs::read_to_string(run.events_path)?;
    assert!(!events.contains("BEFORE_INVOCATION_BODY"));
    assert!(!events.contains(&skill.to_string_lossy().to_string()));
    assert_eq!(discovery.0.load(Ordering::SeqCst), 1);
    fs::remove_file(skill.join("SKILL.md"))?;
    let mut denied_config = restore_config.clone();
    denied_config
        .skills
        .permissions
        .insert("inherited".into(), PermissionMode::Deny);
    let restored = spawn_coordinator(
        restore_config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    restored
        .resume_run(run.run_id.to_string(), "restored skill observation")
        .await?;
    let history = restored.subagent_history().await?;
    assert!(history.records.contains_key(&child));
    assert_eq!(
        discovery.0.load(Ordering::SeqCst),
        1,
        "restore rediscovered skills"
    );
    write_skill(&skill, "inherited", "RESTORED_LOAD_BODY")?;
    let restored_load = restored
        .execute_agent_tool_call(
            EventActor::new(ActorKind::Worker, Some(child.clone())),
            None,
            "skill",
            json!({"name":"inherited"}),
        )
        .await?;
    assert!(restored_load.display_text.contains("RESTORED_LOAD_BODY"));
    assert_eq!(discovery.0.load(Ordering::SeqCst), 1);
    restored.stop_run().await?;
    let denied = spawn_coordinator(
        denied_config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    denied
        .resume_run(run.run_id.to_string(), "changed skill policy")
        .await?;
    let error = denied
        .execute_agent_tool_call(
            EventActor::new(ActorKind::Worker, Some(child)),
            None,
            "skill",
            json!({"name":"inherited"}),
        )
        .await
        .err()
        .ok_or("restored catalog bypassed current policy")?;
    assert!(error.to_string().contains("permission denied"));
    assert_eq!(discovery.0.load(Ordering::SeqCst), 1);
    denied.stop_run().await?;
    Ok(())
}
