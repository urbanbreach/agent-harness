use harness_core::agent::{build_provider_tool_defs, AgentProfile};
use harness_core::config::{
    load_config_from_file_with_context, load_config_from_str, ConfigLoadContext, HarnessConfig,
    ResolvedModelTarget,
};
use harness_core::perm::{
    permission_kind_for_tool_call, PermissionKind, PermissionRuleRequest, PolicyDecision,
};
use harness_core::tool::ToolRegistry;
use harness_core::workspace::WorkspaceEnvironment;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

const V1_PROMPT_PROFILES: &str = "build plan general explore visual-engineering artistry ultrabrain deep quick unspecified-low unspecified-high writing";
const V1_PRIMARY_PROMPTS: [&str; 2] = ["build", "plan"];
const V1_CATEGORY_PROMPTS: [&str; 8] = [
    "visual-engineering",
    "artistry",
    "ultrabrain",
    "deep",
    "quick",
    "unspecified-low",
    "unspecified-high",
    "writing",
];
const V1_HIDDEN_PROMPTS: [&str; 3] = ["title", "summary", "compaction"];
const V1_COMPOSED_PROMPTS: [&str; 15] = [
    "build",
    "plan",
    "general",
    "explore",
    "visual-engineering",
    "artistry",
    "ultrabrain",
    "deep",
    "quick",
    "unspecified-low",
    "unspecified-high",
    "writing",
    "title",
    "summary",
    "compaction",
];
const UPDATE_PROMPT_SNAPSHOTS_ENV: &str = "HARNESS_UPDATE_PROMPT_SNAPSHOTS";

#[allow(dead_code)]
#[path = "../src/dynamic_prompt.rs"]
mod dynamic_prompt;

#[allow(dead_code)]
#[path = "../src/bootstrap.rs"]
mod bootstrap;

#[allow(dead_code)]
#[path = "../src/cli_config.rs"]
mod cli_config;

fn write_agent_markdown(repo_root: &Path, name: &str, body: &str) {
    let path = repo_root
        .join(".agent-harness")
        .join("agents")
        .join(format!("{name}.md"));
    fs::create_dir_all(path.parent().expect("agent markdown parent"))
        .expect("create agent markdown dir");
    fs::write(path, body).expect("write agent markdown");
}

fn load_config_from_repo_file(config_path: &Path, repo: &Path) -> HarnessConfig {
    let context = ConfigLoadContext::from_env().with_current_dir(repo.to_path_buf());
    load_config_from_file_with_context(config_path, &context).expect("load harness config")
}

#[test]
fn shipped_v1_prompt_assets_have_contract_bodies() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config_path = repo_root.join("configs/harness.example.jsonc");
    let config = load_config_from_repo_file(&config_path, &repo_root);

    // act
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).expect("build config");

    // assert
    for profile in V1_PROMPT_PROFILES.split_whitespace() {
        let prompt = &coordinator_config.agent_profiles[profile].system_prompt;
        for required in [
            "## Identity",
            "## Goal",
            "## Use When",
            "## Do Not Use When",
            "## Scope Guard",
            "## Runtime-Enforced Permissions",
            "## Behavioral Guidance",
            "## Operating Loop",
            "## Ask Gate",
            "## Failure Recovery",
            "## Output Contract",
            "## Verification Gate",
        ] {
            assert!(
                prompt.contains(required),
                "{profile} prompt missing required V1 prompt section {required}"
            );
        }
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
            serde_json::to_string_pretty(&actual).expect("render actual prompt snapshot")
        )
    });
    let expected: serde_json::Value =
        serde_json::from_str(&expected).expect("parse prompt asset snapshot");

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
        bootstrap::build_interactive_coordinator_config(&config).expect("build config");
    let snapshot_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("v1_composed_prompts");

    // act
    for hidden_profile in V1_HIDDEN_PROMPTS {
        assert!(
            V1_COMPOSED_PROMPTS.contains(&hidden_profile),
            "hidden profile {hidden_profile} must have a full composed prompt snapshot"
        );
    }

    for profile in V1_COMPOSED_PROMPTS {
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
    let expected_files = V1_COMPOSED_PROMPTS
        .iter()
        .map(|profile| format!("{profile}.txt"))
        .collect::<Vec<_>>();
    assert_snapshot_dir_contains_exact_files(&snapshot_dir, &expected_files);
}

include!("common/bootstrap_profile_helpers.rs");

