//! Configuration contracts and model selection.
pub use harness_compaction::{CompactionMethod, ManualMode, ShakeMode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    num::NonZeroU32,
    path::{Path, PathBuf},
};
mod defaults;
mod discovery;
mod edit;
mod eval;
mod extensions;
mod integrations;
mod limits;
mod loader;
mod models;
mod normalize;
mod numbers;
mod ordered;
mod permissions;
mod profiles;
mod provider;
mod references;
mod registries;
mod resolved;
mod runtime;
mod schema;
mod selection;
mod selectors;
mod settings_registry;
#[cfg(test)]
mod settings_tests;
mod settings_write;
mod skills;
mod subagents;
mod threshold;
pub use crate::perm::PermissionAction as PermissionMode;
pub use defaults::{default_permission_rule_set_with_read_env, default_read_env_permission_rules};
pub use discovery::*;
pub use edit::*;
pub use eval::*;
pub use extensions::*;
pub use integrations::*;
pub use limits::*;
pub use loader::*;
pub use models::*;
pub use permissions::*;
pub use profiles::*;
pub use provider::*;
pub use registries::*;
pub use resolved::*;
pub use runtime::*;
pub use schema::config_json_schema;
pub use selection::*;
pub use settings_registry::*;
pub use settings_write::*;
pub use skills::*;
pub use subagents::*;
pub use threshold::*;

const fn yes() -> bool {
    true
}
const fn mcp_timeout() -> u64 {
    30
}

#[derive(Debug, thiserror::Error)]
#[error("configuration: {0}")]
pub struct ConfigError(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionFile {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct HarnessConfig {
    #[serde(rename = "$schema")]
    pub schema: Option<String>,
    pub providers: BTreeMap<String, ProviderConfig>,
    pub disabled_providers: Vec<String>,
    pub enabled_providers: Vec<String>,
    #[serde(
        rename = "model_profile",
        alias = "modelProfile",
        alias = "model_profiles"
    )]
    pub model_profiles: BTreeMap<String, ModelProfileConfig>,
    pub agents: BTreeMap<String, ProfileConfig>,
    pub subagents: SubagentsConfig,
    pub features: SubagentFeaturesConfig,
    pub permissions: PermissionsConfig,
    pub runtime: RuntimeConfig,
    pub integrations: IntegrationsConfig,
    pub hooks: HooksConfig,
    pub skills: SkillsConfig,
    pub lsp: LspConfig,
    pub eval: EvalConfig,
    #[serde(skip)]
    pub background_task: BackgroundTaskSettings,
    #[serde(skip)]
    pub paths: PathsConfig,
    #[serde(skip)]
    pub deterministic: DeterministicConfig,
    pub ui: UiConfig,
    pub logging: LoggingConfig,
    pub edit: EditConfig,
    pub formatter: FormatterConfig,
    #[serde(skip)]
    pub instruction_files: Vec<InstructionFile>,
    #[serde(alias = "modelRoles")]
    pub model_roles: ModelRolesConfig,
}

