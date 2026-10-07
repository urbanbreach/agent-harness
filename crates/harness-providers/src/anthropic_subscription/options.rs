//! Lane settings and SDK query options (senpi `settings.ts`, `options.ts`, `system-prompt.ts`).
use super::errors::{
    override_system_prompt_guidance, preset_append_deprecation_guidance, LaneError,
};
use super::prompt::LaneContext;
use super::protocol::{QueryOptions, SystemPrompt, Thinking};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemPromptMode {
    PresetAppend,
    Full,
    Override,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeMode {
    Auto,
    Off,
}
/// How a managed account reaches Claude Code: `CLAUDE_CODE_OAUTH_TOKEN`, a per-account
/// `CLAUDE_CONFIG_DIR`, or the host's own `claude` login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenInjection {
    OauthSlots,
    ConfigDir,
    Ambient,
}
impl TokenInjection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OauthSlots => "oauth-slots",
            Self::ConfigDir => "config-dir",
            Self::Ambient => "ambient",
        }
    }
}

/// `anthropicSubscriptionProvider` settings; `None` means unset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnthropicSubscriptionSettings {
    /// Explicit opt-in for the ambient (host Claude CLI) lane; stored accounts and
    /// `CLAUDE_CODE_OAUTH_TOKEN*` are opt-ins in themselves.
    pub enabled: Option<bool>,
    pub append_system_prompt: Option<bool>,
    pub system_prompt_mode: Option<SystemPromptMode>,
    pub system_prompt_file: Option<String>,
    pub resume_mode: Option<ResumeMode>,
    pub setting_sources: Option<Vec<String>>,
    pub strict_mcp_config: Option<bool>,
    pub pinned_account: Option<String>,
    pub token_injection: Option<TokenInjection>,
    /// Set when the mode came from the environment.
    pub system_prompt_mode_from_env: bool,
}

pub fn parse_system_prompt_mode(value: &str) -> Option<SystemPromptMode> {
    match value {
        "preset-append" => Some(SystemPromptMode::PresetAppend),
        "full" => Some(SystemPromptMode::Full),
        "override" => Some(SystemPromptMode::Override),
        _ => None,
    }
}
pub fn parse_resume_mode(value: &str) -> Option<ResumeMode> {
    match value {
        "auto" => Some(ResumeMode::Auto),
        "off" => Some(ResumeMode::Off),
        _ => None,
    }
}
pub fn parse_token_injection(value: &str) -> Option<TokenInjection> {
    match value {
        "oauth-slots" => Some(TokenInjection::OauthSlots),
        "config-dir" => Some(TokenInjection::ConfigDir),
        "ambient" => Some(TokenInjection::Ambient),
        _ => None,
    }
}
pub fn parse_setting_sources(values: &[String]) -> Option<Vec<String>> {
    values
        .iter()
        .all(|s| matches!(s.as_str(), "user" | "project" | "local"))
        .then(|| values.to_vec())
}

impl AnthropicSubscriptionSettings {
    /// Environment values take precedence over configuration.
    pub fn with_env(mut self, env: &dyn Fn(&str) -> Option<String>) -> Self {
        let var = |name: &str| env(&format!("HARNESS_CLAUDE_SDK_OAUTH_{name}"));
        if let Some(enabled) = var("ENABLED").and_then(|v| match v.to_lowercase().as_str() {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        }) {
            self.enabled = Some(enabled);
        }
        if let Some(mode) = var("SYSTEM_PROMPT_MODE")
            .as_deref()
            .and_then(parse_system_prompt_mode)
        {
            self.system_prompt_mode = Some(mode);
            self.system_prompt_mode_from_env = true;
        }
        if let Some(file) = var("SYSTEM_PROMPT_FILE").filter(|v| !v.is_empty()) {
            self.system_prompt_file = Some(file);
        }
        if let Some(mode) = var("RESUME").as_deref().and_then(parse_resume_mode) {
            self.resume_mode = Some(mode);
        }
        if let Some(lane) = var("TOKEN_INJECTION")
            .as_deref()
            .and_then(parse_token_injection)
        {
            self.token_injection = Some(lane);
        }
        if let Some(sources) = var("SETTING_SOURCES") {
            let parsed = if sources.is_empty() {
                Some(Vec::new())
            } else {
                parse_setting_sources(
                    &sources
                        .split(',')
                        .map(|s| s.trim().to_owned())
                        .collect::<Vec<_>>(),
                )
            };
            if parsed.is_some() {
                self.setting_sources = parsed;
            }
        }
        if let Some(account) = var("PINNED_ACCOUNT").filter(|v| !v.is_empty()) {
            self.pinned_account = Some(account);
        }
        self
    }

