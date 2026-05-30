use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use harness_core::config::{registered_skills_config, PermissionMode, SkillsConfig};
use harness_core::tool::ToolError;
use serde::{Deserialize, Serialize};

use crate::text::has_trimmed_content;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCatalog {
    pub entries: Vec<SkillCatalogEntry>,
}

impl SkillCatalog {
    pub fn active_entry(&self, name: &str) -> Option<&SkillCatalogEntry> {
        self.entries
            .iter()
            .find(|entry| entry.name == name && entry.status != SkillCatalogStatus::Shadowed)
    }

    pub fn entries_for(&self, name: &str) -> Vec<&SkillCatalogEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.name == name)
            .collect()
    }

    pub(crate) fn loadable_names(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|entry| entry.status == SkillCatalogStatus::Loadable)
            .map(|entry| entry.name.as_str())
            .take(5)
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCatalogEntry {
    pub stable_id: String,
    pub name: String,
    pub description: String,
    pub source_scope: String,
    pub root_path: PathBuf,
    pub location: PathBuf,
    pub loadable: bool,
    pub permission_mode: String,
    pub status: SkillCatalogStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deferred_mcp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deferred_resources: Option<String>,
    pub body_loaded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_digest: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillCatalogStatus {
    Loadable,
    Denied,
    Disabled,
    Malformed,
    Shadowed,
}

impl SkillCatalogStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Loadable => "loadable",
            Self::Denied => "denied",
            Self::Disabled => "disabled",
            Self::Malformed => "malformed",
            Self::Shadowed => "shadowed",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ActivatedSkill {
    pub(crate) metadata: SkillCatalogEntry,
    pub(crate) content: String,
}

#[derive(Debug, Clone, Default)]
struct ParsedSkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    argument_hint: Option<String>,
    allowed_tools: Vec<String>,
    target_agent: Option<String>,
    target_category: Option<String>,
    deferred_mcp: Option<String>,
    deferred_resources: Option<String>,
}

struct FrontmatterFieldLine<'a> {
    key: &'a str,
    raw_value: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillSearchDir {
    path: PathBuf,
    project_base: Option<PathBuf>,
    source_scope: SkillSourceScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkillSourceScope {
    Project,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkillRootKind {
    HarnessOwned,
    Custom,
    Compatibility,
}

impl SkillSourceScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
        }
    }
}

pub fn discover_skill_catalog(workspace_root: &Path) -> Result<SkillCatalog, ToolError> {
    let config = registered_skills_config();
    discover_skill_catalog_with_config(workspace_root, &config)
}

pub(crate) fn activate_skill(entry: &SkillCatalogEntry) -> Result<ActivatedSkill, ToolError> {
    if entry.status != SkillCatalogStatus::Loadable {
        return Err(ToolError::Execution(format!(
            "Skill \"{}\" not found: skill is {}{}",
            entry.name,
            entry.status.as_str(),
            entry
                .reason
                .as_deref()
                .map(|reason| format!(" ({reason})"))
                .unwrap_or_default()
        )));
    }
    let content = std::fs::read_to_string(&entry.location).map_err(|err| {
        ToolError::Execution(format!(
            "failed to read skill file {}: {err}",
            entry.location.display()
        ))
    })?;
    Ok(ActivatedSkill {
        metadata: entry.clone(),
        content,
    })
}

