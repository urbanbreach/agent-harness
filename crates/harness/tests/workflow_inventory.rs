mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use common::repo_root;
use harness_core::agent_catalog::SLASH_AGENT_NAMES;
use harness_core::command_registry::{CommandAction, CommandRegistry};
use serde_json::Value;

const INVENTORY_JSON: &str = include_str!("fixtures/harness_omx_workflow_inventory.json");
const LOCKED_OMX_SKILL_IDS: &[&str] = &[
    "omx-skill:ai-slop-cleaner",
    "omx-skill:analyze",
    "omx-skill:ask",
    "omx-skill:ask-claude",
    "omx-skill:ask-gemini",
    "omx-skill:autopilot",
    "omx-skill:autoresearch",
    "omx-skill:autoresearch-goal",
    "omx-skill:best-practice-research",
    "omx-skill:build-fix",
    "omx-skill:cancel",
    "omx-skill:code-review",
    "omx-skill:configure-notifications",
    "omx-skill:deep-interview",
    "omx-skill:deepsearch",
    "omx-skill:design",
    "omx-skill:doctor",
    "omx-skill:ecomode",
    "omx-skill:frontend-ui-ux",
    "omx-skill:git-master",
    "omx-skill:help",
    "omx-skill:hud",
    "omx-skill:note",
    "omx-skill:omx-setup",
    "omx-skill:performance-goal",
    "omx-skill:pipeline",
    "omx-skill:plan",
    "omx-skill:ralph",
    "omx-skill:ralph-init",
    "omx-skill:ralplan",
    "omx-skill:review",
    "omx-skill:security-review",
    "omx-skill:skill",
    "omx-skill:swarm",
    "omx-skill:tdd",
    "omx-skill:team",
    "omx-skill:trace",
    "omx-skill:ultragoal",
    "omx-skill:ultraqa",
    "omx-skill:ultrawork",
    "omx-skill:visual-ralph",
    "omx-skill:visual-verdict",
    "omx-skill:web-clone",
    "omx-skill:wiki",
    "omx-skill:worker",
];

#[test]
fn workflow_inventory_fixture_has_required_schema() {
    let inventory = inventory();
    let allowed_statuses = string_set(inventory["status_values"].as_array().unwrap());
    assert_eq!(
        allowed_statuses,
        BTreeSet::from([
            "clashing",
            "missing",
            "non_applicable",
            "partial",
            "present"
        ])
    );

    let required_fields = string_set(inventory["required_fields"].as_array().unwrap());
    assert_eq!(
        required_fields,
        BTreeSet::from([
            "aliases",
            "authority_model",
            "blocker_or_stage",
            "canonical_id",
            "harness_mapping",
            "source_refs",
            "status",
            "tests",
            "visibility"
        ])
    );

    let entries = inventory["entries"].as_array().unwrap();
    assert!(
        entries.len() >= 100,
        "inventory should cover Harness, project skills, and OMX reference surfaces"
    );

    let mut ids = BTreeSet::new();
    for entry in entries {
        for field in &required_fields {
            assert!(
                entry.get(*field).is_some(),
                "entry {entry:?} missing required field {field}"
            );
        }

        let canonical_id = non_empty_string(entry, "canonical_id");
        assert!(
            ids.insert(canonical_id),
            "duplicate inventory id {canonical_id}"
        );
        assert_array_of_strings(entry, "aliases");
        assert_non_empty_array_of_strings(entry, "source_refs");
        assert_non_empty_array_of_strings(entry, "tests");
        non_empty_string(entry, "visibility");
        non_empty_string(entry, "authority_model");

        let status = non_empty_string(entry, "status");
        assert!(allowed_statuses.contains(status), "unknown status {status}");
        let blocker_or_stage = non_empty_string(entry, "blocker_or_stage");
        if matches!(status, "partial" | "missing" | "clashing") {
            assert_ne!(
                blocker_or_stage, "none",
                "{canonical_id} must describe its blocker/stage"
            );
        }

        let mapping = entry["harness_mapping"]
            .as_object()
            .unwrap_or_else(|| panic!("{canonical_id} missing harness_mapping object"));
        assert!(
            mapping
                .get("surface")
                .and_then(Value::as_str)
                .is_some_and(|surface| !surface.is_empty()),
            "{canonical_id} missing harness_mapping.surface"
        );
        assert!(
            mapping
                .get("target")
                .and_then(Value::as_str)
                .is_some_and(|target| !target.is_empty()),
            "{canonical_id} missing harness_mapping.target"
        );
    }
}