    /// `(mode, conflict)`: an explicit mode wins over the legacy `appendSystemPrompt`.
    pub fn resolve_system_prompt_mode(&self) -> (SystemPromptMode, bool) {
        if let Some(mode) = self.system_prompt_mode {
            return (mode, self.append_system_prompt.is_some());
        }
        match self.append_system_prompt {
            Some(true) => (SystemPromptMode::Full, false),
            Some(false) => (SystemPromptMode::PresetAppend, false),
            None => (SystemPromptMode::Full, false),
        }
    }
}

const ADAPTIVE_THINKING_MODEL_MARKERS: [&str; 12] = [
    "opus-4-6",
    "opus-4.6",
    "opus-4-7",
    "opus-4.7",
    "opus-4-8",
    "opus-4.8",
    "opus-5",
    "sonnet-4-6",
    "sonnet-4.6",
    "sonnet-5",
    "fable-5",
    "mythos-5",
];
const NATIVE_XHIGH_EFFORT_MODEL_MARKERS: [&str; 6] = [
    "opus-4-7", "opus-4-8", "opus-5", "sonnet-5", "fable-5", "mythos-5",
];

fn includes_marker(model: &str, markers: &[&str]) -> bool {
    let id = model.to_lowercase();
    markers.iter().any(|marker| id.contains(marker))
}
pub fn supports_adaptive_thinking(model: &str) -> bool {
    includes_marker(model, &ADAPTIVE_THINKING_MODEL_MARKERS)
}

/// Harness reasoning effort as senpi's `ThinkingLevel`; `None` turns reasoning off.
pub fn thinking_level(effort: Option<&str>) -> Option<&str> {
    effort.filter(|e| matches!(*e, "minimal" | "low" | "medium" | "high" | "xhigh" | "max"))
}

pub fn map_thinking_level_to_effort(model: &str, level: &str) -> &'static str {
    match level {
        "minimal" | "low" => "low",
        "medium" => "medium",
        "high" => "high",
        "xhigh" if includes_marker(model, &NATIVE_XHIGH_EFFORT_MODEL_MARKERS) => "xhigh",
        _ => "max",
    }
}

pub fn map_thinking_tokens(level: &str) -> u32 {
    match level {
        "minimal" => 2048,
        "low" => 8192,
        "medium" => 16384,
        "high" | "xhigh" => 31999,
        _ => 63999,
    }
}

