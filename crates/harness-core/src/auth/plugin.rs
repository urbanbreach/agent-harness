use super::ProviderId;
use std::{collections::BTreeMap, sync::Arc};

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

#[derive(Default)]
pub struct AuthPluginRegistry {
    plugins: BTreeMap<ProviderId, Arc<dyn AuthPlugin>>,
}
impl AuthPluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(&mut self, plugin: Arc<dyn AuthPlugin>) {
        self.plugins.insert(plugin.provider_id().clone(), plugin);
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
pub struct CodexAuthPlugin {
    provider: ProviderId,
    methods: Vec<AuthMethodSpec>,
}
impl CodexAuthPlugin {
    pub fn new() -> Self {
        Self::default()
    }
}
impl Default for CodexAuthPlugin {
    fn default() -> Self {
        Self {
            provider: ProviderId::codex(),
            methods: vec![
                AuthMethodSpec::OAuthAuto {
                    label: "ChatGPT Pro/Plus (browser)".into(),
                    port: 1455,
                },
                AuthMethodSpec::OAuthCode {
                    label: "ChatGPT Pro/Plus (headless)".into(),
                },
                AuthMethodSpec::ApiKey {
                    label: "Manually enter API Key".into(),
                },
            ],
        }
    }
}
impl AuthPlugin for CodexAuthPlugin {
    fn provider_id(&self) -> &ProviderId {
        &self.provider
    }
    fn label(&self) -> &'static str {
        "OpenAI"
    }
    fn description(&self) -> &'static str {
        "ChatGPT Plus/Pro or API key"
    }
    fn auth_methods(&self) -> &[AuthMethodSpec] {
        &self.methods
    }
}
pub struct CopilotAuthPlugin {
    provider: ProviderId,
    methods: Vec<AuthMethodSpec>,
}
impl CopilotAuthPlugin {
    pub fn new() -> Self {
        Self::default()
    }
}
impl Default for CopilotAuthPlugin {
    fn default() -> Self {
        Self {
            provider: ProviderId::github_copilot(),
            methods: vec![AuthMethodSpec::Prompts {
                label: "Device login".into(),
                prompts: vec![
                    PromptField {
                        key: "deployment".into(),
                        message: "Select GitHub deployment type".into(),
                        placeholder: None,
                        field_type: PromptFieldType::Select,
                        when: None,
                        options: vec![
                            PromptOption {
                                id: "public".into(),
                                label: "GitHub.com".into(),
                            },
                            PromptOption {
                                id: "enterprise".into(),
                                label: "GitHub Enterprise".into(),
                            },
                        ],
                    },
                    PromptField {
                        key: "enterprise_url".into(),
                        message: "Enter your GitHub Enterprise URL or domain".into(),
                        placeholder: Some("company.ghe.com or https://company.ghe.com".into()),
                        field_type: PromptFieldType::Text,
                        when: Some(PromptCondition {
                            key: "deployment".into(),
                            op: PromptOp::Eq,
                            value: "enterprise".into(),
                        }),
                        options: Vec::new(),
                    },
                ],
            }],
        }
    }
}
impl AuthPlugin for CopilotAuthPlugin {
    fn provider_id(&self) -> &ProviderId {
        &self.provider
    }
    fn label(&self) -> &'static str {
        "GitHub Copilot"
    }
    fn description(&self) -> &'static str {
        "Device login"
    }
    fn auth_methods(&self) -> &[AuthMethodSpec] {
        &self.methods
    }
}
