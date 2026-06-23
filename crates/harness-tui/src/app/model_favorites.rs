use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MODEL_FAVORITES_SCHEMA_VERSION: u8 = 1;
const MODEL_FAVORITES_FILE_NAME: &str = "model-favorites.json";
const MODEL_RECENTS_FILE_NAME: &str = "model-recents.json";
const MAX_RECENTS: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedModelFavorites {
    schema_version: u8,
    favorites: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedModelRecents {
    schema_version: u8,
    recents: Vec<String>,
}

pub fn model_favorites_path_for_session_dir(session_dir: &Path) -> PathBuf {
    session_dir.join("tui").join(MODEL_FAVORITES_FILE_NAME)
}

pub fn model_recents_path_for_session_dir(session_dir: &Path) -> PathBuf {
    session_dir.join("tui").join(MODEL_RECENTS_FILE_NAME)
}

pub(super) fn load_model_favorites(path: &Path) -> Result<BTreeSet<String>, String> {
    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(err) => {
            return Err(format!(
                "failed to read model favorites {}: {err}",
                path.display()
            ))
        }
    };

    let favorites = serde_json::from_str::<PersistedModelFavorites>(&body)
        .map_err(|err| format!("failed to parse model favorites {}: {err}", path.display()))?;
    if favorites.schema_version != MODEL_FAVORITES_SCHEMA_VERSION {
        return Err(format!(
            "unsupported model favorites schema {} in {}",
            favorites.schema_version,
            path.display()
        ));
    }

    Ok(favorites.favorites.into_iter().collect())
}

pub(super) fn save_model_favorites(
    path: &Path,
    favorites: &BTreeSet<String>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create model favorites dir {}: {err}",
                parent.display()
            )
        })?;
    }

    let favorites = PersistedModelFavorites {
        schema_version: MODEL_FAVORITES_SCHEMA_VERSION,
        favorites: favorites.iter().cloned().collect(),
    };
    let body = serde_json::to_vec_pretty(&favorites)
        .map_err(|err| format!("failed to serialize model favorites: {err}"))?;
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, body).map_err(|err| {
        format!(
            "failed to write model favorites {}: {err}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, path).map_err(|err| {
        format!(
            "failed to replace model favorites {}: {err}",
            path.display()
        )
    })
}

pub(super) fn load_model_recents(path: &Path) -> Result<Vec<String>, String> {
    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(format!(
                "failed to read model recents {}: {err}",
                path.display()
            ))
        }
    };

    let recents = serde_json::from_str::<PersistedModelRecents>(&body)
        .map_err(|err| format!("failed to parse model recents {}: {err}", path.display()))?;
    if recents.schema_version != MODEL_FAVORITES_SCHEMA_VERSION {
        return Err(format!(
            "unsupported model recents schema {} in {}",
            recents.schema_version,
            path.display()
        ));
    }

    Ok(recents.recents)
}

pub(super) fn save_model_recents(path: &Path, recents: &[String]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create model recents dir {}: {err}",
                parent.display()
            )
        })?;
    }

    let recents = PersistedModelRecents {
        schema_version: MODEL_FAVORITES_SCHEMA_VERSION,
        recents: recents.to_vec(),
    };
    let body = serde_json::to_vec_pretty(&recents)
        .map_err(|err| format!("failed to serialize model recents: {err}"))?;
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, body).map_err(|err| {
        format!(
            "failed to write model recents {}: {err}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, path)
        .map_err(|err| format!("failed to replace model recents {}: {err}", path.display()))
}

pub(super) fn push_model_recent(recents: &mut Vec<String>, model_key: &str) {
    recents.retain(|r| r != model_key);
    recents.insert(0, model_key.to_string());
    if recents.len() > MAX_RECENTS {
        recents.truncate(MAX_RECENTS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn model_favorites_round_trip() {
        // arrange
        let dir = tempdir().unwrap();
        let path = model_favorites_path_for_session_dir(dir.path());
        let mut favorites = BTreeSet::new();
        favorites.insert("openai/gpt-5.5".to_string());
        favorites.insert("anthropic/claude-4".to_string());

        // act
        save_model_favorites(&path, &favorites).unwrap();
        let loaded = load_model_favorites(&path).unwrap();
        // assert
        assert_eq!(loaded, favorites);
    }

    #[test]
    fn model_recents_round_trip() {
        // arrange
        let dir = tempdir().unwrap();
        let path = model_recents_path_for_session_dir(dir.path());
        let recents = vec![
            "openai/gpt-5.5".to_string(),
            "anthropic/claude-4".to_string(),
        ];

        // act
        save_model_recents(&path, &recents).unwrap();
        let loaded = load_model_recents(&path).unwrap();
        // assert
        assert_eq!(loaded, recents);
    }

    #[test]
    fn push_model_recent_dedupes_and_caps() {
        // arrange
        let mut recents = vec!["a".to_string(), "b".to_string()];
        // act
        push_model_recent(&mut recents, "b");
        // assert
        assert_eq!(recents, vec!["b".to_string(), "a".to_string()]);

        for i in 0..15 {
            push_model_recent(&mut recents, &format!("model-{i}"));
        }
        assert!(recents.len() <= MAX_RECENTS);
    }
}