pub fn discover_skill_catalog_with_config(
    workspace_root: &Path,
    config: &SkillsConfig,
) -> Result<SkillCatalog, ToolError> {
    let mut entries = Vec::new();
    let mut active_names = BTreeSet::new();
    for search_dir in skill_search_dirs(workspace_root, config) {
        if !search_dir.path.exists() {
            continue;
        }
        let canonical_dir = search_dir.path.canonicalize().map_err(|err| {
            ToolError::Execution(format!(
                "failed to resolve skill directory {}: {err}",
                search_dir.path.display()
            ))
        })?;
        if let Some(base_dir) = search_dir.project_base.as_deref() {
            let canonical_base = base_dir.canonicalize().map_err(|err| {
                ToolError::Execution(format!(
                    "failed to resolve skill search base {}: {err}",
                    base_dir.display()
                ))
            })?;
            if !canonical_dir.starts_with(canonical_base) {
                continue;
            }
        }

        for entry in sorted_skill_entries(&canonical_dir)? {
            let directory_name = entry.file_name().to_string_lossy().to_string();
            let source_scope = search_dir.source_scope.as_str();
            let stable_id = stable_skill_id(source_scope, &directory_name);
            let permission = resolve_skill_permission(&directory_name, &config.permissions);
            let disabled = skill_disabled(&directory_name, &stable_id, &config.disabled);

            let mut metadata = if entry
                .file_type()
                .map_err(|err| {
                    ToolError::Execution(format!("failed to inspect skill entry: {err}"))
                })?
                .is_symlink()
            {
                malformed_entry(
                    &directory_name,
                    &stable_id,
                    source_scope,
                    &canonical_dir,
                    entry.path().join("SKILL.md"),
                    permission,
                    "skill directory is a symlink",
                )
            } else {
                let skill_file = entry.path().join("SKILL.md");
                if !skill_file.exists() {
                    continue;
                }
                let canonical_skill_file = match skill_file.canonicalize() {
                    Ok(path) if path.starts_with(&canonical_dir) => path,
                    Ok(path) => {
                        entries.push(malformed_entry(
                            &directory_name,
                            &stable_id,
                            source_scope,
                            &canonical_dir,
                            path,
                            permission,
                            "skill file resolves outside its root",
                        ));
                        continue;
                    }
                    Err(err) => {
                        entries.push(malformed_entry(
                            &directory_name,
                            &stable_id,
                            source_scope,
                            &canonical_dir,
                            skill_file,
                            permission,
                            &format!("failed to resolve skill file: {err}"),
                        ));
                        continue;
                    }
                };
                match read_frontmatter_text(&canonical_skill_file)
                    .and_then(|frontmatter| parse_skill_frontmatter(&frontmatter))
                    .and_then(|frontmatter| {
                        build_skill_metadata(
                            SkillMetadataBuildContext {
                                directory_name: &directory_name,
                                stable_id: &stable_id,
                                source_scope,
                                root_path: &canonical_dir,
                                skill_file: &canonical_skill_file,
                                permission: permission.clone(),
                                disabled,
                            },
                            frontmatter,
                        )
                    }) {
                    Ok(entry) => entry,
                    Err(reason) => malformed_entry(
                        &directory_name,
                        &stable_id,
                        source_scope,
                        &canonical_dir,
                        canonical_skill_file,
                        permission,
                        &reason,
                    ),
                }
            };

            if active_names.contains(&metadata.name) {
                metadata.status = SkillCatalogStatus::Shadowed;
                metadata.loadable = false;
                metadata.reason = Some(format!(
                    "shadowed by a higher-precedence `{}` skill",
                    metadata.name
                ));
            } else {
                active_names.insert(metadata.name.clone());
            }
            entries.push(metadata);
        }
    }

    Ok(SkillCatalog { entries })
}

struct SkillMetadataBuildContext<'a> {
    directory_name: &'a str,
    stable_id: &'a str,
    source_scope: &'a str,
    root_path: &'a Path,
    skill_file: &'a Path,
    permission: PermissionMode,
    disabled: bool,
}

fn build_skill_metadata(
    ctx: SkillMetadataBuildContext<'_>,
    frontmatter: ParsedSkillFrontmatter,
) -> Result<SkillCatalogEntry, String> {
    let name = frontmatter.name.ok_or_else(|| {
        format!(
            "skill {} is missing required frontmatter `name`",
            ctx.skill_file.display()
        )
    })?;
    validate_skill_name(&name, ctx.skill_file)?;
    if name != ctx.directory_name {
        return Err(format!(
            "skill {} frontmatter `name` `{name}` must match directory `{}`",
            ctx.skill_file.display(),
            ctx.directory_name
        ));
    }
    let description = frontmatter.description.ok_or_else(|| {
        format!(
            "skill {} is missing required frontmatter `description`",
            ctx.skill_file.display()
        )
    })?;
    let description_len = description.chars().count();
    if !(1..=1024).contains(&description_len) {
        return Err(format!(
            "skill {} frontmatter `description` must be 1-1024 characters",
            ctx.skill_file.display()
        ));
    }

    let (status, loadable, reason) = if ctx.disabled {
        (
            SkillCatalogStatus::Disabled,
            false,
            Some("disabled by skills.disabled".to_string()),
        )
    } else if ctx.permission == PermissionMode::Deny {
        (
            SkillCatalogStatus::Denied,
            false,
            Some("denied by skills.permissions".to_string()),
        )
    } else {
        (SkillCatalogStatus::Loadable, true, None)
    };

    Ok(SkillCatalogEntry {
        stable_id: ctx.stable_id.to_string(),
        name,
        description,
        source_scope: ctx.source_scope.to_string(),
        root_path: ctx.root_path.to_path_buf(),
        location: ctx.skill_file.to_path_buf(),
        loadable,
        permission_mode: permission_mode_label(&ctx.permission).to_string(),
        status,
        reason,
        argument_hint: frontmatter.argument_hint,
        allowed_tools: frontmatter.allowed_tools,
        target_agent: frontmatter.target_agent,
        target_category: frontmatter.target_category,
        deferred_mcp: frontmatter.deferred_mcp,
        deferred_resources: frontmatter.deferred_resources,
        body_loaded: false,
        body_digest: None,
    })
}