#[test]
fn shipped_v1_prompt_bodies_have_agent_specific_seams_and_intent_gate() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let required = [
        (
            "build",
            &["hashline edit", "real CLI", "recoverable tool failure"] as &[_],
        ),
        (
            "plan",
            &[
                ".agent-harness/plans/<run>.md",
                "plan_exit",
                "plan_enter",
                "read-only shell guard",
                "delegate only to Explore",
            ],
        ),
        (
            "explore",
            &[
                "read-only tools",
                "files",
                "relationships",
                "answer",
                "next_steps",
                "stop condition",
            ],
        ),
        (
            "general",
            &[
                "multistep work",
                "belongs to Build",
                "compact parent context",
            ],
        ),
        (
            "visual-engineering",
            &["UI/UX", "layout", "visual evidence", "recursion-deny"],
        ),
        (
            "artistry",
            &["creative", "recursion-deny", "output contract"],
        ),
        (
            "ultrabrain",
            &["logic", "architecture", "effort estimate", "recursion-deny"],
        ),
        ("deep", &["end-to-end", "autonomous", "recursion-deny"]),
        ("quick", &["small", "low-risk", "recursion-deny"]),
        (
            "unspecified-low",
            &["low-to-moderate", "uncategorized", "recursion-deny"],
        ),
        (
            "unspecified-high",
            &["complex uncategorized", "high-effort", "recursion-deny"],
        ),
        ("writing", &["documentation", "prose", "recursion-deny"]),
    ];

    for (profile, anchors) in required {
        let body = shipped_profile_body(&repo_root, profile);
        // act
        for anchor in anchors {
            // assert
            assert!(
                body.contains(anchor),
                "{profile} prompt missing distinctive V1 seam anchor `{anchor}`"
            );
        }
    }

    for profile in V1_PRIMARY_PROMPTS {
        let body = shipped_profile_body(&repo_root, profile);
        assert!(
            body.contains("## Intent Gate"),
            "{profile} primary prompt missing named Intent Gate section"
        );
        for route in [
            "explain",
            "investigate",
            "implement",
            "plan",
            "ask exactly one blocking question",
        ] {
            assert!(
                body.contains(route),
                "{profile} Intent Gate missing route `{route}`"
            );
        }
    }
}

#[test]
fn shipped_profile_permission_promises_match_runtime_policy_and_toolsets() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config_path = repo_root.join("configs/harness.example.jsonc");
    let config = load_config_from_repo_file(&config_path, &repo_root);
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).expect("build config");

    let plan_prompt = prompt_section(
        &coordinator_config.agent_profiles["plan"].system_prompt,
        "Runtime-Enforced Permissions",
    );
    for anchor in [
        "write only the active `.agent-harness/plans/<run>.md` plan file",
        "runtime read-only shell guard for `bash`",
        "delegate only to Explore",
        // act
    ] {
        // assert
        assert!(
            plan_prompt.contains(anchor),
            "plan permission prompt missing runtime-enforced anchor `{anchor}`"
        );
    }
    assert_eq!(
        coordinator_config.permission_policy.evaluate_request(
            Some("plan"),
            PermissionKind::EditFs,
            Some(&PermissionRuleRequest::WorkspacePath(
                ".agent-harness/plans/run_demo.md".to_string()
            )),
        ),
        PolicyDecision::Allow
    );
    assert_eq!(
        coordinator_config.permission_policy.evaluate_request(
            Some("plan"),
            PermissionKind::EditFs,
            Some(&PermissionRuleRequest::WorkspacePath(
                "src/lib.rs".to_string()
            )),
        ),
        PolicyDecision::Deny
    );
    assert!(matches!(
        coordinator_config
            .permission_policy
            .evaluate(Some("plan"), PermissionKind::Shell),
        PolicyDecision::Ask { .. }
    ));
    let plan_task_description = task_description_for_profile(
        coordinator_config.tool_registry.as_ref(),
        &coordinator_config.agent_profiles["plan"],
    );
    assert!(plan_task_description.contains("- explore:"));
    assert!(!plan_task_description.contains("- general:"));

    let explore_prompt = prompt_section(
        &coordinator_config.agent_profiles["explore"].system_prompt,
        "Runtime-Enforced Permissions",
    );
    for (tool_id, prompt_anchor) in [
        ("edit", "denies edit"),
        ("bash", "bash/shell"),
        ("webfetch", "webfetch"),
        ("websearch", "websearch"),
        ("codesearch", "codesearch"),
        ("task", "task redelegation"),
    ] {
        assert!(
            explore_prompt.contains(prompt_anchor),
            "explore prompt missing restriction anchor `{prompt_anchor}`"
        );
        assert!(
            coordinator_denies_tool_for_profile(&coordinator_config, "explore", tool_id),
            "explore prompt claims `{tool_id}` is restricted but runtime allows it"
        );
    }
    assert!(
        explore_prompt.contains("MCP write calls"),
        "explore prompt must declare the MCP write-call boundary"
    );

    let general_prompt = prompt_section(
        &coordinator_config.agent_profiles["general"].system_prompt,
        "Runtime-Enforced Permissions",
    );
    assert!(general_prompt.contains("cannot redelegate"));
    assert!(coordinator_denies_tool_for_profile(
        &coordinator_config,
        "general",
        "task"
    ));

    for category in V1_CATEGORY_PROMPTS {
        let section = prompt_section(
            &coordinator_config.agent_profiles[category].system_prompt,
            "Runtime-Enforced Permissions",
        );
        assert!(
            section.contains("denies recursive task delegation"),
            "{category} prompt missing category recursion-deny claim"
        );
        assert!(
            coordinator_denies_tool_for_profile(&coordinator_config, category, "task"),
            "{category} prompt claims recursive task denial but runtime allows task"
        );
    }
}

#[test]
fn shipped_v1_primary_prompts_are_not_generic_scaffold_copies() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut distinctive_sections = std::collections::BTreeSet::new();

    for profile in V1_PRIMARY_PROMPTS {
        let body = shipped_profile_body(&repo_root, profile);
        let distinctive = format!(
            "{}\n{}",
            prompt_section(&body, "Behavioral Guidance"),
            prompt_section(&body, "Operating Loop") // act
        );
        // assert
        assert!(
            distinctive_sections.insert(distinctive),
            "{profile} Behavioral Guidance + Operating Loop duplicate another primary prompt"
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
