use std::collections::BTreeMap;

use crate::config::{CategoryPermissions, HarnessConfig, PermissionMode};
use crate::tool::ToolCapability;

const DEFAULT_ASK_TIMEOUT_MS: u64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionKind {
    EditFs,
    Shell,
    Network,
}

impl PermissionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EditFs => "edit_fs",
            Self::Shell => "shell",
            Self::Network => "network",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny,
    Ask {
        timeout_ms: u64,
        default_decision: PermissionDecision,
    },
}

#[derive(Debug, Clone)]
pub struct PermissionPolicy {
    defaults: DefaultPermissionModes,
    category_overrides: BTreeMap<String, CategoryPermissions>,
    ask_timeout_ms: u64,
}

#[derive(Debug, Clone)]
struct DefaultPermissionModes {
    edit: PermissionMode,
    shell: PermissionMode,
    network: PermissionMode,
}

impl PermissionPolicy {
    pub fn from_config(config: &HarnessConfig) -> Self {
        let category_overrides = config
            .categories
            .iter()
            .filter_map(|(name, category)| {
                category
                    .permissions
                    .clone()
                    .map(|permissions| (name.clone(), permissions))
            })
            .collect();

        Self {
            defaults: DefaultPermissionModes {
                edit: config.permissions.edit.clone(),
                shell: config.permissions.shell.clone(),
                network: config.permissions.network.clone(),
            },
            category_overrides,
            ask_timeout_ms: DEFAULT_ASK_TIMEOUT_MS,
        }
    }

    pub fn new(edit: PermissionMode, shell: PermissionMode, network: PermissionMode) -> Self {
        Self {
            defaults: DefaultPermissionModes {
                edit,
                shell,
                network,
            },
            category_overrides: BTreeMap::new(),
            ask_timeout_ms: DEFAULT_ASK_TIMEOUT_MS,
        }
    }

    pub fn with_ask_timeout_ms(mut self, ask_timeout_ms: u64) -> Self {
        self.ask_timeout_ms = ask_timeout_ms;
        self
    }

    pub fn with_category_override(
        mut self,
        category: impl Into<String>,
        permissions: CategoryPermissions,
    ) -> Self {
        self.category_overrides.insert(category.into(), permissions);
        self
    }

    pub fn evaluate(&self, category: Option<&str>, kind: PermissionKind) -> PolicyDecision {
        let effective_mode = category
            .and_then(|name| self.category_overrides.get(name))
            .and_then(|permissions| mode_for_kind(permissions, kind).cloned())
            .unwrap_or_else(|| self.default_mode(kind).clone());

        match effective_mode {
            PermissionMode::Allow => PolicyDecision::Allow,
            PermissionMode::Deny => PolicyDecision::Deny,
            PermissionMode::Ask => PolicyDecision::Ask {
                timeout_ms: self.ask_timeout_ms,
                default_decision: PermissionDecision::Deny,
            },
        }
    }

    fn default_mode(&self, kind: PermissionKind) -> &PermissionMode {
        match kind {
            PermissionKind::EditFs => &self.defaults.edit,
            PermissionKind::Shell => &self.defaults.shell,
            PermissionKind::Network => &self.defaults.network,
        }
    }
}

impl Default for PermissionPolicy {
    fn default() -> Self {
        Self::new(
            PermissionMode::Allow,
            PermissionMode::Allow,
            PermissionMode::Allow,
        )
    }
}

pub fn permission_kind_for_tool(tool_id: &str) -> Option<PermissionKind> {
    if tool_id.starts_with("edit.") {
        return Some(PermissionKind::EditFs);
    }

    if tool_id.starts_with("shell.") {
        return Some(PermissionKind::Shell);
    }

    if tool_id.starts_with("network.") || tool_id.starts_with("net.") {
        return Some(PermissionKind::Network);
    }

    None
}

pub fn permission_kind_for_capability(capability: ToolCapability) -> Option<PermissionKind> {
    match capability {
        ToolCapability::EditFs => Some(PermissionKind::EditFs),
        ToolCapability::Shell => Some(PermissionKind::Shell),
        ToolCapability::Network => Some(PermissionKind::Network),
        ToolCapability::ReadFs | ToolCapability::SpawnAgent => None,
    }
}

fn mode_for_kind(
    permissions: &CategoryPermissions,
    kind: PermissionKind,
) -> Option<&PermissionMode> {
    match kind {
        PermissionKind::EditFs => permissions.edit.as_ref(),
        PermissionKind::Shell => permissions.shell.as_ref(),
        PermissionKind::Network => permissions.network.as_ref(),
    }
}

#[cfg(test)]
mod tests {
    use super::{PermissionDecision, PermissionKind, PermissionPolicy, PolicyDecision};
    use crate::config::{CategoryPermissions, PermissionMode};

    #[test]
    fn evaluate_uses_global_defaults() {
        let policy = PermissionPolicy::new(
            PermissionMode::Ask,
            PermissionMode::Allow,
            PermissionMode::Deny,
        )
        .with_ask_timeout_ms(1_234);

        assert_eq!(
            policy.evaluate(None, PermissionKind::Shell),
            PolicyDecision::Allow
        );
        assert_eq!(
            policy.evaluate(None, PermissionKind::Network),
            PolicyDecision::Deny
        );
        assert_eq!(
            policy.evaluate(None, PermissionKind::EditFs),
            PolicyDecision::Ask {
                timeout_ms: 1_234,
                default_decision: PermissionDecision::Deny,
            }
        );
    }

    #[test]
    fn evaluate_uses_category_override_when_present() {
        let policy = PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Deny,
            PermissionMode::Deny,
        )
        .with_category_override(
            "deep",
            CategoryPermissions {
                edit: Some(PermissionMode::Allow),
                shell: Some(PermissionMode::Ask),
                network: None,
            },
        )
        .with_ask_timeout_ms(55);

        assert_eq!(
            policy.evaluate(Some("deep"), PermissionKind::EditFs),
            PolicyDecision::Allow
        );
        assert_eq!(
            policy.evaluate(Some("deep"), PermissionKind::Shell),
            PolicyDecision::Ask {
                timeout_ms: 55,
                default_decision: PermissionDecision::Deny,
            }
        );
        assert_eq!(
            policy.evaluate(Some("deep"), PermissionKind::Network),
            PolicyDecision::Deny
        );
    }
}