fn malformed_entry(
    directory_name: &str,
    stable_id: &str,
    source_scope: &str,
    root_path: &Path,
    skill_file: PathBuf,
    permission: PermissionMode,
    reason: &str,
) -> SkillCatalogEntry {
    SkillCatalogEntry {
        stable_id: stable_id.to_string(),
        name: directory_name.to_string(),
        description: String::new(),
        source_scope: source_scope.to_string(),
        root_path: root_path.to_path_buf(),
        location: skill_file,
        loadable: false,
        permission_mode: permission_mode_label(&permission).to_string(),
        status: SkillCatalogStatus::Malformed,
        reason: Some(reason.to_string()),
        argument_hint: None,
        allowed_tools: Vec::new(),
        target_agent: None,
        target_category: None,
        deferred_mcp: None,
        deferred_resources: None,
        body_loaded: false,
        body_digest: None,
    }
}

fn read_frontmatter_text(skill_file: &Path) -> Result<String, String> {
    let content = std::fs::read_to_string(skill_file)
        .map_err(|err| format!("failed to read skill file {}: {err}", skill_file.display()))?;
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return Err("skill frontmatter must start with `---`".to_string());
    }
    let mut frontmatter = String::from("---\n");
    let mut found_closing = false;
    for line in lines {
        frontmatter.push_str(line);
        frontmatter.push('\n');
        if line == "---" {
            found_closing = true;
            break;
        }
    }
    if !found_closing {
        return Err("skill frontmatter must end with `---`".to_string());
    }
    Ok(frontmatter)
}

fn parse_skill_frontmatter(content: &str) -> Result<ParsedSkillFrontmatter, String> {
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return Err("skill frontmatter must start with `---`".to_string());
    }

    let mut frontmatter_lines = Vec::new();
    let mut found_closing = false;
    for line in lines {
        if line == "---" {
            found_closing = true;
            break;
        }
        frontmatter_lines.push(line);
    }

    if !found_closing {
        return Err("skill frontmatter must end with `---`".to_string());
    }

    parse_frontmatter_fields(&frontmatter_lines)
}

fn parse_frontmatter_fields(lines: &[&str]) -> Result<ParsedSkillFrontmatter, String> {
    let mut frontmatter = ParsedSkillFrontmatter::default();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        if !has_trimmed_content(line) {
            index += 1;
            continue;
        }
        if leading_indent(line)? != 0 {
            return Err(format!(
                "frontmatter line `{}` must not be indented",
                line.trim()
            ));
        }

        let field = parse_frontmatter_field_line(line, |trimmed| {
            format!("frontmatter line `{trimmed}` must use `key: value` syntax")
        })?;

        match field.key {
            "name" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.name = Some(value);
                index = next_index;
            }
            "description" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.description = Some(value);
                index = next_index;
            }
            "argument_hint" | "argumentHint" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.argument_hint = non_empty(value);
                index = next_index;
            }
            "allowed_tools" | "allowedTools" | "expected_tools" | "expectedTools" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.allowed_tools = parse_list_field(&value);
                index = next_index;
            }
            "target_agent" | "targetAgent" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.target_agent = non_empty(value);
                index = next_index;
            }
            "target_category" | "targetCategory" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.target_category = non_empty(value);
                index = next_index;
            }
            "mcp" | "deferred_mcp" | "deferredMcp" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.deferred_mcp = non_empty(value);
                index = next_index;
            }
            "resources" | "deferred_resources" | "deferredResources" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.deferred_resources = non_empty(value);
                index = next_index;
            }
            "license" | "compatibility" => {
                let (_, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                index = next_index;
            }
            "metadata" => {
                index = parse_metadata_field(lines, index, field.raw_value)?;
            }
            _ => {
                return Err(format!(
                    "unsupported V1 skill frontmatter field `{}`",
                    field.key
                ));
            }
        }
    }

    Ok(frontmatter)
}