#[test]
fn workflow_inventory_tracks_command_registry_drift() {
    let inventory = inventory();
    let entries = entries_by_id(&inventory);
    let registry = CommandRegistry::builtins();

    for command in registry.commands() {
        let entry = entries
            .get(command.name)
            .unwrap_or_else(|| panic!("missing inventory entry for command {}", command.name));
        let aliases = value_strings(entry["aliases"].as_array().unwrap());
        for alias in command.aliases {
            assert!(
                aliases.contains(alias),
                "{} missing command-registry alias {}",
                command.name,
                alias
            );
        }
        for alias in command.dollar_aliases {
            assert!(
                command.name == *alias || aliases.contains(alias),
                "{} missing registry-derived dollar alias ${alias}",
                command.name
            );
        }

        let semantics = entry["runtime_semantics"]
            .as_object()
            .unwrap_or_else(|| panic!("{} missing runtime_semantics", command.name));
        assert_eq!(
            semantics.get("surface").and_then(Value::as_str),
            Some(command.surface.as_str()),
            "{} inventory surface drifted from runtime registry",
            command.name
        );
        assert_eq!(
            semantics.get("effect").and_then(Value::as_str),
            Some(command.effect.as_str()),
            "{} inventory effect drifted from runtime registry",
            command.name
        );
        assert_eq!(
            semantics.get("availability").and_then(Value::as_str),
            Some(command.availability.as_str()),
            "{} inventory availability drifted from runtime registry",
            command.name
        );

        let target = entry["harness_mapping"]["target"].as_str().unwrap();
        match &command.action {
            CommandAction::WorkflowSkill {
                skill,
                intent,
                continuation_mode,
                ..
            } => {
                assert!(
                    target == intent.as_str() || target.contains(intent.as_str()),
                    "{} target {target} should mention workflow intent {}",
                    command.name,
                    intent.as_str()
                );
                assert!(
                    target.contains(skill) || command.name.contains(skill),
                    "{} target {target} should mention workflow skill {skill}",
                    command.name
                );
                if let Some(mode) = continuation_mode {
                    assert!(
                        target.contains(mode.as_str()),
                        "{} target {target} should mention continuation mode {}",
                        command.name,
                        mode.as_str()
                    );
                }
            }
            CommandAction::WorkflowIntent { intent } => assert_eq!(target, intent.as_str()),
            CommandAction::StartContinuation { mode } => {
                assert!(
                    target.contains(mode.as_str()),
                    "{} target {target} should mention continuation mode {}",
                    command.name,
                    mode.as_str()
                );
            }
            CommandAction::StopContinuation => {
                assert!(
                    target.contains("stop"),
                    "{} target {target} should describe stop-continuation",
                    command.name
                );
            }
            CommandAction::BlockedWorkflow {
                reason,
                inventory_ref,
            } => {
                assert_eq!(
                    *inventory_ref, command.name,
                    "{} blocked workflow action should point at its inventory entry",
                    command.name
                );
                assert!(
                    !reason.trim().is_empty(),
                    "{} blocked workflow action should explain why it is non-completing",
                    command.name
                );
                assert!(
                    target.contains("blocked"),
                    "{} target {target} should identify blocked workflow dispatch",
                    command.name
                );
            }
            CommandAction::LoadSkills { skills } => {
                for skill in *skills {
                    assert!(
                        target.contains(skill),
                        "{} target {target} should mention loaded skill {skill}",
                        command.name
                    );
                }
            }
            CommandAction::SlashAgent { role } => {
                assert!(
                    target.contains(role),
                    "{} target {target} should mention slash-agent role {role}",
                    command.name
                );
            }
            CommandAction::PlanArtifact { artifact }
            | CommandAction::HandoffArtifact { artifact } => {
                assert!(
                    target.contains(artifact),
                    "{} target {target} should mention artifact {artifact}",
                    command.name
                );
            }
            CommandAction::PromptTemplate { .. }
            | CommandAction::ProfileSwitch { .. }
            | CommandAction::NativeTool { .. }
            | CommandAction::TuiAction { .. } => {}
        }
    }
}

