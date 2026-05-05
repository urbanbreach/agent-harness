use std::fs;

pub fn setup_workspace() -> tempfile::TempDir {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(temp_dir.path().join("workspace")).expect("workspace");
    temp_dir
}