fn find_agents_md_in_parents(cwd: &Path) -> Option<PathBuf> {
    let mut current = std::path::absolute(cwd).ok()?;
    loop {
        let candidate = current.join("AGENTS.md");
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn sanitize_agents_content(content: &str) -> String {
    let rules = [
        (r"(?i)~/harness\b", "~/.claude"),
        (r"(^|[\s'`]|\x22)\.harness/", "${1}.claude/"),
        (r"(?i)\bharness\b", "environment"),
    ];
    rules
        .iter()
        .fold(content.to_owned(), |text, (pattern, replacement)| {
            regex::Regex::new(pattern).map_or(text.clone(), |re| {
                re.replace_all(&text, *replacement).into_owned()
            })
        })
}

fn extract_agents_append(cwd: &Path, agent_dir: Option<&Path>) -> Option<String> {
    let path = find_agents_md_in_parents(cwd).or_else(|| agent_dir.map(|d| d.join("AGENTS.md")))?;
    let content = sanitize_agents_content(std::fs::read_to_string(path).ok()?.trim());
    (!content.is_empty()).then(|| format!("# CLAUDE.md\n\n{content}"))
}

fn extract_skills_append(system_prompt: Option<&str>) -> Option<String> {
    let prompt = system_prompt?;
    let marker = "The following skills provide specialized instructions for specific tasks.";
    let start = prompt.find(marker)?;
    let end = prompt[start..].find("</available_skills>")? + start;
    Some(
        prompt[start..end + "</available_skills>".len()]
            .trim()
            .to_owned(),
    )
}

pub fn load_override_system_prompt(path: Option<&str>) -> Result<String, LaneError> {
    let Some(path) = path else {
        return Err(LaneError::Message(override_system_prompt_guidance(
            None,
            "the path is not configured",
        )));
    };
    let content = std::fs::read_to_string(path).map_err(|e| {
        LaneError::Message(override_system_prompt_guidance(Some(path), &e.to_string()))
    })?;
    if content.trim().is_empty() {
        return Err(LaneError::Message(override_system_prompt_guidance(
            Some(path),
            "the file is empty",
        )));
    }
    Ok(content)
}

pub struct QueryOptionsInput<'a> {
    pub model: &'a str,
    pub context: &'a LaneContext,
    pub reasoning: Option<&'a str>,
    pub tool_less: bool,
    pub cwd: &'a Path,
    pub agent_dir: Option<&'a Path>,
    pub settings: &'a AnthropicSubscriptionSettings,
    pub auth_lane: TokenInjection,
    pub tools: &'a [String],
    pub executable: &'a Path,
    pub session_id: Option<&'a str>,
}

/// `buildAnthropicSubscriptionQueryOptions`; guidance diagnostics are returned alongside.
pub fn build_query_options(
    input: &QueryOptionsInput<'_>,
) -> Result<(QueryOptions, Option<String>), LaneError> {
    let settings = input.settings;
    let append_system_prompt = settings.append_system_prompt != Some(false);
    let (mode, conflict) = settings.resolve_system_prompt_mode();
    let guidance = input.session_id.and_then(|id| {
        preset_append_deprecation_guidance(mode == SystemPromptMode::PresetAppend, conflict, id)
    });
    let system_prompt = match mode {
        SystemPromptMode::PresetAppend => {
            let append: Vec<String> = [
                extract_agents_append(input.cwd, input.agent_dir),
                extract_skills_append(input.context.system_prompt.as_deref()),
            ]
            .into_iter()
            .flatten()
            .collect();
            SystemPrompt::Preset {
                append: (!append.is_empty()).then(|| append.join("\n\n")),
            }
        }
        SystemPromptMode::Override => SystemPrompt::Custom(load_override_system_prompt(
            settings.system_prompt_file.as_deref(),
        )?),
        SystemPromptMode::Full => {
            SystemPrompt::Custom(input.context.system_prompt.clone().unwrap_or_default())
        }
    };
    let empty_tool_context = input.context.tools.as_ref().is_none_or(Vec::is_empty);
    let strict = input.tool_less || settings.strict_mcp_config.unwrap_or(!append_system_prompt);
    let setting_sources = settings.setting_sources.clone().unwrap_or_else(|| {
        if mode == SystemPromptMode::PresetAppend && input.auth_lane == TokenInjection::Ambient {
            vec!["user".into(), "project".into()]
        } else {
            Vec::new()
        }
    });
    let mut options = QueryOptions {
        cwd: input.cwd.to_path_buf(),
        model: input.model.into(),
        tools: if input.tool_less || empty_tool_context {
            Vec::new()
        } else {
            input.tools.to_vec()
        },
        permission_mode: "dontAsk".into(),
        include_partial_messages: true,
        system_prompt,
        settings: json!({"autoCompactEnabled": true}),
        setting_sources,
        executable: input.executable.to_path_buf(),
        max_turns: input.tool_less.then_some(1),
        extra_args: Vec::new(),
        thinking: None,
        effort: None,
        env: BTreeMap::new(),
        custom_tools: Vec::new(),
        resume: None,
        resume_session_at: None,
        fork_session: false,
        session_id: None,
    };
    if strict {
        options.set_extra_arg("strict-mcp-config", None);
    }
    if let Some(level) = thinking_level(input.reasoning) {
        if supports_adaptive_thinking(input.model) {
            options.thinking = Some(Thinking::Adaptive {
                display: "summarized".into(),
            });
            options.effort = Some(map_thinking_level_to_effort(input.model, level).into());
        } else {
            options.thinking = Some(Thinking::Budget(map_thinking_tokens(level)));
        }
    }
    Ok((options, guidance))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effort_and_thinking_follow_the_model_family() {
        assert_eq!(
            map_thinking_level_to_effort("claude-opus-5-5", "xhigh"),
            "xhigh"
        );
        assert_eq!(
            map_thinking_level_to_effort("claude-sonnet-4-6", "xhigh"),
            "max"
        );
        assert_eq!(
            map_thinking_level_to_effort("claude-opus-5-5", "minimal"),
            "low"
        );
        assert!(!supports_adaptive_thinking("claude-haiku-4-5"));
        assert_eq!(map_thinking_tokens("xhigh"), 31999);
        let settings = AnthropicSubscriptionSettings {
            append_system_prompt: Some(false),
            ..Default::default()
        };
        assert_eq!(
            settings.resolve_system_prompt_mode(),
            (SystemPromptMode::PresetAppend, false)
        );
        let env = |name: &str| {
            (name == "HARNESS_CLAUDE_SDK_OAUTH_SYSTEM_PROMPT_MODE").then(|| "full".to_owned())
        };
        assert_eq!(
            settings.with_env(&env).resolve_system_prompt_mode(),
            (SystemPromptMode::Full, true)
        );
    }
}