#[test]
fn workflow_inventory_hides_staged_placeholder_commands_from_default_tui() {
    let inventory = inventory();
    let entries = entries_by_id(&inventory);
    let registry = CommandRegistry::builtins();
    let app_rs = fs::read_to_string(repo_root().join("crates/harness-tui/src/app.rs")).unwrap();
    assert!(
        app_rs.contains("commands.extend(registered_slash_commands())"),
        "TUI slash command list must derive registered workflow commands from CommandRegistry"
    );
    assert!(
        app_rs.contains("filter(|command| command.enabled_by_default)"),
        "TUI registered workflow slash commands must honor enabled_by_default"
    );
    let static_tui_commands = parse_slash_commands(&app_rs)
        .into_iter()
        .collect::<BTreeSet<_>>();

    for command in registry.commands() {
        let entry = entries
            .get(command.name)
            .unwrap_or_else(|| panic!("missing inventory entry for command {}", command.name));
        assert!(
            !static_tui_commands.contains(command.name),
            "{} should be registry-derived, not duplicated in static TUI slash commands",
            command.name
        );

        if !command.enabled_by_default {
            assert!(
                matches!(
                    &command.action,
                    CommandAction::BlockedWorkflow { .. }
                        | CommandAction::StopContinuation
                        | CommandAction::StartContinuation { .. }
                ),
                "{} disabled command must be explicitly blocked or non-completing, not a prompt/no-op placeholder",
                command.name
            );
            assert_ne!(
                entry["status"].as_str().unwrap(),
                "present",
                "{} disabled command should not be inventoried as fully present",
                command.name
            );
        }
    }
}

#[test]
fn workflow_inventory_tracks_tui_slash_command_drift() {
    let inventory = inventory();
    let entries = entries_by_id(&inventory);
    let app_rs = fs::read_to_string(repo_root().join("crates/harness-tui/src/app.rs")).unwrap();
    let session_navigation_rs =
        fs::read_to_string(repo_root().join("crates/harness-tui/src/app/session_navigation.rs"))
            .unwrap();

    for command in parse_slash_commands(&app_rs) {
        assert!(
            entries.contains_key(command.as_str()),
            "missing inventory entry for TUI slash command {command}"
        );
    }

    for (command, aliases) in parse_slash_aliases(&session_navigation_rs) {
        let entry = entries.get(command.as_str()).unwrap_or_else(|| {
            panic!("missing inventory entry for TUI slash alias owner {command}")
        });
        let inventory_aliases = value_strings(entry["aliases"].as_array().unwrap());
        for alias in aliases {
            assert!(
                inventory_aliases.contains(alias.as_str()),
                "{command} missing TUI slash alias {alias}"
            );
        }
    }
}

#[test]
fn workflow_inventory_tracks_registry_derived_tui_dollar_commands() {
    let root = repo_root();
    let app_rs = fs::read_to_string(root.join("crates/harness-tui/src/app.rs")).unwrap();
    let session_navigation_rs =
        fs::read_to_string(root.join("crates/harness-tui/src/app/session_navigation.rs")).unwrap();

    assert!(
        app_rs.contains("pub(crate) fn registered_dollar_commands()"),
        "TUI dollar command list must expose a registry-derived adapter"
    );
    assert!(
        app_rs.contains(".dollar_aliases"),
        "TUI dollar command descriptions must derive registry dollar aliases"
    );
    assert!(
        !app_rs.contains("STAGED_DOLLAR_COMMANDS"),
        "TUI dollar command list must not carry a hardcoded staged-command table"
    );
    assert!(
        session_navigation_rs.contains(".dollar_aliases.contains(&command)"),
        "TUI dollar command dispatch must resolve through CommandRegistry dollar aliases"
    );
    assert!(
        !session_navigation_rs.contains("dollar command ${command} is not executable"),
        "staged dollar commands must fail closed through CommandRegistry BlockedWorkflow actions"
    );
    assert!(
        !session_navigation_rs.contains("\"deep-interview\" => \"init-deep\""),
        "TUI must not keep a hardcoded dollar-to-registry dispatch table"
    );
}

#[test]
fn workflow_inventory_tracks_skill_and_agent_asset_drift() {
    let inventory = inventory();
    let entries = entries_by_id(&inventory);
    let root = repo_root();

    for agent_id in agent_ids(&root) {
        assert!(
            entries.contains_key(agent_id.as_str()),
            "missing inventory entry for agent asset {agent_id}"
        );
    }

    for (label, dir) in [
        ("harness-skill", ".agent-harness/skills"),
        ("project-codex-skill", ".codex/skills"),
        ("project-agent-skill", ".agents/skills"),
    ] {
        for skill_id in skill_ids(&root.join(dir), label) {
            assert!(
                entries.contains_key(skill_id.as_str()),
                "missing inventory entry for skill asset {skill_id}"
            );
        }
    }
}

