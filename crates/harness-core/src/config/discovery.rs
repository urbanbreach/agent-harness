use super::*;

#[derive(Debug, Clone)]
pub struct ConfigDiscoveryContext {
    pub current_dir: PathBuf,
    pub harness_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
    /// Resolved Harness home, used for all user-level files.
    pub data_dir: Option<PathBuf>,
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
            harness_home: path("HARNESS_HOME"),
            home: path("HOME"),
            data_dir: crate::storage_paths::data_dir_from_lookup(&|key| std::env::var(key).ok()),
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
            "HARNESS_HOME" => &mut self.harness_home,
            "HOME" => &mut self.home,
            "HARNESS_CONFIG" => &mut self.runtime_config_path,
            "HARNESS_TUI_CONFIG" => &mut self.tui_config_path,
            _ => return self,
        };
        *field = value.filter(|v| !v.is_empty()).map(PathBuf::from);
        if matches!(name, "HOME" | "HARNESS_HOME") {
            #[cfg(not(windows))]
            {
                self.data_dir = self
                    .harness_home
                    .clone()
                    .or_else(|| self.home.as_ref().map(|home| home.join(".harness")));
            }
            #[cfg(windows)]
            {
                self.data_dir = self.harness_home.clone().or_else(|| {
                    crate::storage_paths::data_dir_from_lookup(&|key| {
                        if key == "HARNESS_HOME" {
                            None
                        } else {
                            std::env::var(key).ok()
                        }
                    })
                });
            }
        }
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
    let loaded = load_config_or_defaults_with_lookup(explicit, context, lookup)?;
    Ok((!loaded.paths.is_empty() || context.runtime_content.is_some()).then_some(loaded))
}

/// Loads defaults, runtime and TUI layers, and workspace instructions even without files.
pub fn load_config_or_defaults_with_lookup(
    explicit: Option<&Path>,
    context: &ConfigLoadContext,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<LoadedConfig, ConfigError> {
    let paths = resolve_config_layer_paths_with_context(explicit, &context.discovery);
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
    loader::instruction_files(
        &mut config,
        &context.discovery.current_dir,
        context.discovery.data_dir.as_deref(),
    )?;
    registries::register(&config)?;
    Ok(LoadedConfig { config, paths })
}

pub(crate) fn search_roots(directory: &Path) -> Vec<&Path> {
    let mut roots: Vec<_> = directory.ancestors().collect();
    if let Some(at) = roots.iter().position(|root| root.join(".git").exists()) {
        roots.truncate(at + 1);
    } else {
        roots.truncate(1);
    }
    roots.reverse();
    roots
}

fn config_names(tui: bool) -> &'static [&'static str] {
    if tui {
        &["tui.jsonc", "tui.json"]
    } else {
        &["harness.jsonc", "harness.json"]
    }
}

fn global_candidates(context: &ConfigDiscoveryContext, tui: bool) -> Vec<PathBuf> {
    context.data_dir.as_ref().map_or_else(Vec::new, |dir| {
        config_names(tui)
            .iter()
            .map(|name| dir.join(name))
            .collect()
    })
}

fn environment_candidate(context: &ConfigDiscoveryContext, tui: bool) -> Option<&PathBuf> {
    if tui {
        context.tui_config_path.as_ref()
    } else {
        context.runtime_config_path.as_ref()
    }
}

fn project_candidates(context: &ConfigDiscoveryContext, tui: bool) -> Vec<PathBuf> {
    let names = config_names(tui);
    search_roots(&context.current_dir)
        .into_iter()
        .flat_map(|base| {
            ["", ".harness"]
                .into_iter()
                .flat_map(move |prefix| names.iter().map(move |name| base.join(prefix).join(name)))
        })
        .collect()
}

/// Every candidate configuration path in merge order, including missing files.
pub fn config_search_paths(context: &ConfigDiscoveryContext, tui: bool) -> Vec<PathBuf> {
    let mut paths = global_candidates(context, tui);
    paths.extend(environment_candidate(context, tui).cloned());
    paths.extend(project_candidates(context, tui));
    paths
}

