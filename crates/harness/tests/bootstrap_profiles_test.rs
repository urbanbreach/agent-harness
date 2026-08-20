use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use harness::UnwrapOrAbort;
use harness_core::agent::{build_provider_tool_defs, AgentProfile};
use harness_core::config::PermissionMode;
use harness_core::config::{
    load_config_from_file_with_context, load_config_from_str, ConfigLoadContext, HarnessConfig,
    ResolvedModelTarget,
};
use harness_core::perm::{
    is_tool_disabled, permission_kind_for_tool_call, PermissionKind, PermissionPolicy,
    PermissionRuleRequest, PolicyDecision,
};
use harness_core::tool::ToolRegistry;
use harness_core::workspace::WorkspaceEnvironment;
use tempfile::tempdir;

const GENERIC_PROMPT_PROFILES: &str = "default explore general librarian";
const GENERIC_COMPOSED_PROMPTS: [&str; 4] = ["default", "explore", "general", "librarian"];
const UPDATE_PROMPT_SNAPSHOTS_ENV: &str = "HARNESS_UPDATE_PROMPT_SNAPSHOTS";

#[path = "../src/dynamic_prompt.rs"]
mod dynamic_prompt;

#[path = "../src/bootstrap.rs"]
mod bootstrap;

#[path = "../src/cli_config.rs"]
mod cli_config;

fn write_agent_markdown(repo_root: &Path, name: &str, body: &str) {
    let path = repo_root
        .join(".agent-harness")
        .join("agents")
        .join(format!("{name}.md"));
    fs::create_dir_all(path.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(path, body).unwrap_or_abort();
}

fn load_config_from_repo_file(config_path: &Path, repo: &Path) -> HarnessConfig {
    let context = ConfigLoadContext::from_env().with_current_dir(repo.to_path_buf());
    load_config_from_file_with_context(config_path, &context).unwrap_or_abort()
}

#[test]
fn shipped_runtime_materializes_generic_default_and_named_subagents() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config_path = repo_root.join("configs/harness.example.jsonc");
    let config = load_config_from_repo_file(&config_path, &repo_root);

    // act
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();
    let profile_ids = coordinator_config
        .agent_profiles
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();

    // assert
    assert_eq!(
        profile_ids,
        vec!["default", "explore", "general", "librarian"]
    );
}

#[test]
fn shipped_v1_prompt_assets_have_contract_bodies() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config_path = repo_root.join("configs/harness.example.jsonc");
    let config = load_config_from_repo_file(&config_path, &repo_root);

    // act
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();

    // assert
    for profile in GENERIC_PROMPT_PROFILES.split_whitespace() {
        let prompt = &coordinator_config.agent_profiles[profile].system_prompt;
        assert!(
            prompt.contains("Harness"),
            "{profile} prompt must identify Harness"
        );
        assert!(
            prompt.contains("Do not") || prompt.contains("Guidelines:"),
            "{profile} prompt must state behavioral boundaries"
        );
        assert!(
            prompt.contains("evidence") || prompt.contains("Verify changes"),
            "{profile} prompt must require verification or evidence"
        );
    }
}

#[test]
fn shipped_v1_prompt_asset_snapshot_matches_source() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let snapshot_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("v1_prompt_assets.json");

    // act
    let actual = shipped_v1_prompt_asset_snapshot(&repo_root);
    let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
        panic!(
            "missing prompt snapshot {}; expected:\n{}",
            snapshot_path.display(),
            serde_json::to_string_pretty(&actual).unwrap_or_abort()
        )
    });
    let expected: serde_json::Value = serde_json::from_str(&expected).unwrap_or_abort();

    // assert
    assert_eq!(actual, expected, "shipped V1 prompt asset snapshot drifted");
}

