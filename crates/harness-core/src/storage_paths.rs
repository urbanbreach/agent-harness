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
        let key = format!("v2-{}", blake3::hash(path.as_bytes()).to_hex());
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
    fn project_storage_is_distinct_bounded_and_stable_through_symlinks(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let data = temp.path().join("data");
        let mut previous = Vec::new();
        for relative in [
            "a-b/c".to_owned(),
            "a/b-c".to_owned(),
            "a:b/c".to_owned(),
            "a\\b/c".to_owned(),
            format!("{0}/{0}/{0}", "long".repeat(25)),
        ] {
            let project = temp.path().join(relative);
            std::fs::create_dir_all(&project)?;
            let paths = ProjectPaths::new(&data, &project)?;
            let directories = [
                paths.sessions_dir(),
                paths.runtime_dir(),
                paths.worktrees_dir(),
            ];
            assert!(
                !directories.iter().any(|dir| previous.contains(dir)),
                "project storage collided"
            );
            assert!(directories.iter().all(|dir| !dir.exists()));
            for directory in &directories {
                std::fs::create_dir_all(directory)?;
                std::fs::write(
                    directory.join("owner"),
                    project.as_os_str().as_encoded_bytes(),
                )?;
            }
            #[cfg(unix)]
            {
                let alias = temp.path().join("alias");
                std::os::unix::fs::symlink(&project, &alias)?;
                let aliases = ProjectPaths::new(&data, &alias)?;
                assert_eq!(
                    directories,
                    [
                        aliases.sessions_dir(),
                        aliases.runtime_dir(),
                        aliases.worktrees_dir()
                    ]
                );
                std::fs::remove_file(alias)?;
            }
            previous.extend(directories);
        }
        Ok(())
    }

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
