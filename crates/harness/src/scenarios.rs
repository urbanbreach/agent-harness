use clap::ValueEnum;
use harness_core::{
    agent::AgentProfile,
    event::{ActorKind, EventActor},
    perm::PermissionPolicy,
};
use harness_providers::mock::MockProvider;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum ScenarioName {
    GoldenPath,
    GoldenPathInteractive,
    QuestionInteractive,
}
impl ScenarioName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GoldenPath => "golden_path",
            Self::GoldenPathInteractive => "golden_path_interactive",
            Self::QuestionInteractive => "question_interactive",
        }
    }
    pub fn interactive_permissions(self) -> bool {
        self != Self::GoldenPath
    }
    pub fn is_question(self) -> bool {
        self == Self::QuestionInteractive
    }
}
pub fn deterministic_run_id(seed: u64, scenario: ScenarioName) -> String {
    let namespace = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("harness-seed:{seed}").as_bytes(),
    );
    format!(
        "run_{}",
        uuid::Uuid::new_v5(&namespace, scenario.as_str().as_bytes()).simple()
    )
}
pub fn create_workspace(
    session_dir: &Path,
    scenario: ScenarioName,
    run_id: Option<&str>,
) -> Result<PathBuf, String> {
    let id = run_id.map(str::to_owned).unwrap_or_else(|| {
        format!(
            "{}-{}-{}",
            scenario.as_str(),
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        )
    });
    harness_core::store::validate_session_id(&id).map_err(|e| e.to_string())?;
    // Demo workspaces sit beside session storage so tools cannot edit managed history.
    let workspace = session_dir.with_extension("workspaces").join(id);
    harness_core::store::create_private_dir(&workspace).map_err(|e| e.to_string())?;
    let demo = workspace.join("demo.txt");
    match std::fs::symlink_metadata(&demo) {
        Ok(metadata) if metadata.is_file() => {
            std::fs::remove_file(demo).map_err(|e| e.to_string())?
        }
        Ok(_) => return Err("demo.txt must be a regular generated file".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    Ok(workspace)
}
pub fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, None)
}
pub fn worker_actor(id: String) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(id))
}
pub fn default_permission_policy() -> PermissionPolicy {
    PermissionPolicy::default()
}
pub fn golden_path_profiles() -> BTreeMap<String, AgentProfile> {
    let mut profile = AgentProfile::fallback("default");
    profile.model_ref = "mock:model-1".into();
    profile.max_iters = Some(8);
    profile.toolset = [
        "read",
        "write",
        "edit",
        "list",
        "glob",
        "grep",
        "bash",
        "apply_patch",
        "question",
    ]
    .map(str::to_owned)
    .into();
    BTreeMap::from([("default".into(), profile)])
}
pub fn golden_path_provider() -> MockProvider {
    MockProvider::default()
}
pub fn golden_path_edit_args() -> Value {
    json!({"filePath":"demo.txt", "oldString":"", "newString":"Hello world\n"})
}
pub fn question_interactive_request_json() -> Value {
    json!({"questions":[{"header":"Choice", "question":"Pick an option", "options":[{"label":"A","description":"First option"},{"label":"B","description":"Second option"}], "multiple":false,"custom":true}]})
}
