use super::*;

/// All discovery inputs are explicit, including the authoritative project trust verdict.
#[derive(Debug, Clone)]
pub struct SubagentDiscoveryContext {
    pub cwd: PathBuf,
    pub project_trusted: bool,
    pub user_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct SubagentDefinitionSnapshot {
    pub project: BTreeMap<String, SubagentDefinition>,
    pub user: BTreeMap<String, SubagentDefinition>,
    pub roles: BTreeMap<String, SubagentRole>,
    pub personas: BTreeMap<String, SubagentPersona>,
    /// Prompt I/O is captured before pure resolution; failures remain observable.
    pub prompt_files: BTreeMap<PathBuf, Result<String, String>>,
    pub warnings: Vec<String>,
}

impl SubagentDefinitionSnapshot {
    pub fn definition(&self, name: &str) -> Option<SubagentDefinition> {
        self.project
            .get(name)
            .cloned()
            .or_else(|| self.user.get(name).cloned())
            .or_else(|| {
                builtin_subagent_definitions()
                    .into_iter()
                    .find(|definition| definition.name == name)
            })
    }

    pub fn available_names(&self, toggles: &BTreeMap<String, bool>) -> Vec<String> {
        let mut names: Vec<_> = builtin_subagent_definitions()
            .into_iter()
            .map(|definition| definition.name)
            .chain(self.project.keys().cloned())
            .chain(self.user.keys().cloned())
            .collect();
        names.sort();
        names.dedup();
        names.retain(|name| {
            toggles.get(name).copied().unwrap_or(true) && self.definition(name).is_some()
        });
        names
    }

    pub fn selectable_types(
        &self,
        toggles: &BTreeMap<String, bool>,
        allowed: Option<&[String]>,
    ) -> Vec<SubagentTypeDescriptor> {
        self.available_names(toggles)
            .into_iter()
            .filter_map(|name| {
                let definition = self.definition(&name)?;
                if allowed.is_some_and(|allowed| {
                    !allowed
                        .iter()
                        .any(|entry| entry.eq_ignore_ascii_case(&name))
                }) {
                    return None;
                }
                Some(SubagentTypeDescriptor {
                    name,
                    description: definition.description,
                })
            })
            .collect()
    }
}

/// Read-only discovery, separate from the pure spawn resolver and coordinator authority.
pub fn discover_subagent_definitions(
    config: &SubagentRuntimeConfig,
    context: &SubagentDiscoveryContext,
) -> SubagentDefinitionSnapshot {
    let mut snapshot = SubagentDefinitionSnapshot {
        roles: config
            .roles
            .iter()
            .filter(|(_, role)| role.source_dir.is_none())
            .map(|(name, role)| (name.clone(), role.clone()))
            .collect(),
        personas: config
            .personas
            .iter()
            .filter(|(_, persona)| persona.source_path.is_none())
            .map(|(name, persona)| (name.clone(), persona.clone()))
            .collect(),
        ..SubagentDefinitionSnapshot::default()
    };
    // Nearest project directory wins; a worktree's .git file is a boundary too.
    for root in super::super::discovery::search_roots(&context.cwd)
        .into_iter()
        .rev()
    {
        let directory = root.join(".harness");
        load_agents(
            &directory.join("agents"),
            SubagentDefinitionSource::Project,
            &mut snapshot.project,
            &mut snapshot.warnings,
        );
        if context.project_trusted {
            load_presets(&directory, &mut snapshot);
        }
    }
    for (name, role) in &config.roles {
        snapshot
            .roles
            .entry(name.clone())
            .or_insert_with(|| role.clone());
    }
    for (name, persona) in &config.personas {
        snapshot
            .personas
            .entry(name.clone())
            .or_insert_with(|| persona.clone());
    }
    if let Some(root) = &context.user_root {
        load_agents(
            &root.join("agents"),
            SubagentDefinitionSource::User,
            &mut snapshot.user,
            &mut snapshot.warnings,
        );
        load_presets(root, &mut snapshot);
    }
    load_prompt_files(&mut snapshot, context);
    snapshot
}

fn load_prompt_files(
    snapshot: &mut SubagentDefinitionSnapshot,
    context: &SubagentDiscoveryContext,
) {
    for role in snapshot.roles.values() {
        if let Some(file) = &role.prompt_file {
            let path = role
                .source_dir
                .as_deref()
                .unwrap_or(&context.cwd)
                .join(file);
            snapshot
                .prompt_files
                .entry(path.clone())
                .or_insert_with(|| {
                    super::super::loader::read_text(&path).map_err(|error| error.to_string())
                });
        }
    }
    for persona in snapshot.personas.values() {
        if let Some(file) = &persona.instructions_file {
            let path = persona
                .source_dir
                .as_deref()
                .unwrap_or(&context.cwd)
                .join(file);
            snapshot
                .prompt_files
                .entry(path.clone())
                .or_insert_with(|| {
                    super::super::loader::read_text(&path).map_err(|error| error.to_string())
                });
        }
    }
}

fn files(directory: &Path, extension: &str, warnings: &mut Vec<String>) -> Vec<PathBuf> {
    if !directory.is_dir() {
        return Vec::new();
    }
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            warnings.push(format!("cannot discover {}: {error}", directory.display()));
            return Vec::new();
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry)
                if entry
                    .path()
                    .extension()
                    .and_then(|extension| extension.to_str())
                    == Some(extension) =>
            {
                paths.push(entry.path())
            }
            Ok(_) => {}
            Err(error) => {
                warnings.push(format!("cannot discover {}: {error}", directory.display()))
            }
        }
    }
    paths.sort();
    paths
}

