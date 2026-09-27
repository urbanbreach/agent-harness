use crate::config::PermissionSelector;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(schemars::JsonSchema)]
pub enum PermissionAction {
    Allow,
    Ask,
    Deny,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRule {
    pub permission: String,
    pub pattern: PermissionSelector,
    pub action: PermissionAction,
}
pub type PermissionRuleset = Vec<PermissionRule>;

#[derive(Debug, Clone)]
pub struct PermissionPolicy {
    rules: Vec<CompiledRule>,
    fallback: PermissionAction,
    pub(crate) timeout_ms: u64,
}
#[derive(Debug, Clone)]
struct CompiledRule {
    permission: regex::Regex,
    pattern: Matcher,
    action: PermissionAction,
}
#[derive(Debug, Clone)]
enum Matcher {
    Exact(String),
    Prefix(String),
    Glob(regex::Regex),
    Any,
}
impl Matcher {
    fn compile(selector: PermissionSelector) -> Result<Self, regex::Error> {
        let value = match &selector {
            PermissionSelector::Exact(value)
            | PermissionSelector::Prefix(value)
            | PermissionSelector::Glob(value) => value,
            PermissionSelector::CatchAll => return Ok(Self::Any),
        };
        if value.len() > 4096 {
            return Err(regex::Error::Syntax(
                "permission pattern exceeds 4096 bytes".into(),
            ));
        }
        Ok(match selector {
            PermissionSelector::Exact(value) => Self::Exact(value),
            PermissionSelector::Prefix(value) => Self::Prefix(value),
            PermissionSelector::Glob(value) if value == "*" => Self::Any,
            PermissionSelector::Glob(value) => Self::Glob(compile(&value)?),
            PermissionSelector::CatchAll => Self::Any,
        })
    }
    fn is_match(&self, value: &str) -> bool {
        match self {
            Self::Exact(pattern) => value == pattern,
            Self::Prefix(pattern) => value.starts_with(pattern),
            Self::Glob(pattern) => pattern.is_match(value),
            Self::Any => true,
        }
    }
}
impl PermissionPolicy {
    pub fn from_rules(rules: PermissionRuleset) -> Result<Self, regex::Error> {
        let rules = rules
            .into_iter()
            .map(|rule| {
                Ok(CompiledRule {
                    permission: compile(&rule.permission)?,
                    pattern: Matcher::compile(rule.pattern)?,
                    action: rule.action,
                })
            })
            .collect::<Result<_, regex::Error>>()?;
        Ok(Self {
            rules,
            fallback: PermissionAction::Ask,
            timeout_ms: 0,
        })
    }
    pub fn allow_all() -> Self {
        Self {
            rules: Vec::new(),
            fallback: PermissionAction::Allow,
            timeout_ms: 0,
        }
    }
    pub fn with_ask_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }
    pub fn ask_timeout_ms(&self) -> u64 {
        self.timeout_ms
    }
    pub fn check(&self, permission: &str, value: &str, role: Option<&Self>) -> PermissionAction {
        let shared = self.matching(permission, value).unwrap_or(self.fallback);
        let role = role
            .and_then(|r| r.matching(permission, value))
            .unwrap_or(shared);
        match (shared, role) {
            (PermissionAction::Deny, _) | (_, PermissionAction::Deny) => PermissionAction::Deny,
            (PermissionAction::Ask, _) | (_, PermissionAction::Ask) => PermissionAction::Ask,
            _ => PermissionAction::Allow,
        }
    }
    pub(crate) fn always_denies(&self, permission: &str) -> bool {
        let mut denied = self.fallback == PermissionAction::Deny;
        for rule in self
            .rules
            .iter()
            .filter(|rule| rule.permission.is_match(permission))
        {
            if rule.action != PermissionAction::Deny {
                denied = false;
            } else if matches!(rule.pattern, Matcher::Any) {
                denied = true;
            }
        }
        denied
    }
    fn matching(&self, permission: &str, value: &str) -> Option<PermissionAction> {
        self.rules
            .iter()
            .rfind(|r| r.permission.is_match(permission) && r.pattern.is_match(value))
            .map(|r| r.action)
    }
}
impl Default for PermissionPolicy {
    fn default() -> Self {
        Self {
            fallback: PermissionAction::Ask,
            ..Self::allow_all()
        }
    }
}

fn compile(pattern: &str) -> Result<regex::Regex, regex::Error> {
    if pattern.len() > 4096 {
        return Err(regex::Error::Syntax(
            "permission pattern exceeds 4096 bytes".into(),
        ));
    }
    let pattern = regex::escape(pattern)
        .replace("\\*", ".*")
        .replace("\\?", ".");
    regex::Regex::new(&format!("(?s)\\A{pattern}\\z"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perm::*;

    #[test]
    fn permissions_apply_last_match_without_loosening_shared_policy() -> Result<(), regex::Error> {
        let rule = |permission: &str, pattern: &str, action| PermissionRule {
            permission: permission.into(),
            pattern: pattern.into(),
            action,
        };
        let shared = PermissionPolicy::from_rules(vec![
            rule("*", "*", PermissionAction::Allow),
            rule("edit", "secrets/*", PermissionAction::Deny),
            rule("bash", "*", PermissionAction::Ask),
            rule("bash", "git status*", PermissionAction::Allow),
        ])?;
        let role = PermissionPolicy::from_rules(vec![rule("*", "*", PermissionAction::Allow)])?;
        for (kind, input, expected) in [
            ("edit", "src/main.rs", PermissionAction::Allow),
            ("edit", "secrets/token", PermissionAction::Deny),
            ("bash", "git status --short", PermissionAction::Allow),
            ("bash", "rm file", PermissionAction::Ask),
        ] {
            assert_eq!(shared.check(kind, input, Some(&role)), expected);
        }
        let unicode =
            PermissionPolicy::from_rules(vec![rule("read", "目录/?.rs", PermissionAction::Allow)])?;
        assert_eq!(
            unicode.check("read", "目录/文.rs", None),
            PermissionAction::Allow
        );
        assert_eq!(
            unicode.check("read", "目录/文件.rs", None),
            PermissionAction::Ask
        );
        assert!(external_path_prefix_covers("/tmp/work", "/tmp/work/file"));
        assert!(!external_path_prefix_covers(
            "/tmp/work",
            "/tmp/work-else/file"
        ));
        assert!(!external_path_prefix_covers(
            "/tmp/work",
            "/tmp/work/../secret"
        ));
        let literal = PermissionPolicy::from_rules(vec![
            PermissionRule {
                permission: "bash".into(),
                pattern: crate::config::PermissionSelector::Exact("printf '*'".into()),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "bash".into(),
                pattern: crate::config::PermissionSelector::Prefix("echo ?".into()),
                action: PermissionAction::Allow,
            },
        ])?;
        for (command, expected) in [
            ("printf '*'", PermissionAction::Allow),
            ("printf 'secret'", PermissionAction::Ask),
            ("echo ? && pwd", PermissionAction::Allow),
            ("echo x && pwd", PermissionAction::Ask),
        ] {
            assert_eq!(literal.check("bash", command, None), expected);
        }
        #[cfg(unix)]
        {
            let backslash = PermissionPolicy::from_rules(vec![rule(
                "read",
                "dir\\file",
                PermissionAction::Allow,
            )])?;
            assert_eq!(
                backslash.check("read", "dir\\file", None),
                PermissionAction::Allow
            );
            assert_eq!(
                backslash.check("read", "dir/file", None),
                PermissionAction::Ask
            );
        }
        Ok(())
    }
}
