//! P0–P2 inventory fixtures for Harness permission parity (PRD).
//! Wave-0 T0: OpenCode agent.ts golden matrix (`opencode_agent_ts_matrix.json`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::agent::{build_provider_tool_defs, AgentProfile};
use harness_core::config::{
    PermissionMode, PermissionRuleSet, PermissionSelector, PermissionSelectorRule,
    ProfilePermissions, ToolFailureMode,
};
use harness_core::perm::{
    evaluate_ruleset, from_profile_permissions, is_tool_disabled, PermissionAction, PermissionRule,
};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use harness_core::UnwrapOrAbort;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct AgentDefaultsFixture {
    reference_git: String,
    source_files: Vec<String>,
    evaluate_default_action: String,
    agents: AgentsFixture,
}

#[derive(Debug, Deserialize)]
struct AgentsFixture {
    explore: ExploreFixture,
    plan: PlanFixture,
}

#[derive(Debug, Deserialize)]
struct ExploreFixture {
    must_hide: Vec<String>,
    must_show: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PlanFixture {
    edit_visible: bool,
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/permission_ruleset_parity/agent_defaults.json")
}

fn load_fixture() -> AgentDefaultsFixture {
    let raw = std::fs::read_to_string(fixture_path())
        .unwrap_or_else(|err| panic!("load agent_defaults.json: {err}"));
    serde_json::from_str(&raw).unwrap_or_else(|err| panic!("parse agent_defaults.json: {err}"))
}

#[derive(Debug, Deserialize)]
struct OpencodeAgentTsMatrixFixture {
    schema_version: u32,
    source_of_truth: SourceOfTruth,
    base_defaults: BaseDefaults,
    required_shipped_agents: Vec<String>,
    agents: BTreeMap<String, AgentMatrixEntry>,
    intentional_divergences: Vec<IntentionalDivergence>,
}

#[derive(Debug, Deserialize)]
struct SourceOfTruth {
    path: String,
}

#[derive(Debug, Deserialize)]
struct BaseDefaults {
    cite: String,
    rules: Vec<MatrixRule>,
}

#[derive(Debug, Deserialize)]
struct MatrixRule {
    permission: String,
    pattern: String,
    action: String,
    cite: String,
}

#[derive(Debug, Deserialize)]
struct AgentMatrixEntry {
    mode: String,
    hidden: bool,
    native_oc: bool,
    cite: String,
    #[serde(default)]
    overlay_rules: Vec<MatrixRule>,
    effective: BTreeMap<String, EffectivePermission>,
    #[serde(default)]
    notes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EffectivePermission {
    action: String,
    cite: String,
    #[serde(default)]
    patterns: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct IntentionalDivergence {
    id: String,
    summary: String,
    cite: String,
}

fn opencode_matrix_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/permission_ruleset_parity/opencode_agent_ts_matrix.json")
}

fn load_opencode_agent_ts_matrix() -> OpencodeAgentTsMatrixFixture {
    let path = opencode_matrix_fixture_path();
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("load opencode_agent_ts_matrix.json at {}: {err}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|err| {
        panic!(
            "parse opencode_agent_ts_matrix.json at {}: {err}",
            path.display()
        )
    })
}

const REQUIRED_SHIPPED_AGENTS: &[&str] = &[
    "build",
    "plan",
    "explore",
    "general",
    "title",
    "summary",
    "compaction",
    "visual-engineering",
    "artistry",
    "ultrabrain",
    "deep",
    "quick",
    "unspecified-low",
    "unspecified-high",
    "writing",
];

#[test]
fn t0_opencode_agent_ts_matrix_loads_without_panic() {
    // arrange
    // act
    let fixture = load_opencode_agent_ts_matrix();

    // assert
    assert_eq!(fixture.schema_version, 1);
    assert!(
        fixture
            .source_of_truth
            .path
            .contains("agent/agent.ts"),
        "source_of_truth must cite OC agent.ts, got {}",
        fixture.source_of_truth.path
    );
    assert!(
        !fixture.base_defaults.rules.is_empty(),
        "base_defaults.rules must encode OC defaults"
    );
    assert!(
        !fixture.base_defaults.cite.is_empty(),
        "base_defaults.cite must be present"
    );
    assert!(
        !fixture.intentional_divergences.is_empty(),
        "must document intentional divergences"
    );
    for divergence in &fixture.intentional_divergences {
        assert!(!divergence.id.is_empty());
        assert!(!divergence.summary.is_empty());
        assert!(
            !divergence.cite.is_empty(),
            "divergence {} missing cite",
            divergence.id
        );
    }
}

#[test]
fn t0_opencode_matrix_lists_every_shipped_agent() {
    // arrange
    let fixture = load_opencode_agent_ts_matrix();
    let required: BTreeSet<&str> = REQUIRED_SHIPPED_AGENTS.iter().copied().collect();
    let listed: BTreeSet<&str> = fixture
        .required_shipped_agents
        .iter()
        .map(String::as_str)
        .collect();
    let agent_keys: BTreeSet<&str> = fixture.agents.keys().map(String::as_str).collect();

    // assert
    assert_eq!(
        listed, required,
        "fixture required_shipped_agents must match REQUIRED_SHIPPED_AGENTS"
    );
    let missing: Vec<&str> = required.difference(&agent_keys).copied().collect();
    assert!(
        missing.is_empty(),
        "agents map missing entries for: {missing:?}"
    );
    let extra_required: Vec<&str> = listed.difference(&required).copied().collect();
    assert!(
        extra_required.is_empty(),
        "unexpected required_shipped_agents: {extra_required:?}"
    );
}

#[test]
fn t0_opencode_matrix_entries_cite_oc_or_harness_divergence() {
    // arrange
    let fixture = load_opencode_agent_ts_matrix();

    // assert
    for name in REQUIRED_SHIPPED_AGENTS {
        let entry = fixture
            .agents
            .get(*name)
            .unwrap_or_else(|| panic!("missing agent matrix entry `{name}`"));
        assert!(
            !entry.cite.is_empty(),
            "agent `{name}` missing top-level cite"
        );
        assert!(
            entry.cite.contains("OC ")
                || entry.cite.contains("agent.ts")
                || entry.cite.contains("Harness divergence"),
            "agent `{name}` cite must reference OC rule or Harness divergence, got: {}",
            entry.cite
        );
        assert!(
            !entry.effective.is_empty(),
            "agent `{name}` must encode effective permissions"
        );
        for (perm, effective) in &entry.effective {
            assert!(
                matches!(
                    effective.action.as_str(),
                    "allow" | "ask" | "deny"
                ),
                "agent `{name}` permission `{perm}` has invalid action {}",
                effective.action
            );
            assert!(
                !effective.cite.is_empty(),
                "agent `{name}` permission `{perm}` missing cite"
            );
            assert!(
                effective.cite.contains("OC ")
                    || effective.cite.contains("agent.ts")
                    || effective.cite.contains("Harness")
                    || effective.cite.contains("OC base")
                    || effective.cite.contains("OC catch-all"),
                "agent `{name}` permission `{perm}` cite must reference OC or Harness, got: {}",
                effective.cite
            );
        }
        for rule in &entry.overlay_rules {
            assert!(
                matches!(rule.action.as_str(), "allow" | "ask" | "deny"),
                "agent `{name}` overlay rule invalid action"
            );
            assert!(!rule.cite.is_empty(), "agent `{name}` overlay rule missing cite");
        }
        for note in &entry.notes {
            assert!(!note.is_empty(), "agent `{name}` has empty note");
        }
        assert!(
            matches!(entry.mode.as_str(), "primary" | "subagent" | "all"),
            "agent `{name}` has unexpected mode {}",
            entry.mode
        );
        let _ = entry.hidden;
        let _ = entry.native_oc;
    }
}

#[test]
fn t0_opencode_matrix_base_defaults_include_safety_exceptions() {
    // arrange
    let fixture = load_opencode_agent_ts_matrix();
    let by_perm: BTreeMap<(&str, &str), &str> = fixture
        .base_defaults
        .rules
        .iter()
        .map(|r| ((r.permission.as_str(), r.pattern.as_str()), r.action.as_str()))
        .collect();

    // assert
    assert_eq!(by_perm.get(&("*", "*")), Some(&"allow"));
    assert_eq!(by_perm.get(&("doom_loop", "*")), Some(&"ask"));
    assert_eq!(by_perm.get(&("external_directory", "*")), Some(&"ask"));
    assert_eq!(by_perm.get(&("question", "*")), Some(&"deny"));
    assert_eq!(by_perm.get(&("plan_enter", "*")), Some(&"deny"));
    assert_eq!(by_perm.get(&("plan_exit", "*")), Some(&"deny"));
    assert_eq!(by_perm.get(&("read", "*")), Some(&"allow"));
    assert_eq!(by_perm.get(&("read", "*.env")), Some(&"ask"));
    assert_eq!(by_perm.get(&("read", "*.env.*")), Some(&"ask"));
    assert_eq!(by_perm.get(&("read", "*.env.example")), Some(&"allow"));

    for rule in &fixture.base_defaults.rules {
        assert!(!rule.cite.is_empty(), "base rule missing cite: {rule:?}");
    }
}

#[test]
fn t0_opencode_matrix_documents_plan_shell_and_category_task_divergences() {
    // arrange
    let fixture = load_opencode_agent_ts_matrix();
    let plan = fixture
        .agents
        .get("plan")
        .expect("plan agent matrix entry");
    let plan_bash = plan
        .effective
        .get("bash")
        .expect("plan.effective.bash");

    // assert
    assert_eq!(plan_bash.action, "ask");
    assert!(
        plan_bash.cite.contains("Harness divergence"),
        "plan bash must cite Harness divergence, got {}",
        plan_bash.cite
    );
    for category in [
        "visual-engineering",
        "artistry",
        "ultrabrain",
        "deep",
        "quick",
        "unspecified-low",
        "unspecified-high",
        "writing",
    ] {
        let entry = fixture
            .agents
            .get(category)
            .unwrap_or_else(|| panic!("missing category `{category}`"));
        assert!(!entry.native_oc, "category `{category}` is not OC-native");
        let task = entry
            .effective
            .get("task")
            .unwrap_or_else(|| panic!("category `{category}` missing task effective"));
        assert_eq!(task.action, "deny");
        assert!(
            task.cite.contains("Harness divergence"),
            "category `{category}` task cite must be Harness divergence"
        );
    }

    let divergence_ids: BTreeSet<&str> = fixture
        .intentional_divergences
        .iter()
        .map(|d| d.id.as_str())
        .collect();
    assert!(divergence_ids.contains("plan_shell_ask_guard"));
    assert!(divergence_ids.contains("category_task_deny"));
}

fn explore_permissions() -> ProfilePermissions {
    ProfilePermissions {
        fallback: None,
        edit: Some(PermissionMode::Deny),
        shell: Some(PermissionMode::Allow),
        network: Some(PermissionMode::Deny),
        question: Some(PermissionMode::Deny),
        task: Some(PermissionMode::Deny),
        webfetch: Some(PermissionMode::Allow),
        websearch: Some(PermissionMode::Allow),
        codesearch: Some(PermissionMode::Deny),
        lsp: Some(PermissionMode::Deny),
        rules: PermissionRuleSet::default(),
    }
}

fn plan_permissions() -> ProfilePermissions {
    ProfilePermissions {
        edit: None,
        shell: Some(PermissionMode::Ask),
        rules: PermissionRuleSet {
            edit: vec![
                PermissionSelectorRule {
                    selector: PermissionSelector::CatchAll,
                    mode: PermissionMode::Deny,
                },
                PermissionSelectorRule {
                    selector: PermissionSelector::Prefix(".agent-harness/plans/".into()),
                    mode: PermissionMode::Allow,
                },
            ],
            task: vec![PermissionSelectorRule {
                selector: PermissionSelector::Exact("general".into()),
                mode: PermissionMode::Deny,
            }],
            ..PermissionRuleSet::default()
        },
        ..ProfilePermissions::default()
    }
}

#[test]
fn p0_fixture_cites_harness_sources_and_loads() {
    // arrange
    // act
    // assert
    let fixture = load_fixture();
    assert!(!fixture.reference_git.is_empty());
    assert!(
        fixture
            .source_files
            .iter()
            .any(|path| path.contains("permission/index.ts")),
        "fixture must cite permission evaluate/disabled source"
    );
    assert_eq!(fixture.evaluate_default_action, "ask");
}

#[test]
fn p0_explore_ruleset_matches_harness_disabled_outcomes() {
    // arrange
    // act
    // assert
    let fixture = load_fixture();
    let ruleset = from_profile_permissions(&explore_permissions());

    for tool in &fixture.agents.explore.must_hide {
        assert!(
            is_tool_disabled(tool, &ruleset),
            "explore must hide `{tool}` under Harness disabled() semantics"
        );
    }
    for tool in &fixture.agents.explore.must_show {
        assert!(
            !is_tool_disabled(tool, &ruleset),
            "explore must show `{tool}`"
        );
    }
}

#[test]
fn p0_plan_keeps_edit_visible_despite_catch_all_deny() {
    // arrange
    // act
    // assert
    let fixture = load_fixture();
    assert!(fixture.agents.plan.edit_visible);
    let ruleset = from_profile_permissions(&plan_permissions());
    assert!(!is_tool_disabled("edit", &ruleset));
    assert!(!is_tool_disabled("write", &ruleset));
}

#[test]
fn p0_evaluate_default_is_ask() {
    // arrange
    // act
    // assert
    let empty: &[PermissionRule] = &[];
    let rule = evaluate_ruleset("unknown_perm", "*", [empty]);
    assert_eq!(rule.action, PermissionAction::Ask);
}

#[test]
fn p2_provider_profile_ruleset_hides_catch_all_denied_tools() {
    // arrange
    // act
    // assert
    let ruleset = vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Deny,
        },
        PermissionRule {
            permission: "read".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "bash".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
    ];
    let toolset = ["read", "edit", "write", "task", "bash"];
    let visible: BTreeSet<&str> = toolset
        .into_iter()
        .filter(|id| !is_tool_disabled(id, &ruleset))
        .collect();
    assert_eq!(
        visible,
        BTreeSet::from(["read", "bash"]),
        "catch-all deny must hide edit/write/task while partial allows stay visible"
    );
}

