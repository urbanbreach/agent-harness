use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use harness_core::redact::{DefaultRedactor, Redactor};
use harness_core::tool::ToolError;

use super::SkillCatalogEntry;

const MAX_SKILL_RESOURCE_FILES: usize = 5;
const MAX_SKILL_RESOURCE_FILE_BYTES: usize = 64 * 1024;
const MAX_SKILL_RESOURCE_TOTAL_BYTES: usize = 200 * 1024;
const MAX_SKILL_RESOURCE_PATH_DEPTH: usize = 4;

#[derive(Debug)]
struct LoadedSkillResource {
    requested_path: String,
    bytes_loaded: usize,
    truncated: bool,
    content: String,
}

pub(super) fn append_bundled_skill_resources(
    content: &mut String,
    entry: &SkillCatalogEntry,
    redactor: &DefaultRedactor,
) -> Result<(), ToolError> {
    let resources = load_bundled_skill_resources(entry, redactor)?;
    if !resources.is_empty() {
        content.push_str(&render_bundled_skill_resources(&resources));
    }
    Ok(())
}

fn load_bundled_skill_resources(
    entry: &SkillCatalogEntry,
    redactor: &DefaultRedactor,
) -> Result<Vec<LoadedSkillResource>, ToolError> {
    let Some(spec) = entry.deferred_resources.as_deref() else {
        return Ok(Vec::new());
    };
    let resource_paths = parse_skill_resource_spec(spec).map_err(|reason| {
        ToolError::Execution(format!(
            "Skill \"{}\" resources are invalid: {reason}",
            entry.name
        ))
    })?;
    if resource_paths.is_empty() {
        return Ok(Vec::new());
    }
    if resource_paths.len() > MAX_SKILL_RESOURCE_FILES {
        return Err(ToolError::Execution(format!(
            "Skill \"{}\" declares {} resources; max {MAX_SKILL_RESOURCE_FILES}",
            entry.name,
            resource_paths.len()
        )));
    }

    let skill_dir = entry.location.parent().unwrap_or(entry.location.as_path());
    let canonical_skill_dir = skill_dir.canonicalize().map_err(|err| {
        ToolError::Execution(format!(
            "failed to resolve skill directory {}: {err}",
            skill_dir.display()
        ))
    })?;
    let mut loaded = Vec::new();
    let mut total_bytes_loaded = 0usize;
    for requested_path in resource_paths {
        let safe_relative = validate_skill_resource_path(&requested_path).map_err(|reason| {
            ToolError::Execution(format!(
                "Skill \"{}\" resource `{requested_path}` is invalid: {reason}",
                entry.name
            ))
        })?;
        let canonical_resource = canonical_skill_resource_path(
            &entry.name,
            &canonical_skill_dir,
            &requested_path,
            &safe_relative,
        )?;
        let remaining_budget = MAX_SKILL_RESOURCE_TOTAL_BYTES.saturating_sub(total_bytes_loaded);
        if remaining_budget == 0 {
            return Err(ToolError::Execution(format!(
                "Skill \"{}\" bundled resources exceed total cap of {MAX_SKILL_RESOURCE_TOTAL_BYTES} bytes",
                entry.name
            )));
        }
        let per_resource_budget = MAX_SKILL_RESOURCE_FILE_BYTES.min(remaining_budget);
        let (content, bytes_loaded, truncated) =
            read_capped_resource(&canonical_resource, per_resource_budget, redactor).map_err(
                |err| {
                    ToolError::Execution(format!(
                        "failed to read skill \"{}\" resource `{requested_path}`: {err}",
                        entry.name
                    ))
                },
            )?;
        total_bytes_loaded += bytes_loaded;
        loaded.push(LoadedSkillResource {
            requested_path,
            bytes_loaded,
            truncated,
            content,
        });
    }
    Ok(loaded)
}

fn parse_skill_resource_spec(spec: &str) -> Result<Vec<String>, String> {
    let resources = spec
        .split([',', '\n'])
        .filter_map(|resource| {
            let trimmed = resource.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .collect::<Vec<_>>();
    if resources.iter().any(|resource| resource.contains('*')) {
        return Err("directories and globs are out of scope for V1 resources".to_string());
    }
    Ok(resources)
}

fn validate_skill_resource_path(requested_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(requested_path);
    if path.is_absolute() {
        return Err("absolute paths are not allowed".to_string());
    }
    let mut depth = 0usize;
    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                depth += 1;
                if depth > MAX_SKILL_RESOURCE_PATH_DEPTH {
                    return Err(format!(
                        "path depth exceeds max {MAX_SKILL_RESOURCE_PATH_DEPTH}"
                    ));
                }
                safe.push(part);
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err("`..` path traversal is not allowed".to_string());
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("rooted paths are not allowed".to_string());
            }
        }
    }
    if safe.as_os_str().is_empty() {
        return Err("resource path must not be empty".to_string());
    }
    Ok(safe)
}

fn canonical_skill_resource_path(
    skill_name: &str,
    canonical_skill_dir: &Path,
    requested_path: &str,
    safe_relative: &Path,
) -> Result<PathBuf, ToolError> {
    let candidate = canonical_skill_dir.join(safe_relative);
    let canonical_resource = candidate.canonicalize().map_err(|err| {
        ToolError::Execution(format!(
            "failed to resolve skill \"{skill_name}\" resource `{requested_path}`: {err}"
        ))
    })?;
    if !canonical_resource.starts_with(canonical_skill_dir) {
        return Err(ToolError::Execution(format!(
            "Skill \"{skill_name}\" resource `{requested_path}` resolves outside its skill root"
        )));
    }
    let metadata = std::fs::metadata(&canonical_resource).map_err(|err| {
        ToolError::Execution(format!(
            "failed to inspect skill \"{skill_name}\" resource `{requested_path}`: {err}"
        ))
    })?;
    if !metadata.is_file() {
        return Err(ToolError::Execution(format!(
            "Skill \"{skill_name}\" resource `{requested_path}` is not a regular file"
        )));
    }
    Ok(canonical_resource)
}

fn read_capped_resource(
    path: &Path,
    budget: usize,
    redactor: &DefaultRedactor,
) -> Result<(String, usize, bool), std::io::Error> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(u64::try_from(budget.saturating_add(1)).unwrap_or(0))
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() > budget;
    if truncated {
        bytes.truncate(budget);
    }
    let bytes_loaded = bytes.len();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok((redactor.redact_text(&text), bytes_loaded, truncated))
}

fn render_bundled_skill_resources(resources: &[LoadedSkillResource]) -> String {
    let mut output = String::from("\n\n## Bundled resources\n");
    for resource in resources {
        output.push_str(&format!(
            "\n<skill_resource path=\"{}\" bytes_loaded=\"{}\" truncated=\"{}\">\n{}\n</skill_resource>\n",
            escape_resource_attr(&resource.requested_path),
            resource.bytes_loaded,
            resource.truncated,
            resource.content.trim_end()
        ));
        if resource.truncated {
            output.push_str(&format!(
                "\n[Resource `{}` was capped at the V1 skill resource byte limit.]\n",
                resource.requested_path
            ));
        }
    }
    output
}

fn escape_resource_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
