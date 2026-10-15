use super::*;

/// Layered read of one boolean `tui.json` flag plus the file a change should be written to.
pub(super) fn flag_settings(
    explicit_runtime: Option<&Path>,
    context: &ConfigDiscoveryContext,
    pick: fn(&PublicTuiConfig) -> Option<bool>,
    default: bool,
) -> Result<(bool, PathBuf), SettingWriteError> {
    let mut paths = discovery::discover(context, true);
    if let Some(runtime) = explicit_runtime {
        let parent = runtime.parent().unwrap_or(Path::new("."));
        let sibling = ["tui.jsonc", "tui.json"]
            .into_iter()
            .map(|name| parent.join(name))
            .find(|path| path.is_file())
            .unwrap_or_else(|| parent.join("tui.json"));
        paths.push(sibling);
    }
    let mut value = default;
    for path in &paths {
        let config: PublicTuiConfig =
            serde_json::from_value(raw_file(path)?.json()?).map_err(parse_error)?;
        if let Some(layer) = pick(&config) {
            value = layer;
        }
    }
    let path = paths
        .pop()
        .unwrap_or_else(|| context.current_dir.join("tui.json"));
    Ok((value, path))
}

pub(super) fn write_flag(path: &Path, key: &str, enabled: bool) -> Result<(), SettingWriteError> {
    let path = absolute(path)?;
    let _lock = lock(&path)?;
    let raw = raw_file(&path)?;
    let mut edited = raw.json()?;
    edited[key] = Value::Bool(enabled);
    let _: PublicTuiConfig = serde_json::from_value(edited.clone()).map_err(parse_error)?;
    let raw = raw.replace(edited);
    save(&path, &raw)
}

/// Effective `collapse_compacted_history` (default on) and the `tui.json` that a change writes.
pub fn collapse_compacted_history_settings(
    explicit_runtime: Option<&Path>,
    context: &ConfigDiscoveryContext,
) -> Result<(bool, PathBuf), SettingWriteError> {
    flag_settings(
        explicit_runtime,
        context,
        |config| config.collapse_compacted_history,
        true,
    )
}

pub fn write_collapse_compacted_history(
    path: &Path,
    enabled: bool,
) -> Result<(), SettingWriteError> {
    write_flag(path, "collapse_compacted_history", enabled)
}