fn parse_frontmatter_field_line<'a>(
    line: &'a str,
    syntax_error: impl FnOnce(&str) -> String,
) -> Result<FrontmatterFieldLine<'a>, String> {
    let trimmed = line.trim_end();
    let (key, raw_value) = trimmed
        .split_once(':')
        .ok_or_else(|| syntax_error(trimmed))?;
    Ok(FrontmatterFieldLine {
        key: key.trim(),
        raw_value: raw_value.trim_start(),
    })
}

fn parse_scalar_field(
    lines: &[&str],
    index: usize,
    raw_value: &str,
) -> Result<(String, usize), String> {
    match raw_value {
        ">" => {
            let (value, next_index) = collect_block_scalar(lines, index + 1, 0)?;
            Ok((
                value
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
                next_index,
            ))
        }
        "|" => collect_block_scalar(lines, index + 1, 0),
        "" => {
            if next_line_is_indented(lines, index + 1)? {
                return Err("frontmatter scalar fields must not use nested mappings".to_string());
            }
            Ok((String::new(), index + 1))
        }
        value => Ok((parse_inline_scalar(value), index + 1)),
    }
}

fn parse_metadata_field(lines: &[&str], index: usize, raw_value: &str) -> Result<usize, String> {
    if !raw_value.is_empty() {
        return Err("frontmatter `metadata` must be a string-to-string map".to_string());
    }

    let mut cursor = index + 1;
    while cursor < lines.len() {
        let line = lines[cursor];
        if !has_trimmed_content(line) {
            cursor += 1;
            continue;
        }

        let indent = leading_indent(line)?;
        if indent == 0 {
            break;
        }

        let field = parse_frontmatter_field_line(line.trim(), |trimmed| {
            format!("frontmatter `metadata` entry `{trimmed}` must use `key: value` syntax")
        })?;
        if !has_trimmed_content(field.key) {
            return Err("frontmatter `metadata` keys must not be empty".to_string());
        }

        match field.raw_value {
            ">" | "|" => {
                let (_, next_index) = collect_block_scalar(lines, cursor + 1, indent)?;
                cursor = next_index;
            }
            "" => {
                if next_line_has_deeper_indent(lines, cursor + 1, indent)? {
                    return Err("frontmatter `metadata` must be a string-to-string map".to_string());
                }
                cursor += 1;
            }
            _ => {
                cursor += 1;
            }
        }
    }

    Ok(cursor)
}

fn collect_block_scalar(
    lines: &[&str],
    start_index: usize,
    parent_indent: usize,
) -> Result<(String, usize), String> {
    let mut cursor = start_index;
    let mut block_indent = None;
    let mut values = Vec::new();

    while cursor < lines.len() {
        let line = lines[cursor];
        if !has_trimmed_content(line) {
            values.push(String::new());
            cursor += 1;
            continue;
        }

        let indent = leading_indent(line)?;
        if indent <= parent_indent {
            break;
        }

        let block_indent = *block_indent.get_or_insert(indent);
        if indent < block_indent {
            break;
        }
        values.push(line[block_indent..].to_string());
        cursor += 1;
    }

    Ok((values.join("\n"), cursor))
}

fn next_line_is_indented(lines: &[&str], start_index: usize) -> Result<bool, String> {
    Ok(next_non_empty_line_indent(lines, start_index)?.is_some_and(|indent| indent > 0))
}