#[test]
fn workflow_inventory_locks_omx_skill_and_slash_agent_rosters() {
    let inventory = inventory();
    let entries = entries_by_id(&inventory);

    let omx_skill_ids = entries
        .keys()
        .copied()
        .filter(|id| id.starts_with("omx-skill:"))
        .collect::<BTreeSet<_>>();
    let locked_omx_skill_ids = LOCKED_OMX_SKILL_IDS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        omx_skill_ids, locked_omx_skill_ids,
        "the command-parity fixture must keep the locked oh-my-codex skill roster without reading untracked inspirations assets"
    );
    let locked_omx_skill_ids_owned = locked_omx_skill_ids
        .iter()
        .map(|id| (*id).to_string())
        .collect::<BTreeSet<_>>();
    let reference_omx_skill_ids = skill_ids(
        &repo_root().join("inspirations/oh-my-codex/skills"),
        "omx-skill",
    )
    .into_iter()
    .collect::<BTreeSet<_>>();
    assert_eq!(
        locked_omx_skill_ids_owned, reference_omx_skill_ids,
        "locked command roster must match inspirations/oh-my-codex/skills exactly"
    );

    let slash_agent_ids = slash_agent_roster_ids();
    assert_eq!(
        slash_agent_ids.len(),
        30,
        "the command-parity reference lock expects 30 slash-agent roles"
    );
    for agent_id in slash_agent_ids {
        assert!(
            entries.contains_key(agent_id.as_str()),
            "missing inventory entry for slash-agent roster item {agent_id}"
        );
    }

    for role in SLASH_AGENT_NAMES {
        let entry = entries
            .get(format!("slash-agent:{role}").as_str())
            .unwrap_or_else(|| panic!("missing slash-agent inventory entry for {role}"));
        assert_eq!(entry["status"].as_str(), Some("present"));
        assert_eq!(entry["visibility"].as_str(), Some("slash_agent"));
        assert_eq!(
            entry["authority_model"].as_str(),
            Some("coordinator_task_tool_with_profile_permissions")
        );
        assert_eq!(
            entry["harness_mapping"]["surface"].as_str(),
            Some("slash_agent_command")
        );
        assert_eq!(
            entry["harness_mapping"]["target"].as_str(),
            Some(format!("task.subagent_type:{role}").as_str())
        );
    }

    let registry = CommandRegistry::builtins();
    let registry_dollar_aliases = registry
        .commands()
        .iter()
        .flat_map(|command| command.dollar_aliases.iter().copied())
        .collect::<BTreeSet<_>>();
    for skill_id in LOCKED_OMX_SKILL_IDS {
        let entry = entries
            .get(*skill_id)
            .unwrap_or_else(|| panic!("missing inventory entry for {skill_id}"));
        if entry["status"].as_str() == Some("non_applicable") {
            assert_eq!(
                *skill_id, "omx-skill:worker",
                "only worker may be a non-applicable oh-my-codex skill"
            );
            continue;
        }
        let skill_name = skill_id.strip_prefix("omx-skill:").unwrap();
        assert!(
            registry_dollar_aliases.contains(skill_name),
            "{skill_id} must be exposed as a registry-backed dollar command"
        );
    }

    for entry in inventory["entries"].as_array().unwrap() {
        assert!(
            !non_empty_string(entry, "canonical_id").contains("$$"),
            "inventory ids must normalize escaped dollar renderings"
        );
        for alias in entry["aliases"].as_array().unwrap() {
            assert!(
                !alias.as_str().unwrap().contains("$$"),
                "inventory aliases must normalize escaped dollar renderings"
            );
        }
    }
}

#[test]
fn workflow_inventory_tracks_oh_my_codex_prompt_parity() {
    let root = repo_root();
    let reference_prompt_dir = root.join("inspirations/oh-my-codex/prompts");
    let harness_agent_dir = root.join(".agent-harness/omx-prompts");
    let mut reference_prompts = Vec::new();

    for entry in fs::read_dir(&reference_prompt_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            reference_prompts.push(path);
        }
    }
    reference_prompts.sort();
    assert_eq!(
        reference_prompts.len(),
        33,
        "oh-my-codex prompt parity lock should track every prompt asset"
    );

    for reference_path in reference_prompts {
        let file_name = reference_path.file_name().unwrap();
        let harness_path = harness_agent_dir.join(file_name);
        assert!(
            harness_path.exists(),
            "missing Harness agent prompt copied from {}",
            reference_path.display()
        );
        assert_eq!(
            fs::read_to_string(&harness_path).unwrap(),
            fs::read_to_string(&reference_path).unwrap(),
            "{} must stay byte-for-byte aligned with oh-my-codex",
            harness_path.display()
        );
    }
}

