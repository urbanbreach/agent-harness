use std::path::Path;

pub(crate) fn display_path(path: &Path) -> String {
    path.display().to_string()
}
