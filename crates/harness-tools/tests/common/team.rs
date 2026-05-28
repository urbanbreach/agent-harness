use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::{ShellAllowlist, ToolFailureMode};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::redact::DefaultRedactor;
use harness_tools::coordinator_registry;

pub(crate) fn team_coordinator(
    session_dir: &Path,
    run_id: &str,
    profile_names: &[&str],
) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir);
    config.run_id_override = Some(run_id.to_string());
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = profile_names
        .iter()
        .map(|name| ((*name).to_string(), team_profile(name)))
        .collect::<BTreeMap<_, _>>();

    spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

fn team_profile(name: &str) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        category: "deep".to_string(),
        model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        system_prompt: format!("{name}-prompt"),
        max_iters: Some(1),
        temperature: Some(0.0),
        tool_failure_mode: ToolFailureMode::FailTurn,
        toolset: vec!["team_status".to_string()],
    }
}
