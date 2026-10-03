use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SubagentCapabilityMode {
    ReadOnly,
    ReadWrite,
    Execute,
    #[default]
    All,
}

impl SubagentCapabilityMode {
    pub fn intersect(self, ceiling: Self) -> Self {
        match (self, ceiling) {
            (Self::All, mode) | (mode, Self::All) => mode,
            (Self::ReadOnly, _) | (_, Self::ReadOnly) => Self::ReadOnly,
            (Self::ReadWrite, Self::Execute) | (Self::Execute, Self::ReadWrite) => Self::ReadOnly,
            (mode, _) => mode,
        }
    }

    pub fn allows(self, kind: SubagentToolKind) -> bool {
        use SubagentToolKind as Kind;
        self == Self::All
            || match kind {
                Kind::Read
                | Kind::ListDir
                | Kind::List
                | Kind::Search
                | Kind::Lsp
                | Kind::Plan
                | Kind::MemorySearch
                | Kind::MemoryGet
                | Kind::WebSearch
                | Kind::WebFetch
                | Kind::BackgroundTaskAction
                | Kind::KillTaskAction
                | Kind::Task
                | Kind::EnterPlan
                | Kind::ExitPlan
                | Kind::AskUser
                | Kind::Skill => true,
                Kind::Edit
                | Kind::Write
                | Kind::Delete
                | Kind::Move
                | Kind::Feedback
                | Kind::ImageGen
                | Kind::VideoGen
                | Kind::ImageToVideo
                | Kind::ReferenceToVideo => self == Self::ReadWrite,
                Kind::Execute => self == Self::Execute,
                Kind::ActiveAgentMessage => self != Self::ReadOnly,
                Kind::Workflow => false,
            }
    }
}

