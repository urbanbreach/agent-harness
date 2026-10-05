use super::*;

#[derive(Debug, Clone)]
pub struct ConfigDiscoveryContext {
    pub current_dir: PathBuf,
    pub xdg_config_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub runtime_config_path: Option<PathBuf>,
    pub tui_config_path: Option<PathBuf>,
}
impl ConfigDiscoveryContext {
    pub fn from_env() -> Self {
        let path = |key| {
            std::env::var_os(key)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        };
        Self {
            current_dir: std::env::current_dir().unwrap_or_else(|_| ".".into()),
            xdg_config_home: path("XDG_CONFIG_HOME"),
            home: path("HOME"),
            runtime_config_path: path("HARNESS_CONFIG"),
            tui_config_path: path("HARNESS_TUI_CONFIG"),
        }
    }
    pub fn with_current_dir(mut self, directory: PathBuf) -> Self {
        self.current_dir = directory;
        self
    }
    pub fn apply_env_var(mut self, name: &str, value: Option<String>) -> Self {
        let field = match name {
            "XDG_CONFIG_HOME" => &mut self.xdg_config_home,
            "HOME" => &mut self.home,
            "HARNESS_CONFIG" => &mut self.runtime_config_path,
            "HARNESS_TUI_CONFIG" => &mut self.tui_config_path,
            _ => return self,
        };
        *field = value.filter(|v| !v.is_empty()).map(PathBuf::from);
        self
    }
}

#[derive(Debug, Clone)]
pub struct ConfigLoadContext {
    pub discovery: ConfigDiscoveryContext,
    pub runtime_content: Option<String>,
}
impl ConfigLoadContext {
    pub fn from_env() -> Self {
        Self {
            discovery: ConfigDiscoveryContext::from_env(),
            runtime_content: std::env::var("HARNESS_CONFIG_CONTENT").ok(),
        }
    }
    pub fn with_current_dir(mut self, directory: PathBuf) -> Self {
        self.discovery.current_dir = directory;
        self
    }
    pub fn apply_env_var(mut self, name: &str, value: Option<String>) -> Self {
        if name == "HARNESS_CONFIG_CONTENT" {
            self.runtime_content = value;
        } else {
            self.discovery = self.discovery.apply_env_var(name, value);
        }
        self
    }
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: HarnessConfig,
    pub paths: Vec<PathBuf>,
}
impl LoadedConfig {
    pub fn primary_path(&self) -> Option<&Path> {
        self.paths.last().map(PathBuf::as_path)
    }
    pub fn path_display(&self) -> String {
        if self.paths.is_empty() {
            "<none>".into()
        } else {
            self.paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(" + ")
        }
    }
}

pub fn resolve_config_layer_paths(explicit: Option<&Path>) -> Vec<PathBuf> {
    resolve_config_layer_paths_with_context(explicit, &ConfigDiscoveryContext::from_env())
}
pub fn resolve_config_path(explicit: Option<&Path>) -> Option<PathBuf> {
    resolve_config_layer_paths(explicit).pop()
}
pub fn resolve_config_path_with_context(
    explicit: Option<&Path>,
    context: &ConfigDiscoveryContext,
) -> Option<PathBuf> {
    resolve_config_layer_paths_with_context(explicit, context).pop()
}
pub fn resolve_config_layer_paths_with_context(
    explicit: Option<&Path>,
    context: &ConfigDiscoveryContext,
) -> Vec<PathBuf> {
    explicit.map_or_else(
        || discover(context, false),
        |path| vec![context.current_dir.join(path)],
    )
}
pub fn resolve_tui_config_layer_paths_with_context(
    context: &ConfigDiscoveryContext,
) -> Vec<PathBuf> {
    discover(context, true)
}