#[test]
fn shipped_v1_prompt_section_snapshots_match_source() {
    // arrange
    let snapshot_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("v1_prompt_sections");
    let model = snapshot_model_target();
    let workspace = snapshot_workspace_environment();
    let environment = dynamic_prompt::DynamicPromptEnvironment {
        workspace: &workspace,
        platform: "linux",
        today: "Fri May 29 2026",
    };
    let context = dynamic_prompt::DynamicPromptContext {
        configured_prompt: Some("Runtime agent prompt fixture."),
        model: &model,
        instruction_prompt: Some("Instructions from: fixture\nFollow the fixture rule."),
        skill_tool_enabled: true,
    };
    let section_names = dynamic_prompt::registered_prompt_sections()
        .iter()
        .map(|section| section.name)
        .collect::<Vec<_>>();

    // act
    for section_name in section_names {
        let rendered = dynamic_prompt::render_prompt_section_with_environment(
            section_name,
            context,
            environment,
        )
        .unwrap_or_else(|| panic!("prompt section {section_name} did not render"));
        assert_snapshot_text(&snapshot_dir.join(format!("{section_name}.txt")), &rendered);
    }

    // assert
    let expected_files = dynamic_prompt::registered_prompt_sections()
        .iter()
        .map(|section| format!("{}.txt", section.name))
        .collect::<Vec<_>>();
    assert_snapshot_dir_contains_exact_files(&snapshot_dir, &expected_files);
}

#[test]
fn shipped_v1_full_composed_prompt_snapshots_match_source() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config_path = repo_root.join("configs/harness.example.jsonc");
    let config = load_config_from_repo_file(&config_path, &repo_root);
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();
    let snapshot_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("v1_composed_prompts");

    // act
    for profile in GENERIC_COMPOSED_PROMPTS {
        let runtime_prompt = coordinator_config
            .agent_profiles
            .get(profile)
            .unwrap_or_else(|| panic!("missing composed prompt profile {profile}"));
        let rendered = normalize_composed_prompt_snapshot(&runtime_prompt.system_prompt);
        assert!(
            !rendered.trim().is_empty(),
            "{profile} composed prompt snapshot source must not be empty"
        );
        assert_snapshot_text(&snapshot_dir.join(format!("{profile}.txt")), &rendered);
    }

    // assert
    let expected_files = GENERIC_COMPOSED_PROMPTS
        .iter()
        .map(|profile| format!("{profile}.txt"))
        .collect::<Vec<_>>();
    assert_snapshot_dir_contains_exact_files(&snapshot_dir, &expected_files);
}

include!("common/bootstrap_profile_helpers.rs");

const V1_FAMILY_PROMPT_SNAPSHOTS: [&str; 4] = ["anthropic", "gemini", "kimi", "trinity"];

fn family_prompt_model_target(
    family: harness_core::model_resolution::PromptFamily,
) -> ResolvedModelTarget {
    let model = format!("fixture-{}", family.id());
    let model_family = match family {
        harness_core::model_resolution::PromptFamily::Anthropic => {
            harness_core::model_resolution::ModelFamily::Claude
        }
        harness_core::model_resolution::PromptFamily::Gemini => {
            harness_core::model_resolution::ModelFamily::Gemini
        }
        harness_core::model_resolution::PromptFamily::Kimi => {
            harness_core::model_resolution::ModelFamily::Kimi
        }
        harness_core::model_resolution::PromptFamily::Trinity => {
            harness_core::model_resolution::ModelFamily::Trinity
        }
        _ => harness_core::model_resolution::ModelFamily::Unknown,
    };
    ResolvedModelTarget {
        model_ref: format!("default:{model}"),
        provider: "default".to_string(),
        model,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        resolution: harness_core::model_resolution::ModelResolution {
            family: model_family,
            family_source: harness_core::model_resolution::ModelFamilySource::Metadata,
            prompt_family: family,
            capabilities: harness_core::model_resolution::ModelCapabilities {
                variants: Vec::new(),
                reasoning_efforts: Vec::new(),
                supports_tool_calls: true,
                supports_vision: false,
                supports_temperature: true,
                supports_top_p: true,
                supports_thinking: false,
                supports_reasoning_summaries: false,
                context_window_tokens: None,
                max_input_tokens: None,
                max_output_tokens: None,
            },
        },
    }
}