struct StaticTool(&'static str);

#[async_trait]
impl Tool for StaticTool {
    fn id(&self) -> &str {
        self.0
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, _ctx: ToolContext, _args_json: Value) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text("ok"))
    }
}

fn inventory_tool_registry(tool_ids: &[&'static str]) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    for tool_id in tool_ids {
        registry.register(Arc::new(StaticTool(tool_id)));
    }
    registry
}

#[test]
fn p2_build_provider_tool_defs_omits_catch_all_denied_tools() {
    // arrange
    let tool_ids = ["read", "edit", "write", "task", "bash"];
    let registry = inventory_tool_registry(&tool_ids);
    let profile = AgentProfile {
        name: "explore-like".to_string(),
        category: "explore".to_string(),
        model_ref: "mock:model".to_string(),
        model_ref_explicit: true,
        system_prompt: "sys".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(4),
        temperature: Some(0.0),
        tool_failure_mode: ToolFailureMode::FailTurn,
        toolset: tool_ids.iter().map(|id| (*id).to_string()).collect(),
        permission_ruleset: from_profile_permissions(&explore_permissions()),
    };

    // act
    let defs = build_provider_tool_defs(&profile, &registry).unwrap_or_abort();
    let visible: BTreeSet<&str> = defs.iter().map(|def| def.tool_id.as_str()).collect();

    // assert
    assert!(
        visible.contains("read") && visible.contains("bash"),
        "allowed tools must remain visible: {visible:?}"
    );
    for hidden in ["edit", "write", "task"] {
        assert!(
            !visible.contains(hidden),
            "build_provider_tool_defs must omit catch-all denied `{hidden}`; visible={visible:?}"
        );
    }
}

#[test]
fn p2_build_provider_tool_defs_keeps_edit_when_plan_path_allow_exists() {
    // arrange
    let tool_ids = ["read", "edit", "write", "task", "bash"];
    let registry = inventory_tool_registry(&tool_ids);
    let profile = AgentProfile {
        name: "plan-like".to_string(),
        category: "plan".to_string(),
        model_ref: "mock:model".to_string(),
        model_ref_explicit: true,
        system_prompt: "sys".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(4),
        temperature: Some(0.0),
        tool_failure_mode: ToolFailureMode::FailTurn,
        toolset: tool_ids.iter().map(|id| (*id).to_string()).collect(),
        permission_ruleset: from_profile_permissions(&plan_permissions()),
    };

    // act
    let defs = build_provider_tool_defs(&profile, &registry).unwrap_or_abort();
    let visible: BTreeSet<&str> = defs.iter().map(|def| def.tool_id.as_str()).collect();

    // assert
    assert!(
        visible.contains("edit") && visible.contains("write"),
        "partial plan-path allow must keep edit/write visible: {visible:?}"
    );
}