pub type CompactionRuntimeConfig = CompactionSettings;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct FormatterConfig {
    pub enabled: bool,
    #[serde(alias = "experimentalOxfmt")]
    pub experimental_oxfmt: bool,
    #[serde(flatten)]
    pub overrides: BTreeMap<String, FormatterOverride>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_substitution_has_a_total_size_limit() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        let mut context = ConfigLoadContext::from_env().with_current_dir(root.path().into());
        for name in [
            "HOME",
            "HARNESS_HOME",
            "HARNESS_CONFIG",
            "HARNESS_TUI_CONFIG",
        ] {
            context = context.apply_env_var(name, None);
        }
        let replacement = "x".repeat(2 * 1024 * 1024 + 1);
        for raw in [
            r#"{"instructions":"${VALUE}${VALUE}"}"#,
            r#"{"instructions":["${VALUE}","${VALUE}"]}"#,
        ] {
            context.runtime_content = Some(raw.into());
            let error =
                load_resolved_config_with_lookup(None, &context, &|_| Some(replacement.clone()))
                    .err()
                    .ok_or("oversized substituted configuration accepted")?;
            assert!(error.to_string().contains("4 MiB"));
        }
        Ok(())
    }

    #[test]
    fn config_rejects_unknown_fields_and_preserves_model_and_permission_intent(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let read_policy = crate::perm::PermissionPolicy::from_rules(
            default_read_env_permission_rules()
                .into_iter()
                .map(|rule| crate::perm::PermissionRule {
                    permission: "read".into(),
                    pattern: rule.selector,
                    action: rule.mode,
                })
                .collect(),
        )?;
        for (path, mode) in [
            ("nested/deployment.env", PermissionMode::Ask),
            (".env.local", PermissionMode::Ask),
            ("nested/.env.example", PermissionMode::Allow),
            ("source.rs", PermissionMode::Allow),
        ] {
            assert_eq!(read_policy.check("read", path, None), mode, "{path}");
        }
        assert!(load_config_from_str("{ runtme: {} }").is_err());
        let public_defaults = load_config_from_str("{}")?;
        [
            "spawn_subagent",
            "get_command_or_subagent_output",
            "wait_commands_or_subagents",
            "kill_command_or_subagent",
            "send_subagent_message",
        ]
        .into_iter()
        .for_each(|tool| {
            assert!(public_defaults.agents["default"]
                .tools
                .iter()
                .any(|name| name == tool));
        });
        ["task", "background_output", "background_cancel"]
            .into_iter()
            .for_each(|tool| {
                assert!(!public_defaults.agents["default"]
                    .tools
                    .iter()
                    .any(|name| name == tool));
            });
        let aliases = load_config_from_str(
            "{agent:{default:{tools:['task','get_task_output','wait_tasks','kill_task']}}}",
        )?;
        assert_eq!(
            aliases.agents["default"].tools,
            [
                "spawn_subagent",
                "get_command_or_subagent_output",
                "wait_commands_or_subagents",
                "kill_command_or_subagent",
            ]
        );
        for settings in [
            serde_json::json!({"command":[]}),
            serde_json::json!({"command":["/bin/sh", ""]}),
            serde_json::json!({"cwd":"../outside"}),
            serde_json::json!({"timeout_ms":0}),
            serde_json::json!({"timeout_ms":300001}),
            serde_json::json!({"env":{"HARNESS_HOOK_EVENT":"forged"}}),
            serde_json::json!({"env":{"INVALID=NAME":"value"}}),
        ] {
            let mut hook = serde_json::json!({"event":"run_started","command":["/usr/bin/true"]});
            if let (Some(hook), Some(settings)) = (hook.as_object_mut(), settings.as_object()) {
                hook.extend(settings.clone());
            }
            assert!(load_config_from_str(
                &serde_json::json!({"hooks":{"lifecycle":[hook]}}).to_string()
            )
            .is_err());
        }
        assert!(
            load_config_from_str("{ runtime: { compaction: { threshold_percent: 101 } } }")
                .is_err()
        );
        let config = load_config_from_str(
            r#"{
            provider: { local: { type: 'openai_compatible', options: {baseURL:'http://localhost:8000/v1'}, models: { test: {name:'Test',limit:{context:8192,input:4096,output:2048},variants:{high:{}}} } } },
            model: 'local/test', agent: {default: {variant: 'high'}},
            permission: {bash: {'git *':'allow','*':'deny','git status':'ask'}},
        }"#,
        )?;
        assert_eq!(config.agents["default"].model_ref, "local:test");
        assert!(config.runtime.compaction.mid_turn_enabled);
        assert!(
            load_config_from_str("{runtime:{compaction:{}}}")?
                .runtime
                .compaction
                .mid_turn_enabled
        );
        assert_eq!(
            config
                .permissions
                .rules
                .shell
                .iter()
                .map(|r| r.mode)
                .collect::<Vec<_>>(),
            vec![
                PermissionMode::Allow,
                PermissionMode::Deny,
                PermissionMode::Ask
            ]
        );
        let ProviderConfig::OpenAiCompatible(provider) = &config.providers["local"] else {
            return Err("wrong backend".into());
        };
        assert_eq!(provider.base_url, "http://localhost:8000/v1");
        assert!(
            load_config_from_str("{provider:{x:{type:'openai_compatible',timeoutMs:0}}}").is_err()
        );
        assert_eq!(
            load_config_from_str("{model:'missing:model'}")?.agents["default"].model_ref,
            "missing:model"
        );
        assert!(load_config_from_str(
            "{provider:{local:{type:'openai_compatible',models:{test:{}}}},model:'missing:model'}",
        )
        .is_err());
        assert!(load_config_from_str("{hooks:{lifecycle:[{command:['echo']}]}}").is_err());
        Ok(())
    }
}