#[test]
fn workflow_inventory_locks_slash_agent_definitions_to_reference_manifest() {
    let root = repo_root();
    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(root.join("inspirations/oh-my-codex/src/catalog/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    let reference_agents = manifest["agents"].as_array().unwrap();
    let reference_names = reference_agents
        .iter()
        .map(|agent| agent["name"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    let harness_names = SLASH_AGENT_NAMES.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(
        harness_names, reference_names,
        "slash-agent roster must match oh-my-codex catalog agents"
    );

    assert_eq!(
        harness_core::agent_catalog::slash_agent_definitions().len(),
        reference_agents.len(),
        "Harness must keep one slash-agent definition for every oh-my-codex catalog agent"
    );
}

#[test]
fn workflow_inventory_tracks_native_agent_config_parity() {
    let root = repo_root();
    let native_agent_dir = root.join(".agent-harness/native-agents");
    for definition in harness_core::agent_catalog::slash_agent_definitions() {
        let toml_path = native_agent_dir.join(format!("{}.toml", definition.name));
        let toml = fs::read_to_string(&toml_path)
            .unwrap_or_else(|err| panic!("missing native agent config {toml_path:?}: {err}"));
        assert!(
            toml.contains(&format!("# oh-my-codex agent: {}", definition.name)),
            "{} must be generated from the oh-my-codex native agent contract",
            toml_path.display()
        );
        assert!(
            toml.contains(&format!("name = \"{}\"", definition.name)),
            "{} missing matching name",
            definition.name
        );
        assert!(
            toml.contains(&format!("description = \"{}\"", definition.description)),
            "{} missing matching description",
            definition.name
        );
        assert!(
            toml.contains(&format!(
                "model_reasoning_effort = \"{}\"",
                definition.reasoning_effort
            )),
            "{} missing matching reasoning effort",
            definition.name
        );
        let expected_model =
            if definition.name == "executor" || definition.model_class == "frontier" {
                "gpt-5.5"
            } else {
                "gpt-5.4-mini"
            };
        assert!(
            toml.contains(&format!("model = \"{expected_model}\"")),
            "{} missing expected model {expected_model}",
            definition.name
        );
        for metadata_line in [
            format!("- role: {}", definition.name),
            format!("- posture: {}", definition.posture),
            format!("- model_class: {}", definition.model_class),
            format!("- routing_role: {}", definition.routing_role),
            format!("- resolved_model: {expected_model}"),
        ] {
            assert!(
                toml.contains(&metadata_line),
                "{} missing metadata line {metadata_line}",
                definition.name
            );
        }
    }
}

#[test]
fn workflow_inventory_markdown_points_at_fixture_and_gate() {
    let markdown =
        fs::read_to_string(repo_root().join("docs/harness-omx-workflow-inventory.md")).unwrap();
    assert!(markdown.contains("crates/harness/tests/fixtures/harness_omx_workflow_inventory.json"));
    assert!(markdown.contains("Deterministic drift gate"));
    assert!(markdown.contains("`workflow-run`"));
    assert!(markdown.contains("`workflow-evidence`"));
    assert!(markdown.contains("`omx-skill:ultragoal`"));
    assert!(markdown.contains("`slash-agent:executor`"));
}

fn inventory() -> Value {
    serde_json::from_str(INVENTORY_JSON).expect("valid workflow inventory JSON fixture")
}

fn entries_by_id(inventory: &Value) -> BTreeMap<&str, &Value> {
    inventory["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| (entry["canonical_id"].as_str().unwrap(), entry))
        .collect()
}

fn string_set(values: &[Value]) -> BTreeSet<&str> {
    values.iter().map(|value| value.as_str().unwrap()).collect()
}

fn value_strings(values: &[Value]) -> BTreeSet<&str> {
    values.iter().map(|value| value.as_str().unwrap()).collect()
}

fn non_empty_string<'a>(entry: &'a Value, field: &str) -> &'a str {
    entry[field]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("entry {entry:?} missing non-empty string field {field}"))
}

fn assert_array_of_strings(entry: &Value, field: &str) {
    let values = entry[field]
        .as_array()
        .unwrap_or_else(|| panic!("entry {entry:?} missing array field {field}"));
    for value in values {
        assert!(
            value.as_str().is_some(),
            "entry {entry:?} has non-string value in {field}"
        );
    }
}

fn assert_non_empty_array_of_strings(entry: &Value, field: &str) {
    let values = entry[field]
        .as_array()
        .unwrap_or_else(|| panic!("entry {entry:?} missing array field {field}"));
    assert!(!values.is_empty(), "entry {entry:?} has empty {field}");
    for value in values {
        assert!(
            value.as_str().is_some_and(|value| !value.is_empty()),
            "entry {entry:?} has empty/non-string value in {field}"
        );
    }
}

fn parse_slash_commands(app_rs: &str) -> Vec<String> {
    let Some(start) = app_rs.find("pub(crate) const SLASH_COMMANDS") else {
        panic!("SLASH_COMMANDS constant missing");
    };
    let body = &app_rs[start..];
    let Some(end) = body.find("];") else {
        panic!("SLASH_COMMANDS terminator missing");
    };
    let mut commands = Vec::new();
    let mut pending_multiline_tuple = false;
    for line in body[..end].lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix("(\"") {
            let end = rest.find('"').expect("slash command string terminator");
            commands.push(rest[..end].to_string());
            pending_multiline_tuple = false;
            continue;
        }
        if line == "(" {
            pending_multiline_tuple = true;
            continue;
        }
        if pending_multiline_tuple {
            if let Some(rest) = line.strip_prefix('"') {
                let end = rest.find('"').expect("slash command string terminator");
                commands.push(rest[..end].to_string());
            }
            pending_multiline_tuple = false;
        }
    }
    commands
}

fn parse_slash_aliases(session_navigation_rs: &str) -> BTreeMap<String, Vec<String>> {
    let Some(start) = session_navigation_rs.find("fn slash_command_aliases") else {
        panic!("slash_command_aliases function missing");
    };
    let body = &session_navigation_rs[start..];
    let Some(end) = body.find("\n}\n\nfn slash_command_display_width") else {
        panic!("slash_command_aliases terminator missing");
    };

    let mut aliases_by_command = BTreeMap::new();
    let mut active_command: Option<String> = None;
    let mut active_aliases = Vec::new();
    for line in body[..end].lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('"') && trimmed.contains("=> &[") {
            if let Some(command) = active_command.take() {
                aliases_by_command.insert(command, std::mem::take(&mut active_aliases));
            }
            let quoted = quoted_strings(trimmed);
            if let Some(command) = quoted.first() {
                active_command = Some(command.clone());
                active_aliases.extend(quoted.into_iter().skip(1));
            }
            if trimmed.contains("],") {
                if let Some(command) = active_command.take() {
                    aliases_by_command.insert(command, std::mem::take(&mut active_aliases));
                }
            }
        } else if active_command.is_some() {
            active_aliases.extend(quoted_strings(trimmed));
            if trimmed.contains("],") {
                if let Some(command) = active_command.take() {
                    aliases_by_command.insert(command, std::mem::take(&mut active_aliases));
                }
            }
        }
    }
    if let Some(command) = active_command {
        aliases_by_command.insert(command, active_aliases);
    }
    aliases_by_command.retain(|_, aliases| !aliases.is_empty());
    aliases_by_command
}

fn quoted_strings(line: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('"') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('"') else {
            break;
        };
        values.push(rest[..end].to_string());
        rest = &rest[end + 1..];
    }
    values
}

fn agent_ids(root: &Path) -> Vec<String> {
    let mut ids = Vec::new();
    let agents_dir = root.join(".agent-harness/agents");
    if let Ok(entries) = fs::read_dir(agents_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
                let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap();
                ids.push(format!("agent:{stem}"));
            }
        }
    }
    ids.sort();
    ids
}

fn skill_ids(base: &Path, label: &str) -> Vec<String> {
    let mut paths = Vec::new();
    collect_skill_files(base, &mut paths);
    let mut ids = paths
        .into_iter()
        .map(|path| {
            let rel = path.parent().unwrap().strip_prefix(base).unwrap();
            let rel = rel.to_string_lossy().replace('\\', "/");
            format!("{label}:{rel}")
        })
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn collect_skill_files(dir: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_skill_files(&path, paths);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
            paths.push(path);
        }
    }
}

fn slash_agent_roster_ids() -> Vec<String> {
    SLASH_AGENT_NAMES
        .iter()
        .map(|role| format!("slash-agent:{role}"))
        .collect()
}
