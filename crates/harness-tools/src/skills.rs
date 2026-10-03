use harness_core::{
    config::{registered_skills_config, PermissionMode, SkillCatalogDiscovery, SkillsConfig},
    redact::{DefaultRedactor, Redactor},
    tool::ToolError,
};
pub(crate) mod load;
pub use harness_core::config::{SkillCatalog, SkillCatalogEntry, SkillCatalogStatus};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, Read},
    path::{Path, PathBuf},
};

pub struct NativeSkillCatalogDiscovery;

impl SkillCatalogDiscovery for NativeSkillCatalogDiscovery {
    fn discover(
        &self,
        cwd: &Path,
        config: &SkillsConfig,
        project_trusted: Option<bool>,
    ) -> Result<SkillCatalog, ToolError> {
        let mut config = config.clone();
        if project_trusted == Some(false) {
            config.project_roots.clear();
        }
        discover_skill_catalog_with_config(cwd, &config)
    }
}
#[derive(Deserialize)]
struct Header {
    name: String,
    description: String,
    #[serde(default, alias = "argument-hint")]
    argument_hint: Option<String>,
    #[serde(default, alias = "allowed-tools")]
    allowed_tools: Option<ToolNames>,
    #[serde(default)]
    mcp: Option<String>,
    #[serde(default)]
    resources: Option<String>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum ToolNames {
    List(Vec<String>),
    Text(String),
}
pub fn discover_skill_catalog(workspace: &Path) -> Result<SkillCatalog, ToolError> {
    discover_skill_catalog_with_config(workspace, &registered_skills_config())
}
pub fn discover_skill_catalog_with_config(
    workspace: &Path,
    config: &SkillsConfig,
) -> Result<SkillCatalog, ToolError> {
    let workspace = workspace.canonicalize()?;
    let mut roots = Vec::new();
    for parent in workspace.ancestors() {
        roots.extend(
            config
                .project_roots
                .iter()
                .map(|path| ("project", parent.join(path))),
        );
        if !config.walk_to_git_root || parent.join(".git").exists() {
            break;
        }
    }
    for path in &config.global_roots {
        let path = match path.strip_prefix("~") {
            Ok(suffix) => match std::env::var_os("HOME") {
                Some(home) => PathBuf::from(home).join(suffix),
                None => continue,
            },
            Err(_) => path.clone(),
        };
        roots.push(("global", path));
    }
    let compile = |pattern: &str| {
        globset::Glob::new(pattern)
            .map(|glob| glob.compile_matcher())
            .map_err(|_| ToolError::InvalidArguments("invalid skill permission pattern".into()))
    };
    let disabled = config
        .disabled
        .iter()
        .map(|pattern| compile(pattern))
        .collect::<Result<Vec<_>, _>>()?;
    let permissions = config
        .permissions
        .iter()
        .map(|(pattern, mode)| compile(pattern).map(|matcher| (matcher, mode)))
        .collect::<Result<Vec<_>, _>>()?;
    let (mut entries, mut names, mut visited, mut scanned) =
        (Vec::new(), BTreeSet::new(), BTreeSet::new(), 0);
    for (scope, root) in roots {
        if !visited.insert(root.clone())
            || !fs::symlink_metadata(&root).is_ok_and(|meta| meta.is_dir())
        {
            continue;
        }
        for item in walkdir::WalkDir::new(&root)
            .follow_links(false)
            .max_depth(8)
            .sort_by_file_name()
        {
            scanned += 1;
            if scanned > 4096 {
                return Err(ToolError::Execution(
                    "skill catalog exceeds 4096 entries".into(),
                ));
            }
            let item =
                item.map_err(|_| ToolError::Execution("cannot inspect skill directory".into()))?;
            if !item.file_type().is_file() || item.file_name() != "SKILL.md" {
                continue;
            }
            let path = item.into_path();
            let parsed = header(&path);
            let fallback = path
                .parent()
                .and_then(Path::file_name)
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let mut entry = SkillCatalogEntry {
                name: fallback,
                stable_id: String::new(),
                description: String::new(),
                source_scope: scope.into(),
                root_path: root.clone(),
                location: path,
                loadable: false,
                permission_mode: "allow".into(),
                status: SkillCatalogStatus::Loadable,
                reason: None,
                argument_hint: None,
                allowed_tools: Vec::new(),
                deferred_mcp: None,
                deferred_resources: None,
                body_loaded: false,
                body_digest: None,
            };
            match parsed {
                Ok(header) => {
                    let redactor = DefaultRedactor::default();
                    entry.name = header.name;
                    entry.description = redactor.redact_text(&header.description);
                    entry.argument_hint = header
                        .argument_hint
                        .map(|value| redactor.redact_text(&value));
                    entry.allowed_tools = match header.allowed_tools {
                        Some(ToolNames::List(names)) => names,
                        Some(ToolNames::Text(names)) => names
                            .split([',', ' '])
                            .filter(|name| !name.is_empty())
                            .map(str::to_owned)
                            .collect(),
                        None => Vec::new(),
                    };
                    entry.deferred_mcp = header.mcp.map(|value| redactor.redact_text(&value));
                    entry.deferred_resources =
                        header.resources.map(|value| redactor.redact_text(&value));
                }
                Err(reason) => {
                    entry.status = SkillCatalogStatus::Malformed;
                    entry.reason = Some(reason);
                }
            }
            entry.stable_id = format!("skill:{scope}:{}", entry.name);
            let mode = permissions
                .iter()
                .rev()
                .find(|(matcher, _)| {
                    matcher.is_match(&entry.name) || matcher.is_match(&entry.stable_id)
                })
                .map_or(PermissionMode::Allow, |(_, mode)| **mode);
            entry.permission_mode = match mode {
                PermissionMode::Allow => "allow",
                PermissionMode::Ask => "ask",
                PermissionMode::Deny => "deny",
            }
            .into();
            if entry.reason.is_none() {
                if disabled.iter().any(|matcher| {
                    matcher.is_match(&entry.name) || matcher.is_match(&entry.stable_id)
                }) {
                    entry.status = SkillCatalogStatus::Disabled;
                    entry.reason = Some("disabled by skills.disabled".into());
                } else if mode == PermissionMode::Deny {
                    entry.status = SkillCatalogStatus::Denied;
                    entry.reason = Some("denied by skills.permissions".into());
                } else if !names.insert(entry.name.clone()) {
                    entry.status = SkillCatalogStatus::Shadowed;
                    entry.reason = Some("a nearer skill has the same name".into());
                } else {
                    entry.loadable = true;
                }
            }
            entries.push(entry);
        }
    }
    Ok(SkillCatalog { entries })
}

fn header(path: &Path) -> Result<Header, String> {
    let file =
        harness_core::store::open_private_file(path).map_err(|_| "cannot read skill header")?;
    let mut reader = std::io::BufReader::with_capacity(1024, file.take(65_537));
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|_| "skill header is not UTF-8")?;
    if line.trim() != "---" {
        return Err("skill requires a YAML header".into());
    }
    let mut raw = String::new();
    loop {
        line.clear();
        if reader
            .read_line(&mut line)
            .map_err(|_| "cannot read skill header")?
            == 0
            || raw.len() + line.len() > 65_536
        {
            return Err("skill header is incomplete or exceeds 64 KiB".into());
        }
        if line.trim() == "---" {
            break;
        }
        raw.push_str(&line);
    }
    let header: Header = serde_yaml_ng::from_str(&raw).map_err(|_| "invalid skill metadata")?;
    if header.name.is_empty()
        || header.name.len() > 128
        || !header
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        || header.description.trim().is_empty()
        || header.description.len() > 4096
    {
        return Err("skill requires a safe name and a description under 4 KiB".into());
    }
    Ok(header)
}
