use super::binding::{format_key_binding, format_key_binding_harness, KeyBinding};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeySequence {
    Single(KeyBinding),
    Leader(KeyBinding),
}

impl KeySequence {
    pub const fn single(binding: KeyBinding) -> Self {
        Self::Single(binding)
    }

    pub const fn leader(second: KeyBinding) -> Self {
        Self::Leader(second)
    }

    pub fn parse_config(input: &str) -> Result<Self, String> {
        let trimmed = input.trim();
        let lower = trimmed.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("<leader>") {
            let key = rest
                .trim()
                .trim_start_matches('+')
                .trim_start_matches('-')
                .trim();
            if key.is_empty() {
                return Err(format!("missing key after <leader> in `{trimmed}`"));
            }
            return Ok(Self::Leader(key.parse()?));
        }

        Ok(Self::Single(trimmed.parse()?))
    }

    pub fn display(self, leader: KeyBinding) -> String {
        match self {
            Self::Single(binding) => format_key_binding(&binding),
            Self::Leader(binding) => {
                format!(
                    "{} {}",
                    format_key_binding(&leader),
                    format_key_binding(&binding)
                )
            }
        }
    }

    pub fn harness_display(self, leader: KeyBinding) -> String {
        match self {
            Self::Single(binding) => format_key_binding_harness(&binding),
            Self::Leader(binding) => {
                format!(
                    "{} {}",
                    format_key_binding_harness(&leader),
                    format_key_binding_harness(&binding)
                )
            }
        }
    }
}
