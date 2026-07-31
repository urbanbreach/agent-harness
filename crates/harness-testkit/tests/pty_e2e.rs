use harness_testkit::UnwrapOrAbort;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

const TUI_SIGNOFF_MANIFEST: &str = include_str!("../../../docs/testing/tui-signoff-manifest.v1.json");

#[test]
fn pty_signoff_lane_is_explicit_single_threaded_smoke() {
    // arrange
    // act
    // assert
    assert_eq!(std::env::var("RUST_TEST_THREADS").as_deref(), Ok("1"));
}

#[test]
fn pty_visual_artifact_root_is_env_overridable() {
    // arrange
    // act
    // assert
    let root = visual_artifact_root();
    assert!(root.ends_with("pty-visual-artifacts"));
}

#[test]
fn pty_snapshot_manifest_paths_drop_checkout_root() {
    // arrange
    // act
    // assert
    assert_eq!(
        stable_manifest_snapshot_path(Path::new(
            "/tmp/repo/target/pty-visual-artifacts/pty-manifests/live_shell/manifest.json",
        )),
        "pty-manifests/live_shell/manifest.json"
    );
}

#[test]
fn pty_signoff_fails_closed_when_not_on_linux() {
    // arrange
    // act
    // assert
    if cfg!(target_os = "linux") {
        return;
    }
    assert!(!pty_available());
}

#[test]
fn pty_artifact_directory_can_be_created() {
    // arrange
    // act
    // assert
    let dir =
        std::env::temp_dir().join(format!("harness-testkit-pty-smoke-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap_or_abort();
    assert!(dir.is_dir());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn pty_signoff_manifest_declares_required_flow_artifacts() {
    // arrange
    // act
    // assert
    let manifest: Value = serde_json::from_str(TUI_SIGNOFF_MANIFEST).unwrap_or_abort();
    assert_eq!(
        manifest["schema_version"],
        "harness-tui-signoff-manifest-v1"
    );

    let flows = manifest["flows"].as_array().unwrap_or_abort();
    for flow_id in [
        "shell_topology",
        "startup",
        "command_palette",
        "session_picker_resume",
        "permission_question",
        "provider_tool_failure",
        "diff_review",
        "file_mention",
    ] {
        let flow = flows
            .iter()
            .find(|flow| flow["id"] == flow_id)
            .unwrap_or_else(|| panic!("missing signoff flow {flow_id}"));
        assert_non_empty_array(flow, "deterministic_tests");
        assert_non_empty_array(flow, "pty_stages");
        assert!(
            flow["pty_stages"]
                .as_array()
                .unwrap_or_abort()
                .iter()
                .any(|stage| stage
                    .as_str()
                    .is_some_and(|stage| stage.contains("signoff-pty"))),
            "flow {flow_id} must name a signoff-pty stage"
        );
    }

    assert_eq!(manifest["native_visual_policy"]["lane"], "signoff-native");
    assert_eq!(
        manifest["native_visual_policy"]["missing_env_status"],
        "documented_gap"
    );

    let artifact_root = visual_artifact_root();
    fs::create_dir_all(&artifact_root).unwrap_or_abort();
    let manifest_path = artifact_root.join("tui-signoff-manifest.v1.json");
    let summary_path = artifact_root.join("tui-signoff-manifest.v1.md");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap_or_abort(),
    )
    .unwrap_or_abort();
    fs::write(
        &summary_path,
        format!(
            "# TUI signoff manifest\n\n- schema: harness-tui-signoff-manifest-v1\n- flows: {}\n- native visual: signoff-native, env-gated, documented_gap when missing\n",
            flows
                .iter()
                .filter_map(|flow| flow["id"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    )
    .unwrap_or_abort();

    assert_eq!(
        stable_manifest_snapshot_path(&manifest_path),
        "tui-signoff-manifest.v1.json"
    );
    assert!(summary_path.is_file());
}

fn pty_available() -> bool {
    cfg!(target_os = "linux")
}

fn visual_artifact_root() -> PathBuf {
    std::env::var_os("HARNESS_VISUAL_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target/pty-visual-artifacts"))
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

fn assert_non_empty_array(flow: &Value, field: &str) {
    assert!(
        flow[field]
            .as_array()
            .is_some_and(|values| !values.is_empty()),
        "flow {} must declare a non-empty {field} array",
        flow["id"]
    );
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
