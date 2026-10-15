//! Recoverable text elision without I/O or history mutation.
use crate::method::ShakeMode;

mod artifact;
mod detect;

pub use artifact::{artifact_text, exact_savings, placeholder, plan_edits, TextEdit, ToolEdit};
pub use detect::{detect_regions, ShakeDetection};

/// Tokens a detection pass assumes a placeholder costs when gating on savings.
pub const PLACEHOLDER_ESTIMATE: u32 = 16;

#[derive(Debug, Clone, Copy)]
pub struct ShakeConfig {
    /// Tool output newer than this many tokens is kept.
    pub protect_tokens: u32,
    /// Detection returns nothing unless the estimated savings reach this.
    pub min_savings: u32,
    /// Smallest fenced or tagged block worth replacing.
    pub fence_min_tokens: u32,
}
pub const AUTO: ShakeConfig = ShakeConfig {
    protect_tokens: 16000,
    min_savings: 4000,
    fence_min_tokens: 400,
};
pub const MANUAL: ShakeConfig = ShakeConfig {
    protect_tokens: 4000,
    min_savings: 0,
    fence_min_tokens: 400,
};
pub const RESCUE: ShakeConfig = ShakeConfig {
    protect_tokens: 0,
    ..MANUAL
};
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShakeRegion {
    ToolResult {
        index: usize,
        tokens: u32,
        original: String,
        label: String,
    },
    Block {
        index: usize,
        start: usize,
        end: usize,
        tokens: u32,
        original: String,
        label: String,
    },
}
impl ShakeRegion {
    pub(crate) fn data(&self) -> (u32, &str, &str) {
        match self {
            Self::ToolResult {
                tokens,
                original,
                label,
                ..
            }
            | Self::Block {
                tokens,
                original,
                label,
                ..
            } => (*tokens, original, label),
        }
    }

    /// Index of the entry the region lives in.
    pub fn index(&self) -> usize {
        match self {
            Self::ToolResult { index, .. } | Self::Block { index, .. } => *index,
        }
    }

    pub fn tokens(&self) -> u32 {
        self.data().0
    }
}
pub fn summarize_shake(
    mode: ShakeMode,
    tool_results: usize,
    blocks: usize,
    images: usize,
    thinking: usize,
    tokens_freed: u32,
) -> String {
    let plural = |count| if count == 1 { "" } else { "s" };
    match mode {
        ShakeMode::Images if images == 0 => "No images found in this session.".into(),
        ShakeMode::Images => format!(
            "Dropped {images} image{} from this session.",
            plural(images)
        ),
        ShakeMode::Thinking if thinking == 0 => "No thinking blocks found in this session.".into(),
        ShakeMode::Thinking => {
            let freed = if tokens_freed > 0 {
                format!(" (~{tokens_freed} tokens freed)")
            } else {
                String::new()
            };
            format!(
                "Dropped {thinking} thinking block{} from this session{freed}.",
                plural(thinking)
            )
        }
        ShakeMode::Elide => {
            let mut parts = Vec::new();
            if tool_results > 0 {
                parts.push(format!(
                    "{tool_results} tool result{}",
                    plural(tool_results)
                ));
            }
            if blocks > 0 {
                parts.push(format!("{blocks} block{}", plural(blocks)));
            }
            if parts.is_empty() {
                "Nothing to shake.".into()
            } else {
                format!(
                    "Shook {} (~{tokens_freed} tokens freed).",
                    parts.join(" + ")
                )
            }
        }
    }
}

#[cfg(test)]
mod tests;