pub fn load_resolved_config(explicit: Option<&Path>) -> Result<Option<LoadedConfig>, ConfigError> {
    load_resolved_config_with_context(explicit, &ConfigLoadContext::from_env())
}
pub fn load_resolved_config_with_context(
    explicit: Option<&Path>,
    context: &ConfigLoadContext,
) -> Result<Option<LoadedConfig>, ConfigError> {
    load_resolved_config_with_lookup(explicit, context, &|name| std::env::var(name).ok())
}
pub fn load_resolved_config_with_lookup(
    explicit: Option<&Path>,
    context: &ConfigLoadContext,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<LoadedConfig>, ConfigError> {
    let paths = resolve_config_layer_paths_with_context(explicit, &context.discovery);
    if paths.is_empty() && context.runtime_content.is_none() {
        return Ok(None);
    }
    let mut value = ordered::OrderedValue::from(serde_json::json!({}));
    let mut instructions = Vec::new();
    for path in &paths {
        let fragment = loader::parse_layer_with_lookup(
            &loader::read_text(path)?,
            path.parent().unwrap_or(Path::new(".")),
            &mut instructions,
            lookup,
        )?;
        value.merge(fragment);
    }
    if let Some(content) = &context.runtime_content {
        value.merge(loader::parse_layer_with_lookup(
            content,
            &context.discovery.current_dir,
            &mut instructions,
            lookup,
        )?);
    }
    let mut config = normalize::normalize(value)?;
    config.instruction_files = instructions;
    let mut tui = serde_json::json!({});
    for path in discover(&context.discovery, true) {
        let layer = json5::from_str(&loader::read_text(&path)?).map_err(normalize::parse_error)?;
        normalize::merge(&mut tui, layer);
    }
    let tui: PublicTuiConfig = serde_json::from_value(tui).map_err(normalize::parse_error)?;
    config.ui.keybindings = tui.keybindings;
    loader::instruction_files(&mut config, &context.discovery.current_dir)?;
    registries::register(&config)?;
    Ok(Some(LoadedConfig { config, paths }))
}

pub(super) fn search_roots(directory: &Path) -> Vec<&Path> {
    let mut roots: Vec<_> = directory.ancestors().collect();
    if let Some(at) = roots.iter().position(|root| root.join(".git").exists()) {
        roots.truncate(at + 1);
    } else {
        roots.truncate(1);
    }
    roots.reverse();
    roots
}

pub(super) fn discover(context: &ConfigDiscoveryContext, tui: bool) -> Vec<PathBuf> {
    let names: &[&str] = if tui {
        &["tui.jsonc", "tui.json"]
    } else {
        &["harness.jsonc", "harness.json", "config.jsonc"]
    };
    let mut paths = Vec::new();
    let global = context
        .xdg_config_home
        .clone()
        .or_else(|| context.home.as_ref().map(|home| home.join(".config")));
    if let Some(base) = global
        && let Some(path) = names
            .iter()
            .map(|name| base.join("harness").join(name))
            .find(|path| path.is_file())
    {
        paths.push(path);
    }
    if let Some(path) = if tui {
        &context.tui_config_path
    } else {
        &context.runtime_config_path
    } && !paths.contains(path)
    {
        paths.push(path.clone());
    }
    for base in search_roots(&context.current_dir) {
        for prefix in ["", ".agent-harness"] {
            for name in &names[..2] {
                let path = base.join(prefix).join(name);
                if path.is_file() && !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
    }
    paths
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PublicTuiConfig {
    #[serde(rename = "$schema")]
    pub schema: Option<String>,
    #[serde(rename = "keybinds", alias = "keybindings")]
    pub keybindings: BTreeMap<String, String>,
    pub confirm_before_rewind: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_discovery_merges_layers_without_writing_and_resolves_file_values(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let global = root.path().join("config/harness");
        let project = root.path().join("project");
        std::fs::create_dir_all(&global)?;
        std::fs::create_dir_all(project.join(".git"))?;
        let context = ConfigLoadContext {
            discovery: ConfigDiscoveryContext {
                current_dir: project.clone(),
                xdg_config_home: Some(root.path().join("config")),
                home: None,
                runtime_config_path: None,
                tui_config_path: None,
            },
            runtime_content: None,
        };
        assert!(load_resolved_config_with_context(None, &context)?.is_none());
        assert!(!project.join("harness.json").exists());
        std::fs::write(global.join("harness.jsonc"), "{runtime:{compaction:{enabled:false,fallbackInputTokens:1234}}, permission:{bash:{'git *':'allow','*':'deny'}}}")?;
        std::fs::write(project.join("prompt.txt"), "local instructions")?;
        std::fs::write(project.join("harness.jsonc"), "{runtime:{compaction:{fallback_input_tokens:8192}},agent:{default:{system_prompt:'{file:prompt.txt}'}},permission:{bash:{'git status':'ask'}}}")?;
        std::fs::write(
            project.join("tui.jsonc"),
            "{keybinds:{copy_selection:'ctrl+y'}}",
        )?;
        let loaded =
            load_resolved_config_with_context(None, &context)?.ok_or("configuration not found")?;
        assert_eq!(loaded.paths.len(), 2);
        assert_eq!(
            loaded.primary_path(),
            Some(project.join("harness.jsonc").as_path())
        );
        assert_eq!(loaded.config.runtime.compaction.fallback_input_tokens, 8192);
        assert_eq!(loaded.config.ui.keybindings["copy_selection"], "ctrl+y");
        assert!(!loaded.config.runtime.compaction.enabled);
        assert_eq!(
            loaded.config.agents["default"].system_prompt.as_deref(),
            Some("local instructions")
        );
        assert_eq!(
            loaded
                .config
                .permissions
                .rules
                .shell
                .iter()
                .map(|r| r.mode)
                .collect::<Vec<_>>(),
            vec![
                PermissionMode::Allow,
                PermissionMode::Deny,
                PermissionMode::Ask
            ]
        );
        assert!(!project.join(".agent-harness").exists());
        let explicit =
            load_resolved_config_with_context(Some(&global.join("harness.jsonc")), &context)?
                .ok_or("explicit config missing")?;
        assert_eq!(explicit.paths, vec![global.join("harness.jsonc")]);
        assert!(explicit.config.agents["default"].system_prompt.is_none());
        let mut references =
            serde_json::json!({"text":"${EMPTY}|${EMPTY:-fallback}|{env:ABSENT}|{env:TOKEN}"});
        references::expand(&mut references, &project, &|name| match name {
            "EMPTY" => Some(String::new()),
            "TOKEN" => Some("${KEEP_LITERAL}".into()),
            _ => None,
        })?;
        assert_eq!(references["text"], "|fallback||${KEEP_LITERAL}");
        assert!(
            references::expand(&mut serde_json::json!("${ABSENT}"), &project, &|_| None).is_err()
        );
        Ok(())
    }
}
