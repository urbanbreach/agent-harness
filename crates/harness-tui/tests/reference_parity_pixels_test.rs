//! Fail-closed RGBA pixel parity unit tests (Wave T06).
//!
//! Pure PNG compare via the `image` crate — no browser/Chrome required.
//! Live xterm.js capture lives under `scripts/tui-parity/` and is opt-in.
//!
//! Contract: zero unapproved RGBA differences; identical dimensions; no crop;
//! no SSIM/similarity pass criterion.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration pixel parity tests use fail-fast asserts"
)]

use std::path::PathBuf;

use image::Rgba;

#[path = "support/pixel_compare.rs"]
mod pixel_compare;

use pixel_compare::{compare_png_paths, solid_image, write_temp_png, PixelCompareOutcome};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root must resolve")
}

fn freeze_png(rel: &str) -> PathBuf {
    repo_root()
        .join("artifacts/qa-evidence/20260717-tui-reference-parity/reference/freeze")
        .join(rel)
        .join("terminal.png")
}

fn require_evidence(path: &std::path::Path) -> bool {
    if path.is_file() {
        return true;
    }
    if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() == Ok("1") {
        panic!(
            "HARNESS_TUI_PARITY_STRICT=1 requires evidence file {}",
            path.display()
        );
    }
    false
}

fn which_node() -> Option<PathBuf> {
    std::process::Command::new("sh")
        .args(["-c", "command -v node"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
        .filter(|path| !path.as_os_str().is_empty())
}

#[test]
fn self_test_identical_pngs_pass() {
    // arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let image = solid_image(8, 4, [12, 34, 56, 255]);
    let a = write_temp_png(dir.path(), "a.png", &image);
    let b = write_temp_png(dir.path(), "b.png", &image);

    // act
    let outcome = compare_png_paths(&a, &b);

    // assert
    assert!(
        outcome.is_pass(),
        "identical PNGs must pass zero-tolerance compare: {outcome:?}"
    );
    match outcome {
        PixelCompareOutcome::ExactMatch {
            width,
            height,
            total_pixels,
        } => {
            assert_eq!((width, height), (8, 4));
            assert_eq!(total_pixels, 32);
        }
        other => panic!("expected ExactMatch, got {other:?}"),
    }
}

#[test]
fn fails_on_one_pixel_difference() {
    // arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let reference = solid_image(8, 4, [12, 34, 56, 255]);
    let mut actual = reference.clone();
    actual.put_pixel(3, 1, Rgba([12, 34, 57, 255]));
    let a = write_temp_png(dir.path(), "ref.png", &reference);
    let b = write_temp_png(dir.path(), "act.png", &actual);

    // act
    let outcome = compare_png_paths(&a, &b);

    // assert
    assert!(!outcome.is_pass(), "1-pixel difference must fail closed");
    match outcome {
        PixelCompareOutcome::RgbaMismatch {
            mismatch_count,
            first,
            ..
        } => {
            assert_eq!(mismatch_count, 1);
            assert_eq!(first.len(), 1);
            assert_eq!(first[0].x, 3);
            assert_eq!(first[0].y, 1);
            assert_eq!(first[0].reference, [12, 34, 56, 255]);
            assert_eq!(first[0].actual, [12, 34, 57, 255]);
        }
        other => panic!("expected RgbaMismatch, got {other:?}"),
    }
}

#[test]
fn fails_on_dimension_mismatch_without_cropping() {
    // arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let a = write_temp_png(dir.path(), "a.png", &solid_image(8, 4, [0, 0, 0, 255]));
    let b = write_temp_png(dir.path(), "b.png", &solid_image(7, 4, [0, 0, 0, 255]));

    // act
    let outcome = compare_png_paths(&a, &b);

    // assert
    assert!(!outcome.is_pass());
    assert_eq!(
        outcome,
        PixelCompareOutcome::DimensionMismatch {
            reference: (8, 4),
            actual: (7, 4),
        }
    );
}

#[test]
fn fails_closed_when_png_missing() {
    // arrange
    // act
    // assert
    // arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let present = write_temp_png(
        dir.path(),
        "present.png",
        &solid_image(2, 2, [1, 2, 3, 255]),
    );
    let missing = dir.path().join("missing.png");

    // act / assert
    let result = std::panic::catch_unwind(|| compare_png_paths(&present, &missing));
    assert!(
        result.is_err(),
        "missing PNG must fail closed (panic/abort), not soft-pass"
    );
}

#[test]
fn freeze_run1_startup_matches_run2_startup_when_present() {
    // arrange
    // act
    // assert
    let run1 = freeze_png("run1-startup");
    let run2 = freeze_png("run2-startup");
    if !require_evidence(&run1) || !require_evidence(&run2) {
        return;
    }

    // act
    let outcome = compare_png_paths(&run1, &run2);

    // assert
    assert!(
        outcome.is_pass(),
        "identical freeze startup runs must pass zero-tolerance: {outcome:?}"
    );
}

#[test]
fn freeze_run1_startup_differs_from_run1_draft_when_present() {
    // arrange
    let startup = freeze_png("run1-startup");
    let draft = freeze_png("run1-draft");
    if !require_evidence(&startup) || !require_evidence(&draft) {
        return;
    }

    // act
    let outcome = compare_png_paths(&startup, &draft);

    // assert
    assert!(
        !outcome.is_pass(),
        "startup vs draft freeze must fail closed under zero tolerance"
    );
    match outcome {
        PixelCompareOutcome::RgbaMismatch { mismatch_count, .. } => {
            assert!(mismatch_count > 0);
        }
        PixelCompareOutcome::DimensionMismatch { .. } => {}
        PixelCompareOutcome::ExactMatch { .. } => {
            panic!("startup and draft freezes must not be exact matches");
        }
    }
}

#[test]
fn unit_path_does_not_require_browser_or_chrome() {
    // arrange
    // act
    // assert
    // arrange / act
    let dir = tempfile::tempdir().expect("temp dir");
    let image = solid_image(3, 3, [200, 100, 50, 255]);
    let a = write_temp_png(dir.path(), "u1.png", &image);
    let b = write_temp_png(dir.path(), "u2.png", &image);
    let outcome = compare_png_paths(&a, &b);

    // assert
    assert!(outcome.is_pass());
}

#[test]
fn node_compare_pixels_self_test_passes_when_node_available() {
    // arrange
    let script = repo_root().join("scripts/tui-parity/compare-pixels.mjs");
    if !script.is_file() {
        panic!(
            "first-party compare entrypoint missing: {}",
            script.display()
        );
    }
    let Some(node) = which_node() else {
        return;
    };

    // act
    let output = std::process::Command::new(node)
        .arg(&script)
        .arg("--self-test")
        .current_dir(repo_root())
        .output()
        .expect("node compare-pixels self-test must be runnable");

    // assert
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "node --self-test must exit 0\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(
        stdout.contains("self-test PASS"),
        "self-test must report PASS: {stdout}"
    );
}