/// Explicit native kind metadata; untyped custom/MCP tools use `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubagentToolKind {
    Read,
    ListDir,
    List,
    Search,
    Lsp,
    Plan,
    MemorySearch,
    MemoryGet,
    WebSearch,
    WebFetch,
    BackgroundTaskAction,
    KillTaskAction,
    Task,
    EnterPlan,
    ExitPlan,
    AskUser,
    Skill,
    Edit,
    Write,
    Delete,
    Move,
    Feedback,
    ImageGen,
    VideoGen,
    ImageToVideo,
    ReferenceToVideo,
    Execute,
    ActiveAgentMessage,
    Workflow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentTool {
    pub id: String,
    pub kind: Option<SubagentToolKind>,
    pub background_capable: bool,
    pub mcp_server: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SubagentRole {
    pub description: String,
    pub default_capability_mode: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub prompt_file: Option<String>,
    pub default_isolation: Option<String>,
    #[serde(skip)]
    pub source_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SubagentPersona {
    pub instructions: Option<String>,
    pub description: Option<String>,
    pub instructions_file: Option<String>,
    pub inputs: Vec<SubagentPersonaIoField>,
    pub outputs: Vec<SubagentPersonaIoField>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub default_isolation: Option<String>,
    #[serde(skip)]
    pub source_dir: Option<PathBuf>,
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubagentPersonaIoField {
    pub name: String,
    #[serde(default = "file_io_type")]
    pub io_type: String,
    #[serde(default)]
    pub required: bool,
    pub description: String,
}
fn file_io_type() -> String {
    "file".into()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SubagentPermissionMode {
    #[default]
    Default,
    AcceptEdits,
    Auto,
    DontAsk,
    BypassPermissions,
    Plan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SubagentMcpInheritance {
    Mode(SubagentMcpMode),
    Named { named: Vec<String> },
    Except { except: Vec<String> },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SubagentMcpMode {
    All,
    None,
}
impl Default for SubagentMcpInheritance {
    fn default() -> Self {
        Self::Mode(SubagentMcpMode::All)
    }
}
impl SubagentMcpInheritance {
    pub fn includes(&self, server: &str) -> bool {
        match self {
            Self::Mode(SubagentMcpMode::All) => true,
            Self::Mode(SubagentMcpMode::None) => false,
            Self::Named { named } => named.iter().any(|name| name == server),
            Self::Except { except } => !except.iter().any(|name| name == server),
        }
    }
}

/// Definition identity is independent of generic `ProfileConfig`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct SubagentDefinition {
    pub name: String,
    pub description: String,
    pub prompt_body: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub capability_mode: Option<SubagentCapabilityMode>,
    pub isolation: Option<SubagentIsolationMode>,
    pub permission_mode: SubagentPermissionMode,
    pub max_turns: Option<NonZeroU32>,
    pub inherit_skills: bool,
    pub discover_skills: bool,
    pub skills: Vec<String>,
    pub mcp_inheritance: SubagentMcpInheritance,
    pub mcp_servers: Vec<String>,
    pub inject_default_tools: bool,
    /// Native declared tool IDs. Empty uses the general-purpose tool inventory.
    pub declared_tools: Vec<String>,
    pub tool_config: Option<SubagentDeclaredToolConfig>,
    /// Author allow/deny filters, separate from parent operator restrictions.
    #[serde(deserialize_with = "string_or_list")]
    pub tools: Vec<String>,
    #[serde(deserialize_with = "string_or_list")]
    pub disallowed_tools: Vec<String>,
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
    #[serde(skip)]
    pub source: SubagentDefinitionSource,
}
impl Default for SubagentDefinition {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            prompt_body: None,
            model: None,
            effort: None,
            capability_mode: None,
            isolation: None,
            permission_mode: SubagentPermissionMode::Default,
            max_turns: None,
            inherit_skills: true,
            discover_skills: true,
            skills: Vec::new(),
            mcp_inheritance: SubagentMcpInheritance::default(),
            mcp_servers: Vec::new(),
            inject_default_tools: true,
            declared_tools: Vec::new(),
            tool_config: None,
            tools: Vec::new(),
            disallowed_tools: Vec::new(),
            source_path: None,
            source: SubagentDefinitionSource::Cli,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubagentDeclaredToolConfig {
    pub tools: Vec<SubagentDeclaredTool>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SubagentDeclaredTool {
    pub id: String,
    #[serde(default)]
    pub params: Option<BTreeMap<String, serde_json::Value>>,
}

fn string_or_list<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Input {
        Text(String),
        List(Vec<String>),
    }
    Ok(match Option::<Input>::deserialize(deserializer)? {
        Some(Input::Text(text)) => regex::Regex::new(r"(?i:Agent|Task)\([^)]*\)|[^,]+")
            .map_err(serde::de::Error::custom)?
            .find_iter(&text)
            .map(|token| token.as_str().trim())
            .filter(|token| !token.is_empty())
            .map(str::to_owned)
            .collect(),
        Some(Input::List(list)) => list,
        None => Vec::new(),
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SubagentDefinitionSource {
    Project,
    Builtin,
    User,
    Bundled,
    Plugin {
        name: String,
    },
    #[default]
    Cli,
}

pub fn builtin_subagent_definitions() -> Vec<SubagentDefinition> {
    ["general-purpose", "explore", "plan"]
        .into_iter()
        .map(|name| {
            let inspection = name != "general-purpose";
            SubagentDefinition {
                name: name.into(),
                description: match name {
                    "explore" => "Explore a codebase using read-only inspection.",
                    "plan" => "Plan work using read-only inspection and a todo list.",
                    _ => "General-purpose implementation and research.",
                }
                .into(),
                declared_tools: if inspection {
                    let mut tools = vec!["read".into(), "list".into(), "grep".into()];
                    if name == "plan" {
                        tools.push("todowrite".into());
                    }
                    tools
                } else {
                    Vec::new()
                },
                inherit_skills: !inspection,
                permission_mode: if inspection {
                    SubagentPermissionMode::Plan
                } else {
                    SubagentPermissionMode::Default
                },
                source: SubagentDefinitionSource::Builtin,
                ..SubagentDefinition::default()
            }
        })
        .collect()
}
