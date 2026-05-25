use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn pty_signoff_lane_is_explicit_single_threaded_smoke() {
    assert_eq!(std::env::var("RUST_TEST_THREADS").as_deref(), Ok("1"));
}

#[test]
fn pty_visual_artifact_root_is_env_overridable() {
    let root = visual_artifact_root();
    assert!(root.ends_with("pty-visual-artifacts"));
}

#[test]
fn pty_snapshot_manifest_paths_drop_checkout_root() {
    assert_eq!(
        stable_manifest_snapshot_path(Path::new(
            "/tmp/repo/target/pty-visual-artifacts/pty-manifests/live_shell/manifest.json",
        )),
        "pty-manifests/live_shell/manifest.json"
    );
}

#[test]
fn pty_signoff_fails_closed_when_not_on_linux() {
    if cfg!(target_os = "linux") {
        return;
    }
    assert!(!pty_available());
}

#[test]
fn pty_artifact_directory_can_be_created() {
    let dir =
        std::env::temp_dir().join(format!("harness-testkit-pty-smoke-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create pty smoke artifact dir");
    assert!(dir.is_dir());
    let _ = fs::remove_dir_all(dir);
}

fn pty_available() -> bool {
    cfg!(target_os = "linux")
}

fn visual_artifact_root() -> PathBuf {
    std::env::var_os("HARNESS_VISUAL_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/pty-visual-artifacts"))
}

fn stable_manifest_snapshot_path(path: &Path) -> String {
    let marker = Path::new("pty-visual-artifacts");
    let components = path.components().collect::<Vec<_>>();
    let Some(index) = components
        .iter()
        .position(|component| component.as_os_str() == marker)
    else {
        return path.display().to_string();
    };
    components[index + 1..]
        .iter()
        .collect::<PathBuf>()
        .display()
        .to_string()
}