fn load_agents(
    directory: &Path,
    source: SubagentDefinitionSource,
    target: &mut BTreeMap<String, SubagentDefinition>,
    warnings: &mut Vec<String>,
) {
    for path in files(directory, "md", warnings) {
        let parsed = super::super::loader::read_text(&path)
            .and_then(|text| parse_subagent_definition(&text));
        match parsed {
            Ok(mut definition) => {
                definition.source_path = Some(path);
                definition.source = source.clone();
                target.entry(definition.name.clone()).or_insert(definition);
            }
            Err(error) => warnings.push(format!("{}: {error}", path.display())),
        }
    }
}

pub fn parse_subagent_definition(text: &str) -> Result<SubagentDefinition, ConfigError> {
    let text = text.trim_start();
    let rest = text
        .strip_prefix("---")
        .ok_or_else(|| ConfigError("missing agent frontmatter delimiters".into()))?;
    let (yaml, body) = rest
        .split_once("\n---")
        .ok_or_else(|| ConfigError("missing closing agent frontmatter delimiter".into()))?;
    let mut definition: SubagentDefinition =
        serde_yaml_ng::from_str(yaml).map_err(super::super::normalize::parse_error)?;
    if definition.name.is_empty() {
        return Err(ConfigError("agent definition requires a name".into()));
    }
    if let Some(model) = &definition.model {
        validate_subagent_model_role(model)?;
    }
    let body = body.split_once('\n').map_or("", |(_, body)| body).trim();
    definition.prompt_body = (!body.is_empty()).then(|| body.to_owned());
    Ok(definition)
}

fn load_presets(root: &Path, snapshot: &mut SubagentDefinitionSnapshot) {
    for path in files(&root.join("roles"), "toml", &mut snapshot.warnings) {
        let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        if snapshot.roles.contains_key(name) {
            continue;
        }
        let role = super::super::loader::read_text(&path).and_then(|text| {
            let role: SubagentRole =
                toml::from_str(&text).map_err(super::super::normalize::parse_error)?;
            if let Some(model) = &role.model {
                validate_subagent_model_role(model)?;
            }
            Ok(role)
        });
        match role {
            Ok(mut role) => {
                role.source_dir = path.parent().map(Path::to_path_buf);
                snapshot.roles.insert(name.into(), role);
            }
            Err(error) => snapshot
                .warnings
                .push(format!("{}: {error}", path.display())),
        }
    }
    for path in files(&root.join("personas"), "toml", &mut snapshot.warnings) {
        let Some(name) = path
            .file_stem()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        if snapshot.personas.contains_key(&name) {
            continue;
        }
        let persona = super::super::loader::read_text(&path).and_then(|text| {
            let persona: SubagentPersona =
                toml::from_str(&text).map_err(super::super::normalize::parse_error)?;
            if let Some(model) = &persona.model {
                validate_subagent_model_role(model)?;
            }
            Ok(persona)
        });
        match persona {
            Ok(mut persona) => {
                persona.source_dir = path.parent().map(Path::to_path_buf);
                persona.source_path = Some(path);
                snapshot.personas.insert(name, persona);
            }
            Err(error) => snapshot
                .warnings
                .push(format!("{}: {error}", path.display())),
        }
    }
}
