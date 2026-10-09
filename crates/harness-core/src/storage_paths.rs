//! User-level storage resolution shared by credentials and project runtime state.
use std::path::{Path, PathBuf};

pub fn data_dir_from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let get = |key| {
        lookup(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    if let Some(path) = get("HARNESS_HOME") {
        return Some(path);
    }
    #[cfg(windows)]
    let path = get("LOCALAPPDATA")
        .or_else(|| get("APPDATA"))
        .map(|path| path.join("harness"));
    #[cfg(not(windows))]
    let path = get("HOME").map(|path| path.join(".harness"));
    path
}

#[derive(Debug, Clone)]
pub struct ProjectPaths {
    data_dir: PathBuf,
    key: String,
}
impl ProjectPaths {
    pub fn new(data_dir: &Path, project: &Path) -> std::io::Result<Self> {
        let project = project.canonicalize()?;
        let path = project.to_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "project path is not UTF-8",
            )
        })?;
        let path = path.strip_prefix(['/', '\\']).unwrap_or(path);
        let mut key = String::with_capacity(path.len() + 4);
        key.push_str("--");
        key.extend(path.chars().map(|ch| match ch {
            '/' | '\\' | ':' => '-',
            other => other,
        }));
        key.push_str("--");
        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            key,
        })
    }
    pub fn sessions_dir(&self) -> PathBuf {
        self.data_dir.join("sessions").join(&self.key)
    }
    pub fn runtime_dir(&self) -> PathBuf {
        self.data_dir.join("projects").join(&self.key)
    }
    pub fn worktrees_dir(&self) -> PathBuf {
        self.data_dir.join("worktrees").join(&self.key)
    }
}

pub fn resolve_session_dir(
    project: &Path,
    configured: &Path,
    data_dir: Option<&Path>,
) -> std::io::Result<PathBuf> {
    if !configured.as_os_str().is_empty() {
        return Ok(project.join(configured));
    }
    let data_dir = data_dir.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cannot resolve harness home directory; set HARNESS_HOME or provide --session-dir",
        )
    })?;
    Ok(ProjectPaths::new(data_dir, project)?.sessions_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_home_is_used_as_is_and_empty_override_uses_platform_default() {
        #[cfg(not(windows))]
        let default = "/users/test/.harness";
        #[cfg(windows)]
        let default = "/users/test/local/harness";
        for (harness_home, expected) in [
            (Some("relative/home"), "relative/home"),
            (Some("~/literal"), "~/literal"),
            (Some(" "), " "),
            (Some(""), default),
            (None, default),
        ] {
            let resolved = data_dir_from_lookup(&|name| match name {
                "HARNESS_HOME" => harness_home.map(str::to_owned),
                "HOME" => Some("/users/test".into()),
                "LOCALAPPDATA" => Some("/users/test/local".into()),
                _ => None,
            });
            assert_eq!(resolved, Some(PathBuf::from(expected)));
        }
        assert_eq!(data_dir_from_lookup(&|_| None), None);
    }
}
