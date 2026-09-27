use super::*;

pub fn rewind_confirmation_settings(
    explicit_runtime: Option<&Path>,
    context: &ConfigDiscoveryContext,
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
    let mut confirmed = true;
    for path in &paths {
        let config: PublicTuiConfig =
            serde_json::from_value(raw_file(path)?.json()?).map_err(parse_error)?;
        if let Some(value) = config.confirm_before_rewind {
            confirmed = value;
        }
    }
    let path = paths
        .pop()
        .unwrap_or_else(|| context.current_dir.join("tui.json"));
    Ok((confirmed, path))
}
pub fn write_rewind_confirmation(path: &Path, enabled: bool) -> Result<(), SettingWriteError> {
    let path = absolute(path)?;
    let _lock = lock(&path)?;
    let raw = raw_file(&path)?;
    let mut edited = raw.json()?;
    edited["confirm_before_rewind"] = Value::Bool(enabled);
    let _: PublicTuiConfig = serde_json::from_value(edited.clone()).map_err(parse_error)?;
    let raw = raw.replace(edited);
    save(&path, &raw)
}
