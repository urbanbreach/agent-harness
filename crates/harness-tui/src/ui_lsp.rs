use std::collections::BTreeMap;
use std::path::Path;

use crate::text::trimmed_json_string_field;

pub(super) fn server_name_from_args(args_summary: &str) -> Option<String> {
    let path = path_from_args(args_summary)?;
    let extension = format!(
        ".{}",
        Path::new(&path)
            .extension()
            .and_then(|value| value.to_str())?
            .to_ascii_lowercase()
    );
    let config = harness_core::config::registered_lsp_config();
    let mut specs = BTreeMap::from([
        ("go".to_string(), (false, vec![".go".to_string()])),
        (
            "json".to_string(),
            (false, vec![".json".to_string(), ".jsonc".to_string()]),
        ),
        (
            "python".to_string(),
            (false, vec![".py".to_string(), ".pyi".to_string()]),
        ),
        ("rust".to_string(), (false, vec![".rs".to_string()])),
        (
            "typescript".to_string(),
            (
                false,
                vec![
                    ".ts".to_string(),
                    ".tsx".to_string(),
                    ".js".to_string(),
                    ".jsx".to_string(),
                    ".mjs".to_string(),
                    ".cjs".to_string(),
                    ".mts".to_string(),
                    ".cts".to_string(),
                ],
            ),
        ),
        (
            "yaml".to_string(),
            (false, vec![".yaml".to_string(), ".yml".to_string()]),
        ),
    ]);

    for (name, server) in config.servers {
        if let Some((disabled, extensions)) = specs.get_mut(&name) {
            *disabled = server.disabled;
            if let Some(custom_extensions) = server.extensions {
                *extensions = custom_extensions;
            }
        } else if let Some(extensions) = server.extensions {
            specs.insert(name, (server.disabled, extensions));
        }
    }

    specs
        .into_iter()
        .find_map(|(name, (disabled, extensions))| {
            (!disabled && extensions.iter().any(|candidate| candidate == &extension))
                .then_some(name)
        })
}

fn path_from_args(args_summary: &str) -> Option<String> {
    let args = serde_json::from_str::<serde_json::Value>(args_summary).ok()?;
    trimmed_json_string_field(Some(&args), &["path", "filePath"])
}

pub(super) fn path_root_from_args(args_summary: &str) -> Option<String> {
    let path = path_from_args(args_summary)?;
    Path::new(&path)
        .parent()
        .and_then(Path::to_str)
        .map(str::to_string)
}