/// Existing layers in merge order. The global directory contributes only its first
/// existing file; an environment-selected path is kept even when missing so loading
/// reports it.
pub(super) fn discover(context: &ConfigDiscoveryContext, tui: bool) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = global_candidates(context, tui)
        .into_iter()
        .find(|path| path.is_file())
        .into_iter()
        .collect();
    if let Some(path) = environment_candidate(context, tui)
        && !paths.contains(path)
    {
        paths.push(path.clone());
    }
    for path in project_candidates(context, tui) {
        if path.is_file() && !paths.contains(&path) {
            paths.push(path);
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
        let global = root.path().join("home");
        let project = root.path().join("project");
        std::fs::create_dir_all(&global)?;
        std::fs::create_dir_all(project.join(".git"))?;
        let context = ConfigLoadContext {
            discovery: ConfigDiscoveryContext {
                current_dir: project.clone(),
                harness_home: None,
                data_dir: Some(global.clone()),
                home: None,
                runtime_config_path: None,
                tui_config_path: None,
            },
            runtime_content: None,
        };
        std::fs::write(global.join("config.jsonc"), "{removed_global_setting:true}")?;
        assert!(load_resolved_config_with_context(None, &context)?.is_none());
        assert!(!project.join("harness.json").exists());
        std::fs::write(global.join("harness.jsonc"), "{runtime:{compaction:{enabled:false,fallbackInputTokens:1234}}, permission:{bash:{'git *':'allow','*':'deny'}}}")?;
        std::fs::write(global.join("harness.json"), "{unknown_global_setting:true}")?;
        std::fs::write(
            project.join("config.jsonc"),
            "{unknown_project_setting:true}",
        )?;
        std::fs::write(project.join("prompt.txt"), "local instructions")?;
        std::fs::write(project.join("harness.jsonc"), "{runtime:{compaction:{fallback_input_tokens:8192}},agent:{default:{system_prompt:'{file:prompt.txt}'}},permission:{bash:{'git status':'ask'}}}")?;
        std::fs::write(
            global.join("tui.jsonc"),
            "{keybinds:{copy_selection:'ctrl+x'}}",
        )?;
        std::fs::write(global.join("tui.json"), "{removed_tui_setting:true}")?;
        std::fs::write(
            project.join("tui.jsonc"),
            "{keybinds:{copy_selection:'ctrl+y'}}",
        )?;
        let loaded =
            load_resolved_config_with_context(None, &context)?.ok_or("configuration not found")?;
        assert_eq!(
            resolve_tui_config_layer_paths_with_context(&context.discovery),
            [global.join("tui.jsonc"), project.join("tui.jsonc")]
        );
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
        assert!(!project.join(".harness").exists());
        let explicit =
            load_resolved_config_with_context(Some(&global.join("harness.jsonc")), &context)?
                .ok_or("explicit config missing")?;
        assert_eq!(explicit.paths, vec![global.join("harness.jsonc")]);
        assert!(explicit.config.agents["default"].system_prompt.is_none());
        for environment_path in [global.join("harness.jsonc"), project.join("harness.jsonc")] {
            let mut duplicate = context.clone();
            duplicate.discovery.runtime_config_path = Some(environment_path);
            let resolved = load_resolved_config_with_context(None, &duplicate)?
                .ok_or("configuration missing with duplicate environment path")?;
            assert_eq!(resolved.paths, loaded.paths);
        }
        let mut missing = context.clone();
        let missing_path = project.join("missing.jsonc");
        missing.discovery.runtime_config_path = Some(missing_path.clone());
        assert!(
            resolve_config_layer_paths_with_context(None, &missing.discovery)
                .contains(&missing_path)
        );
        assert!(load_resolved_config_with_context(None, &missing).is_err());
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

    #[test]
    fn injected_home_overrides_preserve_precedence_and_can_disable_discovery(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let home = root.path().join("user");
        let custom = root.path().join("custom");
        let context = ConfigDiscoveryContext {
            current_dir: root.path().into(),
            home: Some(home),
            harness_home: Some(custom.clone()),
            data_dir: Some(custom.clone()),
            runtime_config_path: None,
            tui_config_path: None,
        };
        let changed_home = root.path().join("other");
        let context =
            context.apply_env_var("HOME", Some(changed_home.to_string_lossy().into_owned()));
        assert_eq!(context.data_dir.as_ref(), Some(&custom));
        #[cfg(not(windows))]
        {
            let fallback = context
                .clone()
                .apply_env_var("HARNESS_HOME", Some(String::new()));
            assert_eq!(fallback.data_dir, Some(changed_home.join(".harness")));
            let missing = context
                .apply_env_var("HOME", None)
                .apply_env_var("HARNESS_HOME", None);
            assert!(missing.data_dir.is_none());
            assert_eq!(config_search_paths(&missing, false).len(), 4);
        }
        Ok(())
    }

    #[test]
    fn startup_instructions_use_injected_home_and_prefer_agents_over_claude(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (files, selected) in [
            (vec![], None),
            (vec!["AGENTS.md"], Some("AGENTS.md")),
            (vec!["CLAUDE.md"], Some("CLAUDE.md")),
            (vec!["CLAUDE.md", "AGENTS.md"], Some("AGENTS.md")),
        ] {
            let root = tempfile::tempdir()?;
            let home = root.path().join("harness-home");
            let project = root.path().join("project");
            let current = project.join("sub");
            std::fs::create_dir(&home)?;
            std::fs::create_dir_all(&current)?;
            std::fs::create_dir(project.join(".git"))?;
            std::fs::write(home.join("AGENTS.md"), "USER_INSTRUCTIONS")?;
            std::fs::write(home.join("CLAUDE.md"), "UNUSED_USER_CLAUDE")?;
            std::fs::write(project.join("AGENTS.md"), "ROOT_INSTRUCTIONS")?;
            for file in files {
                std::fs::write(current.join(file), file)?;
            }
            let config_path = project.join("harness.jsonc");
            std::fs::write(&config_path, "{instructions:'CONFIGURED_INSTRUCTIONS'}")?;
            let context = ConfigLoadContext {
                discovery: ConfigDiscoveryContext {
                    current_dir: current,
                    harness_home: Some(home.clone()),
                    home: None,
                    data_dir: Some(home),
                    runtime_config_path: None,
                    tui_config_path: None,
                },
                runtime_content: None,
            };
            let mut expected = vec![
                "USER_INSTRUCTIONS",
                "CONFIGURED_INSTRUCTIONS",
                "ROOT_INSTRUCTIONS",
            ];
            expected.extend(selected);
            for config in [
                load_config_or_defaults_with_lookup(None, &context, &|_| None)?.config,
                loader::load_config_from_file_with_context(&config_path, &context)?,
            ] {
                assert_eq!(
                    config
                        .instruction_files
                        .iter()
                        .map(|file| file.content.as_str())
                        .collect::<Vec<_>>(),
                    expected
                );
            }
        }
        Ok(())
    }
}