#[test]
fn shipped_v1_family_prompt_assets_match_golden_snapshots() {
    // arrange
    // act
    // assert
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let snapshot_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("v1_family_prompts");
    let workspace = WorkspaceEnvironment {
        working_directory: repo_root.clone(),
        workspace_root: repo_root,
        is_git_repository: true,
        git_branch: Some("dev".to_string()),
    };
    let environment = dynamic_prompt::DynamicPromptEnvironment {
        workspace: &workspace,
        platform: "linux",
        today: "Fri May 29 2026",
    };

    for family in dynamic_prompt::family_prompt_asset_families() {
        let model = family_prompt_model_target(*family);
        let rendered = dynamic_prompt::compose_with_environment(
            dynamic_prompt::DynamicPromptContext {
                configured_prompt: None,
                model: &model,
                instruction_prompt: Some("Instructions from: fixture\nFollow the fixture rule."),
                skill_tool_enabled: true,
            },
            environment,
        );
        let rendered = normalize_composed_prompt_snapshot(&rendered);
        assert_snapshot_text(
            &snapshot_dir.join(format!("{}.txt", family.id())),
            &rendered,
        );
    }

    let expected_files = V1_FAMILY_PROMPT_SNAPSHOTS
        .iter()
        .map(|family| format!("{family}.txt"))
        .collect::<Vec<_>>();
    assert_snapshot_dir_contains_exact_files(&snapshot_dir, &expected_files);
}

#[test]
fn shipped_generic_prompt_bodies_have_distinct_scopes() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let required = [
        (
            "default",
            &[
                "expert coding assistant",
                "smallest complete change",
                "real user surface",
            ] as &[_],
        ),
        (
            "explore",
            &[
                "read-only codebase research helper",
                "files",
                "relationships",
                "answer",
                "next_steps",
                "Stop when",
            ],
        ),
        (
            "general",
            &[
                "focused helper",
                "bounded implementation",
                "compact parent context",
            ],
        ),
        (
            "librarian",
            &[
                "external research specialist",
                "official documentation",
                "source URL",
            ],
        ),
    ];

    for (profile, anchors) in required {
        let body = shipped_profile_body(&repo_root, profile);
        // act
        for anchor in anchors {
            // assert
            assert!(
                body.contains(anchor),
                "{profile} prompt missing distinctive generic-agent anchor `{anchor}`"
            );
        }
    }
}

#[test]
fn shipped_named_subagent_permissions_match_runtime_toolsets() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config_path = repo_root.join("configs/harness.example.jsonc");
    let config = load_config_from_repo_file(&config_path, &repo_root);
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();

    let default_task_description = task_description_for_profile(
        coordinator_config.tool_registry.as_ref(),
        &coordinator_config.agent_profiles["default"],
    );
    // act
    // assert
    for subagent in ["explore", "general", "librarian"] {
        assert!(
            default_task_description.contains(&format!("- {subagent}:")),
            "default task description must list {subagent}"
        );
        assert!(
            coordinator_denies_tool_for_profile(&coordinator_config, subagent, "task"),
            "named subagent {subagent} must not redelegate"
        );
    }
    assert!(
        coordinator_denies_tool_for_profile(&coordinator_config, "explore", "edit"),
        "explore must remain read-only"
    );
    assert!(
        coordinator_denies_tool_for_profile(&coordinator_config, "librarian", "edit"),
        "librarian must remain read-only"
    );
    assert!(
        !coordinator_denies_tool_for_profile(&coordinator_config, "general", "edit"),
        "general must retain bounded implementation capability"
    );
}

#[test]
fn shipped_generic_prompt_assets_are_distinct() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut distinctive_sections = std::collections::BTreeSet::new();

    // act
    for profile in GENERIC_PROMPT_PROFILES.split_whitespace() {
        let body = shipped_profile_body(&repo_root, profile);
        let distinctive = body;
        // assert
        assert!(
            distinctive_sections.insert(distinctive),
            "{profile} prompt duplicates another generic profile"
        );
    }
}

#[test]
fn dynamic_prompt_named_sections_are_addressable() {
    // arrange
    let sections = dynamic_prompt::registered_prompt_sections();
    let names = sections
        .iter()
        .map(|section| section.name)
        .collect::<Vec<_>>();
    for required in [
        "base_model",
        "environment",
        "delegation_reminder",
        "project_instructions",
        "skill_guidance",
        "intent_gate",
        // act
    ] {
        // assert
        assert!(
            names.contains(&required),
            "dynamic prompt section registry missing `{required}`"
        );
    }
}

mod runtime_bootstrap_test {
    use super::*;
    include!("bootstrap_profiles/runtime_bootstrap_test.rs");
}

mod permission_ruleset_export_test {
    use super::*;
    include!("bootstrap_profiles/permission_ruleset_export_test.rs");
}

mod oc_parity_permission_matrices_test {
    use super::*;
    include!("bootstrap_profiles/oc_parity_permission_matrices_test.rs");
}
