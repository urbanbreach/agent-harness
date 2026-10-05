use super::PromptFamily;
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct PromptSource {
    pub configured: Option<String>,
    pub suffix: String,
}
impl PromptSource {
    pub fn resolve(&self, profile: &str, family: PromptFamily, workspace: &Path) -> String {
        let explicit = configured_prompt_override(profile, self.configured.as_deref());
        let mut prompt = resolve_prompt(family, explicit, workspace).0;
        if explicit.is_none()
            && let Some(role) = shipped_agent_prompt(profile)
        {
            prompt.push_str(&format!("\n\n{role}"));
        }
        prompt.push_str(&self.suffix);
        prompt
    }
}

pub struct PromptFamilyAssetStatus {
    pub family: &'static str,
    pub status: &'static str,
    pub source: &'static str,
    pub path: Option<PathBuf>,
    pub warning: Option<String>,
}
impl PromptFamily {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reasoning => "reasoning",
            Self::Codex => "codex",
            Self::Gpt6 => "gpt-6",
            Self::Gpt => "gpt",
            Self::Meta => "meta",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::Kimi => "kimi",
            Self::Default => "default",
        }
    }
    pub fn bundled_prompt(self) -> &'static str {
        match self {
            Self::Reasoning => {
                include_str!("../../../../.agent-harness/prompt-families/reasoning.md")
            }
            Self::Codex => include_str!("../../../../.agent-harness/prompt-families/codex.md"),
            Self::Gpt6 => include_str!("../../../../.agent-harness/prompt-families/gpt-6.md"),
            Self::Gpt => include_str!("../../../../.agent-harness/prompt-families/gpt.md"),
            Self::Meta => include_str!("../../../../.agent-harness/prompt-families/meta.md"),
            Self::Anthropic => {
                include_str!("../../../../.agent-harness/prompt-families/anthropic.md")
            }
            Self::Gemini => include_str!("../../../../.agent-harness/prompt-families/gemini.md"),
            Self::Kimi => include_str!("../../../../.agent-harness/prompt-families/kimi.md"),
            Self::Default => include_str!("../../../../.agent-harness/prompt-families/default.md"),
        }
    }
}
pub fn shipped_agent_prompt(profile: &str) -> Option<&'static str> {
    let source = match profile {
        "default" => include_str!("../../../../.agent-harness/agents/default.md"),
        "general" => include_str!("../../../../.agent-harness/agents/general.md"),
        "explore" => include_str!("../../../../.agent-harness/agents/explore.md"),
        "librarian" => include_str!("../../../../.agent-harness/agents/librarian.md"),
        _ => return None,
    };
    Some(
        source
            .strip_prefix("---\n")
            .and_then(|text| text.split_once("\n---"))
            .map_or(source, |(_, body)| body)
            .trim(),
    )
}
pub fn configured_prompt_override<'a>(profile: &str, prompt: Option<&'a str>) -> Option<&'a str> {
    prompt.filter(|prompt| {
        !prompt.trim().is_empty() && Some(prompt.trim()) != shipped_agent_prompt(profile)
    })
}
pub fn effective_prompt_status(
    family: PromptFamily,
    configured: Option<&str>,
    workspace: &Path,
) -> PromptFamilyAssetStatus {
    resolve_prompt(family, configured, workspace).1
}
pub fn resolve_prompt(
    family: PromptFamily,
    configured: Option<&str>,
    workspace: &Path,
) -> (String, PromptFamilyAssetStatus) {
    let mut status = PromptFamilyAssetStatus {
        family: family.as_str(),
        status: "ready",
        source: "bundled_asset",
        path: None,
        warning: None,
    };
    if let Some(prompt) = configured.filter(|text| !text.trim().is_empty()) {
        status.source = "configured_prompt";
        return (prompt.into(), status);
    }
    let path = workspace
        .join(".agent-harness/prompt-families")
        .join(format!("{}.md", family.as_str()));
    let mut custom = String::new();
    match crate::store::open_private_file(&path)
        .and_then(|file| file.take(1_048_577).read_to_string(&mut custom))
    {
        Ok(_) if !custom.trim().is_empty() && custom.len() <= 1_048_576 => {
            if custom.trim() != family.bundled_prompt().trim() {
                status.source = "data_asset";
                status.path = Some(path);
                return (custom, status);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => {
            status.warning = Some("Prompt override is empty, unreadable, or larger than 1 MiB; using the bundled prompt.".into());
        }
    }
    (family.bundled_prompt().into(), status)
}
