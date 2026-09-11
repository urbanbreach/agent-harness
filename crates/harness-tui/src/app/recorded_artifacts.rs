use super::*;

impl AppState {
    /// Read only session-owned, recorded text artifacts while ingesting events.
    /// Paint and resize use this cache and never open workspace paths.
    pub(in crate::app) fn cache_recorded_artifacts(&mut self, event: &EventEnvelopeV1) {
        let Some(root) = self
            .session_path
            .as_ref()
            .and_then(|path| path.canonicalize().ok())
            .filter(|path| path.is_dir())
        else {
            return;
        };
        let mut paths = Vec::new();
        match &event.payload {
            EventV1::ArtifactWritten(artifact)
                if artifact.path.ends_with(".diff") || artifact.path.contains("before") =>
            {
                paths.push(artifact.path.clone())
            }
            EventV1::EditApplied(edit) => paths.extend(edit.diff_rel_path.clone()),
            EventV1::ToolCallFinished(tool) => {
                paths.extend(
                    tool.output_json
                        .as_ref()
                        .into_iter()
                        .flat_map(artifact_paths),
                );
            }
            _ => {}
        }
        let mut bytes = self
            .recorded_artifacts
            .values()
            .map(String::len)
            .sum::<usize>();
        for relative in paths {
            if bytes >= 32 * 1024 * 1024 {
                break;
            }
            if self.recorded_artifacts.contains_key(&relative) {
                continue;
            }
            let path = Path::new(&relative);
            if path.is_absolute()
                || path
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                continue;
            }
            let Some(resolved) = root
                .join(path)
                .canonicalize()
                .ok()
                .filter(|path| path.starts_with(&root))
            else {
                continue;
            };
            if !std::fs::metadata(&resolved).is_ok_and(|meta| {
                meta.is_file()
                    && meta.len() <= 8 * 1024 * 1024
                    && meta.len() <= u64::try_from(32 * 1024 * 1024 - bytes).unwrap_or_default()
            }) {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(resolved) {
                bytes = bytes.saturating_add(content.len());
                self.recorded_artifacts.insert(relative, content);
            }
        }
    }
}

fn artifact_paths(value: &serde_json::Value) -> impl Iterator<Item = String> + '_ {
    std::iter::once(value)
        .chain(
            value
                .get("edits")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten(),
        )
        .flat_map(|object| {
            ["before_rel_path", "diff_rel_path"]
                .into_iter()
                .filter_map(move |key| {
                    object
                        .get(key)
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
        })
}
