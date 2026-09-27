use super::*;
const MAX_BYTES: u64 = 8 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    schema_version: String,
    plugins: Vec<InstalledPlugin>,
}
pub(super) fn io_error(path: &Path, message: &str) -> PluginLifecycleError {
    PluginLifecycleError::RegistryIo {
        path: path.display().to_string(),
        message: message.into(),
    }
}
pub(super) fn load(
    root: &Path,
    path: &Path,
) -> Result<BTreeMap<String, InstalledPlugin>, PluginLifecycleError> {
    let Some(bytes) =
        store::read_private_bytes(path, MAX_BYTES).map_err(|e| io_error(path, &e.to_string()))?
    else {
        return Ok(BTreeMap::new());
    };
    let doc: Document =
        serde_json::from_slice(&bytes).map_err(|_| io_error(path, "invalid registry JSON"))?;
    if doc.schema_version != "harness-plugin-registry.v1" || doc.plugins.len() > 256 {
        return Err(io_error(path, "unsupported registry schema or size"));
    }
    let mut packages = BTreeMap::new();
    for plugin in doc.plugins {
        plugin
            .manifest
            .validate()
            .map_err(|source| PluginLifecycleError::ManifestInvalid {
                path: path.display().to_string(),
                source,
            })?;
        if plugin.id != plugin.manifest.id
            || resolve_under_workspace(root, &plugin.package_root)? != plugin.package_root
            || (!plugin.enablement.is_enabled() && plugin.loaded.is_some())
        {
            return Err(io_error(
                path,
                "invalid plugin identity, path, or load state",
            ));
        }
        if let Some(loaded) = &plugin.loaded {
            if loaded.kinds.is_empty()
                || loaded.kinds.len() > 3
                || loaded.entrypoints.len() > 258
                || loaded.entry_paths.len() > 259
                || Path::new(&loaded.receipt_path)
                    != plugin.package_root.join(PLUGIN_LOAD_RECEIPT_FILE_NAME)
            {
                return Err(io_error(path, "invalid load receipt metadata"));
            }
            for entry in &loaded.entrypoints {
                plugin_load::local_entry(&plugin.package_root, entry).map_err(|source| {
                    PluginLifecycleError::PackageLoadFailed {
                        id: plugin.id.clone(),
                        source,
                    }
                })?;
            }
            for entry in &loaded.entry_paths {
                let relative = Path::new(entry)
                    .strip_prefix(&plugin.package_root)
                    .ok()
                    .and_then(|p| p.to_str())
                    .ok_or_else(|| io_error(path, "entry path escapes plugin"))?;
                plugin_load::local_entry(&plugin.package_root, relative).map_err(|source| {
                    PluginLifecycleError::PackageLoadFailed {
                        id: plugin.id.clone(),
                        source,
                    }
                })?;
            }
        }
        if packages.insert(plugin.id.clone(), plugin).is_some() {
            return Err(io_error(path, "duplicate plugin id"));
        }
    }
    Ok(packages)
}
pub(super) fn commit(
    path: Option<&Path>,
    current: &BTreeMap<String, InstalledPlugin>,
    next: &BTreeMap<String, InstalledPlugin>,
) -> Result<(), PluginLifecycleError> {
    if next.len() > 256 {
        return Err(io_error(
            path.unwrap_or_else(|| Path::new("plugins")),
            "registry exceeds 256 plugins",
        ));
    }
    let doc = Document {
        schema_version: "harness-plugin-registry.v1".into(),
        plugins: next.values().cloned().collect(),
    };
    let bytes = serde_json::to_vec(&doc)
        .map_err(|_| io_error(Path::new("plugins"), "cannot encode registry"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io_error(Path::new("plugins"), "registry exceeds 8 MiB"));
    }
    let mut receipts = BTreeMap::new();
    for (id, before) in current {
        if next.get(id) == Some(before) {
            continue;
        }
        if before.loaded.is_some() {
            receipts.insert(
                before.package_root.join(PLUGIN_LOAD_RECEIPT_FILE_NAME),
                None,
            );
        }
    }
    for (id, after) in next {
        if current.get(id) == Some(after) {
            continue;
        }
        if let Some(loaded) = &after.loaded {
            receipts.insert(
                after.package_root.join(PLUGIN_LOAD_RECEIPT_FILE_NAME),
                Some(plugin_load::receipt_bytes(id, loaded).map_err(|source| {
                    PluginLifecycleError::PackageLoadFailed {
                        id: id.clone(),
                        source,
                    }
                })?),
            );
        }
    }
    let mut backups = Vec::new();
    for (receipt, replacement) in &receipts {
        let previous = store::read_private_bytes(receipt, 1024 * 1024)
            .map_err(|e| io_error(receipt, &e.to_string()))?;
        backups.push((receipt, previous, replacement));
    }
    let result = (|| {
        for (receipt, _, replacement) in &backups {
            plugin_load::replace_receipt(receipt, replacement.as_deref())
                .map_err(|e| io_error(receipt, &e.to_string()))?;
        }
        if let Some(path) = path {
            store::write_private_atomic(path, &bytes)
                .map_err(|e| io_error(path, &e.to_string()))?;
        }
        Ok(())
    })();
    if let Err(original) = result {
        for (receipt, previous, _) in backups.into_iter().rev() {
            if let Err(rollback) = plugin_load::replace_receipt(receipt, previous.as_deref()) {
                return Err(io_error(
                    receipt,
                    &format!("{original}; receipt rollback failed: {rollback}"),
                ));
            }
        }
        return Err(original);
    }
    Ok(())
}
