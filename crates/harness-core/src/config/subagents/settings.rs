use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SubagentsConfig {
    pub enabled: bool,
    pub max_depth: Option<i64>,
    pub max_concurrent: Option<i64>,
    pub sampling_limit: Option<i64>,
    pub limit_behavior: Option<String>,
    pub models: BTreeMap<String, String>,
    pub toggle: BTreeMap<String, bool>,
    pub roles: BTreeMap<String, SubagentRole>,
    pub personas: BTreeMap<String, SubagentPersona>,
}

impl Default for SubagentsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_depth: None,
            max_concurrent: None,
            sampling_limit: None,
            limit_behavior: None,
            models: BTreeMap::new(),
            toggle: BTreeMap::new(),
            roles: BTreeMap::new(),
            personas: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SubagentFeaturesConfig {
    pub active_agent_messages: Option<bool>,
    pub subagent_model_inheritance: Option<bool>,
    pub subagent_worktree_snapshot: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubagentFeatureRequirements {
    pub active_agent_messages: Option<bool>,
    pub subagent_model_inheritance: Option<bool>,
    pub subagent_worktree_snapshot: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubagentRemoteConfig {
    /// Retained to make the deliberately ignored remote gate explicit.
    pub enabled: Option<bool>,
    pub max_depth: Option<u32>,
    pub max_concurrent: Option<u32>,
    pub sampling_limit: Option<u32>,
    pub limit_behavior: Option<String>,
    pub active_agent_messages: Option<bool>,
    pub subagent_model_inheritance: Option<bool>,
    pub subagent_worktree_snapshot: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubagentLimitBehavior {
    #[default]
    Queue,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentRuntimeConfig {
    pub enabled: bool,
    pub max_depth: u32,
    pub max_concurrent: usize,
    pub sampling_limit: usize,
    pub limit_behavior: SubagentLimitBehavior,
    pub messaging_enabled: bool,
    pub model_inheritance: bool,
    pub worktree_snapshot: bool,
    pub models: BTreeMap<String, String>,
    pub toggle: BTreeMap<String, bool>,
    pub roles: BTreeMap<String, SubagentRole>,
    pub personas: BTreeMap<String, SubagentPersona>,
    pub warnings: Vec<String>,
}

impl Default for SubagentRuntimeConfig {
    fn default() -> Self {
        SubagentsConfig::default().resolve_with_lookup(
            None,
            &SubagentFeaturesConfig::default(),
            &SubagentRemoteConfig::default(),
            &SubagentFeatureRequirements::default(),
            &|_| None,
        )
    }
}

fn bool_env(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn positive_count(
    name: &str,
    env: Option<String>,
    config: Option<i64>,
    remote: Option<u32>,
    default: usize,
    warnings: &mut Vec<String>,
) -> usize {
    let environment = env.and_then(|value| {
        let parsed = value
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|value| *value > 0);
        if parsed.is_none() {
            warnings.push(format!("invalid {name}; ignoring environment value"));
        }
        parsed
    });
    if let Some(count) = environment {
        return count;
    }
    let Some(count) = config.or_else(|| remote.map(i64::from)) else {
        return default;
    };
    if count < 1 {
        warnings.push(format!("{name} below 1; clamping to 1"));
    }
    usize::try_from(count.max(1)).unwrap_or(usize::MAX)
}

impl SubagentsConfig {
    /// Local enablement ignores remote enablement. Limits resolve env > config > remote > defaults.
    /// Features resolve managed requirements > env > effective layered config > remote > false.
    pub fn resolve_with_lookup(
        &self,
        cli_enabled: Option<bool>,
        features: &SubagentFeaturesConfig,
        remote: &SubagentRemoteConfig,
        requirements: &SubagentFeatureRequirements,
        lookup: &dyn Fn(&str) -> Option<String>,
    ) -> SubagentRuntimeConfig {
        let mut warnings = Vec::new();
        let depth = lookup("HARNESS_SUBAGENTS_MAX_DEPTH")
            .and_then(|value| {
                let parsed = value.trim().parse::<i64>().ok();
                if parsed.is_none() {
                    warnings.push(
                        "invalid HARNESS_SUBAGENTS_MAX_DEPTH; ignoring environment value".into(),
                    );
                }
                parsed
            })
            .or(self.max_depth)
            .or_else(|| remote.max_depth.map(i64::from))
            .unwrap_or(2);
        if !(1..=i64::from(u32::MAX)).contains(&depth) {
            warnings.push("subagents max_depth outside 1..u32::MAX; clamping".into());
        }
        let depth = depth.clamp(1, i64::from(u32::MAX));
        let max_concurrent = positive_count(
            "HARNESS_MAX_CONCURRENT_SUBAGENTS",
            lookup("HARNESS_MAX_CONCURRENT_SUBAGENTS"),
            self.max_concurrent,
            remote.max_concurrent,
            32,
            &mut warnings,
        );
        let sampling_limit = positive_count(
            "HARNESS_SUBAGENT_SAMPLING_LIMIT",
            lookup("HARNESS_SUBAGENT_SAMPLING_LIMIT"),
            self.sampling_limit,
            remote.sampling_limit,
            max_concurrent,
            &mut warnings,
        );
        if sampling_limit > 512 {
            warnings.push("subagent sampling limit above 512; clamping".into());
        }
        let sampling_limit = sampling_limit.min(512);
        let limit_behavior = [
            lookup("HARNESS_SUBAGENT_LIMIT_BEHAVIOR"),
            self.limit_behavior.clone(),
            remote.limit_behavior.clone(),
        ]
        .into_iter()
        .flatten()
        .find_map(|value| {
            if value.eq_ignore_ascii_case("queue") {
                Some(SubagentLimitBehavior::Queue)
            } else if value.eq_ignore_ascii_case("fail") {
                Some(SubagentLimitBehavior::Fail)
            } else {
                warnings.push(format!(
                    "invalid subagent limit_behavior \"{value}\"; ignoring"
                ));
                None
            }
        })
        .unwrap_or_default();
        let feature =
            |name: &str, required: Option<bool>, local: Option<bool>, remote: Option<bool>| {
                required
                    .or_else(|| lookup(name).as_deref().and_then(bool_env))
                    .or(local)
                    .or(remote)
                    .unwrap_or(false)
            };
        SubagentRuntimeConfig {
            enabled: cli_enabled
                .or_else(|| lookup("HARNESS_SUBAGENTS").as_deref().and_then(bool_env))
                .unwrap_or(self.enabled),
            max_depth: u32::try_from(depth).unwrap_or(u32::MAX),
            max_concurrent,
            sampling_limit,
            limit_behavior,
            messaging_enabled: feature(
                "HARNESS_ACTIVE_AGENT_MESSAGES",
                requirements.active_agent_messages,
                features.active_agent_messages,
                remote.active_agent_messages,
            ),
            model_inheritance: feature(
                "HARNESS_SUBAGENT_MODEL_INHERITANCE",
                requirements.subagent_model_inheritance,
                features.subagent_model_inheritance,
                remote.subagent_model_inheritance,
            ),
            worktree_snapshot: feature(
                "HARNESS_SUBAGENT_WORKTREE_SNAPSHOT",
                requirements.subagent_worktree_snapshot,
                features.subagent_worktree_snapshot,
                remote.subagent_worktree_snapshot,
            ),
            models: self.models.clone(),
            toggle: self.toggle.clone(),
            roles: self.roles.clone(),
            personas: self.personas.clone(),
            warnings,
        }
    }
}
