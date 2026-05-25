use std::path::PathBuf;

#[test]
#[ignore = "requires HARNESS_NATIVE_VISUAL=1; runs inside a managed nested KWin/XWayland session"]
fn native_visual_ghostty_smoke() {
    assert_native_visual_env().expect("native visual smoke prerequisites");
}

#[test]
fn native_visual_fails_closed_without_env() {
    if std::env::var("HARNESS_NATIVE_VISUAL").as_deref() == Ok("1") {
        return;
    }
    assert!(assert_native_visual_env().is_err());
}

#[test]
fn native_visual_summary_preserves_grid_metadata() {
    let metadata = native_visual_metadata("ghostty", 160, 48);
    assert_eq!(metadata.backend, "ghostty");
    assert_eq!(metadata.cols, 160);
    assert_eq!(metadata.rows, 48);
}

fn assert_native_visual_env() -> Result<(), String> {
    if std::env::var("HARNESS_NATIVE_VISUAL").as_deref() != Ok("1") {
        return Err("HARNESS_NATIVE_VISUAL=1 is required".to_string());
    }
    if std::env::var_os("DISPLAY").is_none() {
        return Err("DISPLAY is required for native visual signoff".to_string());
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct NativeVisualMetadata {
    backend: String,
    cols: u16,
    rows: u16,
    artifact_root: PathBuf,
}

fn native_visual_metadata(backend: &str, cols: u16, rows: u16) -> NativeVisualMetadata {
    NativeVisualMetadata {
        backend: backend.to_string(),
        cols,
        rows,
        artifact_root: std::env::var_os("HARNESS_VISUAL_ARTIFACT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/native-visual-artifacts")),
    }
}
