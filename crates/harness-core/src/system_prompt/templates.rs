use super::PromptSource;
use minijinja::{Environment, Error, ErrorKind};
use std::{
    fs,
    io::{ErrorKind as IoErrorKind, Read},
    path::{Component, Path},
};

mod bundled;

const MAX_TEMPLATE_BYTES: u64 = 256 * 1024;

pub(super) fn configure(
    environment: &mut Environment<'static>,
    source: &PromptSource,
    workspace: &Path,
) {
    let root = source.project_prompt_root.as_deref().unwrap_or(workspace);
    let mut directories: Vec<_> = crate::config::search_roots(root)
        .into_iter()
        .rev()
        .map(|path| path.join(".agent-harness/prompts"))
        .collect();
    directories.extend(source.user_prompt_dir.iter().cloned());
    environment.set_loader(move |name| {
        let path = Path::new(name);
        if path.extension().is_none_or(|extension| extension != "md")
            || !path
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            || name.contains('\\')
        {
            return Err(Error::new(
                ErrorKind::InvalidOperation,
                "prompt includes must be relative Markdown paths without traversal",
            ));
        }
        for directory in &directories {
            let candidate = directory.join(path);
            let metadata = match fs::symlink_metadata(&candidate) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == IoErrorKind::NotFound => continue,
                Err(error) => return Err(load_error(&candidate, error)),
            };
            let canonical =
                fs::canonicalize(&candidate).map_err(|error| load_error(&candidate, error))?;
            let root = fs::canonicalize(directory).map_err(|error| load_error(directory, error))?;
            if !canonical.starts_with(root) || (!metadata.is_file() && !metadata.is_symlink()) {
                return Err(load_error(
                    &candidate,
                    "prompt must be a file inside its prompt directory",
                ));
            }
            if !fs::metadata(&canonical)
                .map_err(|error| load_error(&candidate, error))?
                .is_file()
            {
                return Err(load_error(&candidate, "prompt must be a regular file"));
            }
            let file = fs::File::open(&canonical).map_err(|error| load_error(&candidate, error))?;
            let mut text = String::new();
            file.take(MAX_TEMPLATE_BYTES + 1)
                .read_to_string(&mut text)
                .map_err(|error| load_error(&candidate, error))?;
            if text.len() as u64 > MAX_TEMPLATE_BYTES || text.trim().is_empty() {
                return Err(load_error(
                    &candidate,
                    "prompt must be nonempty and at most 256 KiB",
                ));
            }
            return Ok(Some(text));
        }
        Ok(bundled::FILES
            .iter()
            .find(|(path, _)| *path == name)
            .map(|(_, text)| (*text).to_owned()))
    });
}

fn load_error(path: &Path, error: impl std::fmt::Display) -> Error {
    Error::new(
        ErrorKind::InvalidOperation,
        format!("cannot load prompt {}: {error}", path.display()),
    )
}
