use std::path::Path;

pub const VISUAL_MANIFEST_JSON_FILE: &str = "manifest.json";
pub const VISUAL_MANIFEST_JSONL_FILE: &str = "manifest.jsonl";

#[allow(dead_code)]
pub(crate) fn stable_manifest_snapshot_path(path: &Path) -> String {
    let components = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    components
        .iter()
        .position(|component| component == "pty-manifests")
        .map(|index| components[index..].join("/"))
        .or_else(|| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}

#[allow(dead_code)]
pub(crate) fn marker_presence_lines(screen: &str, markers: &[&str]) -> Vec<String> {
    marker_presence_states(screen, markers)
        .into_iter()
        .map(|(marker, present)| format!("{marker}: {present}"))
        .collect()
}

pub(crate) fn marker_presence_states(screen: &str, markers: &[&str]) -> Vec<(String, bool)> {
    markers
        .iter()
        .map(|marker| ((*marker).to_string(), screen.contains(marker)))
        .collect()
}
