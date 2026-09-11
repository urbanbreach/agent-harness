use super::PromptFamily;
use std::path::{Path, PathBuf};

pub fn shipped_agent_prompt(profile: &str) -> Option<&'static str> {
    let markdown = match profile {
        "default" => include_str!("../../../../.agent-harness/agents/default.md"),
        "explore" => include_str!("../../../../.agent-harness/agents/explore.md"),
        "general" => include_str!("../../../../.agent-harness/agents/general.md"),
        "librarian" => include_str!("../../../../.agent-harness/agents/librarian.md"),
        _ => return None,
    };
    Some(
        markdown
            .strip_prefix("---\n")
            .and_then(|body| body.split_once("\n---"))
            .map_or(markdown, |(_, body)| body)
            .trim(),
    )
}

/// Discovery also loads shipped role files into `system_prompt`. Those are
/// additive role guidance, not user-authored replacements for the model base.
pub fn configured_prompt_override<'a>(profile: &str, prompt: Option<&'a str>) -> Option<&'a str> {
    prompt.filter(|body| shipped_agent_prompt(profile) != Some(body.trim()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptFamilyAssetStatus {
    pub family: &'static str,
    pub status: &'static str,
    pub source: &'static str,
    pub path: Option<PathBuf>,
    pub warning: Option<String>,
}

pub fn effective_prompt_status(
    family: PromptFamily,
    configured_prompt: Option<&str>,
    workspace_root: &Path,
) -> PromptFamilyAssetStatus {
    if configured_prompt.is_some() {
        return PromptFamilyAssetStatus {
            family: family.id(),
            status: "configured",
            source: "configured_prompt",
            path: None,
            warning: None,
        };
    }
    family.resolve_prompt(workspace_root).1
}

impl PromptFamily {
    pub fn bundled_prompt(self) -> &'static str {
        match self {
            Self::Reasoning => {
                include_str!("../../../../.agent-harness/prompt-families/reasoning.md")
            }
            Self::Codex => include_str!("../../../../.agent-harness/prompt-families/codex.md"),
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

    pub fn prompt(self, workspace_root: &Path) -> String {
        let (body, status) = self.resolve_prompt(workspace_root);
        if let Some(warning) = status.warning {
            tracing::warn!(family = self.id(), "{warning}");
        }
        body
    }

    fn resolve_prompt(self, workspace_root: &Path) -> (String, PromptFamilyAssetStatus) {
        let relative =
            Path::new(".agent-harness/prompt-families").join(format!("{}.md", self.id()));
        let mut status = PromptFamilyAssetStatus {
            family: self.id(),
            status: "builtin",
            source: "bundled_prompt",
            path: None,
            warning: None,
        };
        match std::fs::read_to_string(workspace_root.join(&relative)) {
            Ok(body) if !body.trim().is_empty() => {
                if body != self.bundled_prompt() {
                    status.status = "available";
                    status.source = "data_asset";
                    status.path = Some(relative);
                }
                return (body, status);
            }
            Ok(_) => {
                status.warning = Some(format!(
                    "empty prompt-family asset {}; using bundled {} prompt",
                    relative.display(),
                    self.id()
                ))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                status.warning = Some(format!(
                    "cannot read prompt-family asset {}: {error}; using bundled {} prompt",
                    relative.display(),
                    self.id()
                ))
            }
        }
        (self.bundled_prompt().to_string(), status)
    }
}
