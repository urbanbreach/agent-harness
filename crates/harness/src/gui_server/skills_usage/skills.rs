use super::*;

fn contents(directory: &Path, depth: usize) -> Result<(u64, u64, Option<u64>), String> {
    if depth > 32 {
        return Err("skill contents exceed 32 directory levels".into());
    }
    let (mut files, mut bytes, mut updated) = (0, 0, None);
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        // Never follow supporting-file links outside the skill directory.
        if entry.file_type().map_err(|e| e.to_string())?.is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            let (f, b, u) = contents(&entry.path(), depth + 1)?;
            files += f;
            bytes += b;
            updated = updated.max(u);
        } else if metadata.is_file() {
            files += 1;
            bytes += metadata.len();
            updated = updated.max(
                metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .and_then(|d| u64::try_from(d.as_millis()).ok()),
            );
        }
    }
    Ok((files, bytes, updated))
}

pub(super) fn skills(
    cwd: &Path,
    home: &Path,
    config: &SkillsConfig,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    let mut discovery = config.clone();
    for root in &mut discovery.global_roots {
        if root == Path::new("~/.harness/skills") {
            *root = home.join("skills");
        }
    }
    let native = harness_tools::discover_skill_installations_with_config(cwd, &discovery)
        .map_err(|e| e.to_string())?;
    let mut grouped: BTreeMap<String, Value> = BTreeMap::new();
    let mut errors = Vec::new();
    for installation in native {
        let entry = installation.entry;
        let file = &entry.location;
        let root = &entry.root_path;
        let scope = if entry.source_scope == "project" {
            "project"
        } else {
            "user"
        };
        let enabled = installation.enabled;
        let directory = file.parent().ok_or("skill manifest has no directory")?;
        if let Some(reason) = &entry.reason
            && entry.status == harness_tools::SkillCatalogStatus::Malformed
        {
            errors.push(format!("{}: {reason}", file.display()));
        }
        let name = &entry.name;
        let description = &entry.description;
        let tools = &entry.allowed_tools;
        let id = format!("{scope}:{name}");
        let (files, size, updated) = contents(directory, 0)?;
        let install = json!({"directory":directory,"file":file,"root":root,"scope":scope,"source":if root.components().any(|c| c.as_os_str() == ".agents") {"Shared"} else {"Harness"},"enabled":enabled});
        if let Some(row) = grouped.get_mut(&id) {
            row["enabled"] = json!(row["enabled"].as_bool().unwrap_or(false) && enabled);
            if let Some(installs) = row["installs"].as_array_mut() {
                installs.push(install);
            }
            row["supportingFiles"] =
                json!(row["supportingFiles"].as_u64().unwrap_or(0) + files.saturating_sub(1));
            row["sizeBytes"] = json!(row["sizeBytes"].as_u64().unwrap_or(0) + size);
            row["updatedAt"] = json!(row["updatedAt"].as_u64().max(updated));
        } else {
            grouped.insert(id.clone(), json!({"id":id,"name":name,"description":description,"scope":scope,"project":if scope == "project" {Some(cwd)} else {None},"enabled":enabled,"body":installation.body,"allowedTools":tools,"primaryDirectory":directory,"primaryFile":file,"installs":[install],"supportingFiles":files.saturating_sub(1),"sizeBytes":size,"updatedAt":updated}));
        }
    }
    if method == "skills.list" {
        return Ok(json!({"skills":grouped.into_values().collect::<Vec<_>>(),"errors":errors}));
    }
    let id = super::super::required(params, "id")?;
    let row = grouped.get(id).ok_or("not_found: skill not found")?;
    if method == "skills.read" {
        return Ok(json!({"id":id,"body":row["body"]}));
    }
    let installs = row["installs"].as_array().ok_or("invalid skill installs")?;
    let enabled = if method == "skills.setEnabled" {
        Some(
            params["enabled"]
                .as_bool()
                .ok_or("invalid_params: enabled must be a boolean")?,
        )
    } else {
        None
    };
    let mut operations = Vec::new();
    for install in installs {
        let root = Path::new(super::super::required(install, "root")?);
        let file = Path::new(super::super::required(install, "file")?);
        let directory = validated_directory(root, file)?;
        if let Some(enabled) = enabled {
            let destination = directory.join(if enabled {
                "SKILL.md"
            } else {
                "SKILL.md.disabled"
            });
            if destination != file {
                if fs::symlink_metadata(&destination).is_ok() {
                    return Err("conflict: both enabled and disabled manifests exist".into());
                }
                operations.push((file.to_path_buf(), destination));
            }
        } else {
            operations.push((directory, PathBuf::new()));
        }
    }
    let mut changed = 0;
    for (source, destination) in operations {
        let result = if enabled.is_some() {
            fs::rename(&source, &destination).map_err(|e| e.to_string())
        } else {
            // gio implements the desktop user's trash, including its restore metadata.
            std::process::Command::new("gio")
                .arg("trash")
                .arg("--")
                .arg(&source)
                .output()
                .map_err(|_| "unsupported: desktop trash service (gio) is unavailable".to_owned())
                .and_then(|output| {
                    if output.status.success() {
                        Ok(())
                    } else {
                        Err("desktop trash operation failed".into())
                    }
                })
        };
        result.map_err(|error| format!("{error}; {changed} installs changed before failure"))?;
        changed += 1;
    }
    Ok(if let Some(enabled) = enabled {
        json!({"id":id,"enabled":enabled,"changed":changed})
    } else {
        json!({"id":id,"trashed":changed})
    })
}

pub(super) fn validated_directory(root: &Path, file: &Path) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let directory = file.parent().ok_or("invalid skill path")?;
    if !matches!(
        file.file_name().and_then(|s| s.to_str()),
        Some("SKILL.md" | "SKILL.md.disabled")
    ) {
        return Err("invalid skill manifest".into());
    }
    let canonical = directory.canonicalize().map_err(|e| e.to_string())?;
    if canonical == root || !canonical.starts_with(&root) {
        return Err(
            "invalid_params: skill directory must be strictly inside a discovered root".into(),
        );
    }
    let relative = file
        .strip_prefix(&root)
        .map_err(|_| "invalid_params: skill path escapes root")?;
    let mut path = root.clone();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("invalid skill path component".into());
        }
        path.push(component);
        if fs::symlink_metadata(&path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("invalid_params: skill mutation cannot follow symlinks".into());
        }
    }
    Ok(canonical)
}
