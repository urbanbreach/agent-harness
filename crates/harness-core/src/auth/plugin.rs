use crate::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::sync::Arc;

use super::ProviderId;

pub trait AuthPlugin: Send + Sync {
    fn provider_id(&self) -> &ProviderId;
    fn label(&self) -> &str;
    fn description(&self) -> &str;
    fn auth_methods(&self) -> &[AuthMethodSpec];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthMethodSpec {
    OAuthAuto {
        label: String,
        port: u16,
    },
    OAuthCode {
        label: String,
    },
    ApiKey {
        label: String,
    },
    Prompts {
        label: String,
        prompts: Vec<PromptField>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptField {
    pub key: String,
    pub message: String,
    pub placeholder: Option<String>,
    pub field_type: PromptFieldType,
    pub when: Option<PromptCondition>,
    pub options: Vec<PromptOption>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptFieldType {
    Text,
    Select,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCondition {
    pub key: String,
    pub op: PromptOp,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptOp {
    Eq,
    Neq,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptOption {
    pub id: String,
    pub label: String,
}

pub struct AuthPluginRegistry {
    plugins: BTreeMap<ProviderId, Arc<dyn AuthPlugin>>,
}

impl AuthPluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<dyn AuthPlugin>) {
        let id = plugin.provider_id().clone();
        self.plugins.insert(id, plugin);
    }

    pub fn get(&self, provider: &ProviderId) -> Option<&Arc<dyn AuthPlugin>> {
        self.plugins.get(provider)
    }

    pub fn providers(&self) -> Vec<&ProviderId> {
        self.plugins.keys().collect()
    }

    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register(Arc::new(CodexAuthPlugin::new()));
        registry.register(Arc::new(CopilotAuthPlugin::new()));
        registry
    }
}

impl Default for AuthPluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CodexAuthPlugin {
    provider_id: ProviderId,
    methods: Vec<AuthMethodSpec>,
}

impl CodexAuthPlugin {
    pub fn new() -> Self {
        Self {
            provider_id: ProviderId::codex(),
            methods: vec![
                AuthMethodSpec::OAuthAuto {
                    label: "ChatGPT Pro/Plus (browser)".to_string(),
                    port: 1455,
                },
                AuthMethodSpec::OAuthCode {
                    label: "ChatGPT Pro/Plus (headless)".to_string(),
                },
                AuthMethodSpec::ApiKey {
                    label: "Manually enter API Key".to_string(),
                },
            ],
        }
    }
}

impl Default for CodexAuthPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthPlugin for CodexAuthPlugin {
    fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }
    fn label(&self) -> &str {
        "OpenAI"
    }
    fn description(&self) -> &str {
        "ChatGPT Plus/Pro or API key"
    }
    fn auth_methods(&self) -> &[AuthMethodSpec] {
        &self.methods
    }
}

pub struct CopilotAuthPlugin {
    provider_id: ProviderId,
    methods: Vec<AuthMethodSpec>,
}

impl CopilotAuthPlugin {
    pub fn new() -> Self {
        Self {
            provider_id: ProviderId::github_copilot(),
            methods: vec![AuthMethodSpec::Prompts {
                label: "Device login".to_string(),
                prompts: vec![
                    PromptField {
                        key: "deployment".to_string(),
                        message: "Select GitHub deployment type".to_string(),
                        placeholder: None,
                        field_type: PromptFieldType::Select,
                        when: None,
                        options: vec![
                            PromptOption {
                                id: "public".to_string(),
                                label: "GitHub.com".to_string(),
                            },
                            PromptOption {
                                id: "enterprise".to_string(),
                                label: "GitHub Enterprise".to_string(),
                            },
                        ],
                    },
                    PromptField {
                        key: "enterprise_url".to_string(),
                        message: "Enter your GitHub Enterprise URL or domain".to_string(),
                        placeholder: Some("company.ghe.com or https://company.ghe.com".to_string()),
                        field_type: PromptFieldType::Text,
                        when: Some(PromptCondition {
                            key: "deployment".to_string(),
                            op: PromptOp::Eq,
                            value: "enterprise".to_string(),
                        }),
                        options: vec![],
                    },
                ],
            }],
        }
    }
}

impl Default for CopilotAuthPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthPlugin for CopilotAuthPlugin {
    fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }
    fn label(&self) -> &str {
        "GitHub Copilot"
    }
    fn description(&self) -> &str {
        "Device login"
    }
    fn auth_methods(&self) -> &[AuthMethodSpec] {
        &self.methods
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    #[test]
    fn registry_get_codex_returns_plugin() {
        // arrange
        let registry = AuthPluginRegistry::with_builtins();

        // act
        let plugin = registry.get(&ProviderId::codex());

        // assert
        assert!(plugin.is_some());
    }

    #[test]
    fn registry_get_unknown_returns_none() {
        // arrange
        let registry = AuthPluginRegistry::with_builtins();
        let provider = ProviderId::parse("unknown").unwrap_or_abort();

        // act
        let plugin = registry.get(&provider);

        // assert
        assert!(plugin.is_none());
    }

    #[test]
    fn codex_auth_methods_returns_three() {
        // arrange
        let plugin = CodexAuthPlugin::new();

        // act
        let result = plugin.auth_methods().len();

        // assert
        assert_eq!(result, 3);
    }

    #[test]
    fn copilot_auth_methods_returns_one_with_prompts() {
        // arrange
        let plugin = CopilotAuthPlugin::new();

        // act
        let methods = plugin.auth_methods();

        // assert
        assert_eq!(methods.len(), 1);
        assert!(matches!(methods[0], AuthMethodSpec::Prompts { .. }));
    }

    #[test]
    fn copilot_prompts_have_conditional_enterprise_url() {
        // arrange
        let plugin = CopilotAuthPlugin::new();

        // act
        let methods = plugin.auth_methods();
        let AuthMethodSpec::Prompts { prompts, .. } = &methods[0] else {
            panic!("expected prompts method");
        };
        let enterprise_url = prompts
            .iter()
            .find(|field| field.key == "enterprise_url")
            .unwrap_or_abort();

        // assert
        assert!(enterprise_url.when.is_some());
    }

    #[test]
    fn prompt_condition_eq_op() {
        // arrange (no setup needed for enum comparison)
        // act
        let eq_result = PromptOp::Eq == PromptOp::Eq;
        let neq_result = PromptOp::Eq != PromptOp::Neq;

        // assert
        assert!(eq_result);
        assert!(neq_result);
    }

    #[test]
    fn registry_with_builtins_has_two_providers() {
        // arrange
        let registry = AuthPluginRegistry::with_builtins();

        // act
        let count = registry.providers().len();

        // assert
        assert_eq!(count, 2);
    }
}