fn next_line_has_deeper_indent(
    lines: &[&str],
    start_index: usize,
    current_indent: usize,
) -> Result<bool, String> {
    Ok(next_non_empty_line_indent(lines, start_index)?
        .is_some_and(|indent| indent > current_indent))
}

fn next_non_empty_line_indent(lines: &[&str], start_index: usize) -> Result<Option<usize>, String> {
    for line in &lines[start_index..] {
        if !has_trimmed_content(line) {
            continue;
        }
        return leading_indent(line).map(Some);
    }
    Ok(None)
}

fn leading_indent(line: &str) -> Result<usize, String> {
    let indent = line
        .chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count();
    if line[..indent].contains('\t') {
        return Err("frontmatter indentation must use spaces".to_string());
    }
    Ok(indent)
}

fn parse_inline_scalar(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        if (bytes[0] == b'"' && bytes[trimmed.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[trimmed.len() - 1] == b'\'')
        {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

fn parse_list_field(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .filter_map(|item| non_empty(item.trim().trim_matches('"').trim_matches('\'').to_string()))
        .collect()
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn validate_skill_name(name: &str, skill_file: &Path) -> Result<(), String> {
    let len = name.chars().count();
    if !(1..=64).contains(&len) {
        return Err(format!(
            "skill {} frontmatter `name` must be 1-64 characters",
            skill_file.display()
        ));
    }
    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return Err(format!(
            "skill {} frontmatter `name` must use single hyphen separators",
            skill_file.display()
        ));
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(format!(
            "skill {} frontmatter `name` must match `^[a-z0-9]+(-[a-z0-9]+)*$`",
            skill_file.display()
        ));
    }
    Ok(())
}

fn skill_search_dirs(current_dir: &Path, config: &SkillsConfig) -> Vec<SkillSearchDir> {
    let mut dirs = Vec::new();
    let project_roots = ordered_skill_roots(&config.project_roots);
    let global_roots = ordered_skill_roots(&config.global_roots);

    for base_dir in project_search_bases(current_dir, config.walk_to_git_root) {
        push_project_search_dirs(&mut dirs, &base_dir, &project_roots, false);
    }

    push_global_search_dirs(&mut dirs, current_dir, &global_roots, false);

    for base_dir in project_search_bases(current_dir, config.walk_to_git_root) {
        push_project_search_dirs(&mut dirs, &base_dir, &project_roots, true);
    }

    push_global_search_dirs(&mut dirs, current_dir, &global_roots, true);

    dirs
}

fn push_project_search_dirs(
    dirs: &mut Vec<SkillSearchDir>,
    base_dir: &Path,
    roots: &[(&PathBuf, SkillRootKind)],
    compatibility: bool,
) {
    for (root, kind) in roots {
        if (*kind == SkillRootKind::Compatibility) != compatibility {
            continue;
        }
        push_unique_search_dir(
            dirs,
            SkillSearchDir {
                path: resolve_skill_root(base_dir, root),
                project_base: Some(base_dir.to_path_buf()),
                source_scope: SkillSourceScope::Project,
            },
        );
    }
}

fn push_global_search_dirs(
    dirs: &mut Vec<SkillSearchDir>,
    current_dir: &Path,
    roots: &[(&PathBuf, SkillRootKind)],
    compatibility: bool,
) {
    for (root, kind) in roots {
        if (*kind == SkillRootKind::Compatibility) != compatibility {
            continue;
        }
        push_unique_search_dir(
            dirs,
            SkillSearchDir {
                path: resolve_skill_root(current_dir, root),
                project_base: None,
                source_scope: SkillSourceScope::Global,
            },
        );
    }
}

fn ordered_skill_roots(roots: &[PathBuf]) -> Vec<(&PathBuf, SkillRootKind)> {
    let mut ordered = roots
        .iter()
        .enumerate()
        .map(|(index, root)| (index, root, classify_skill_root(root)))
        .collect::<Vec<_>>();
    ordered.sort_by_key(|(index, _, kind)| (skill_root_kind_rank(*kind), *index));
    ordered
        .into_iter()
        .map(|(_, root, kind)| (root, kind))
        .collect()
}

fn skill_root_kind_rank(kind: SkillRootKind) -> usize {
    match kind {
        SkillRootKind::HarnessOwned => 0,
        SkillRootKind::Custom => 1,
        SkillRootKind::Compatibility => 2,
    }
}

fn classify_skill_root(root: &Path) -> SkillRootKind {
    if root.ends_with(".agent-harness/skills")
        || root.ends_with(".harness/skills")
        || root.ends_with(".config/agent-harness/skills")
    {
        SkillRootKind::HarnessOwned
    } else if root.ends_with(".external-editor/skills")
        || root.ends_with(".assistant/skills")
        || root.ends_with(".agents/skills")
    {
        SkillRootKind::Compatibility
    } else {
        SkillRootKind::Custom
    }
}

fn project_search_bases(current_dir: &Path, walk_to_git_root: bool) -> Vec<PathBuf> {
    if !walk_to_git_root {
        return vec![current_dir.to_path_buf()];
    }

    let ancestors = current_dir
        .ancestors()
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    if let Some(git_root_index) = ancestors.iter().position(|path| path.join(".git").exists()) {
        return ancestors.into_iter().take(git_root_index + 1).collect();
    }

    vec![current_dir.to_path_buf()]
}

fn push_unique_search_dir(paths: &mut Vec<SkillSearchDir>, candidate: SkillSearchDir) {
    if !paths.iter().any(|existing| existing.path == candidate.path) {
        paths.push(candidate);
    }
}

fn resolve_skill_root(base_dir: &Path, root: &Path) -> PathBuf {
    let expanded = expand_home(root);
    if expanded.is_absolute() {
        expanded
    } else {
        base_dir.join(expanded)
    }
}

fn expand_home(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| path.to_path_buf());
    }
    if let Some(stripped) = text.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    path.to_path_buf()
}

fn sorted_skill_entries(dir: &Path) -> Result<Vec<std::fs::DirEntry>, ToolError> {
    let mut entries = std::fs::read_dir(dir)
        .map_err(|err| {
            ToolError::Execution(format!(
                "failed to read skill directory {}: {err}",
                dir.display()
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| ToolError::Execution(format!("failed to read skill entry: {err}")))?;
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}

fn resolve_skill_permission(
    name: &str,
    permissions: &BTreeMap<String, PermissionMode>,
) -> PermissionMode {
    permissions
        .iter()
        .filter(|(pattern, _)| skill_name_matches_pattern(name, pattern))
        .max_by(|(left, _), (right, _)| compare_permission_patterns(left, right))
        .map(|(_, mode)| mode.clone())
        .unwrap_or(PermissionMode::Allow)
}

fn skill_disabled(name: &str, stable_id: &str, disabled: &[String]) -> bool {
    disabled
        .iter()
        .any(|pattern| skill_name_matches_pattern(name, pattern) || stable_id == pattern)
}

fn compare_permission_patterns(left: &str, right: &str) -> std::cmp::Ordering {
    skill_pattern_specificity(left)
        .cmp(&skill_pattern_specificity(right))
        .then_with(|| left.cmp(right))
}

fn skill_pattern_specificity(pattern: &str) -> (bool, usize, usize) {
    let non_wildcard_len = pattern.chars().filter(|ch| *ch != '*').count();
    (!pattern.contains('*'), non_wildcard_len, pattern.len())
}

fn skill_name_matches_pattern(name: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let segments = pattern.split('*').collect::<Vec<_>>();
    let anchored_start = !pattern.starts_with('*');
    let anchored_end = !pattern.ends_with('*');
    let mut remainder = name;

    for (index, segment) in segments.iter().enumerate() {
        if segment.is_empty() {
            continue;
        }

        if index == 0 && anchored_start {
            let Some(stripped) = remainder.strip_prefix(segment) else {
                return false;
            };
            remainder = stripped;
            continue;
        }

        if index == segments.len() - 1 && anchored_end {
            return remainder.ends_with(segment);
        }

        let Some(position) = remainder.find(segment) else {
            return false;
        };
        remainder = &remainder[position + segment.len()..];
    }

    !anchored_end
        || segments.last().is_some_and(|segment| segment.is_empty())
        || remainder.is_empty()
}

fn stable_skill_id(source_scope: &str, name: &str) -> String {
    format!("skill:{source_scope}:{name}")
}

fn permission_mode_label(mode: &PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Allow => "allow",
        PermissionMode::Deny => "deny",
        PermissionMode::Ask => "ask",
    }
}
