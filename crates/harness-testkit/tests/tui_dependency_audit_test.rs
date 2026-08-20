use std::collections::BTreeSet;
use std::error::Error;

use harness_testkit::tui_dependency_audit::audit_workspace;

#[path = "support/harness_bin.rs"]
mod harness_bin;

const TASK_SIX_GAP_MAPPING: &[(&str, &str)] = &[
    (
        "viewport-scale",
        "large/max layout, resize, and settling behavior",
    ),
    ("interaction", "mouse capture, decoding, and hit routing"),
    (
        "surface",
        "media/dashboard/modal composition, clipping, and layering",
    ),
    (
        "lifecycle",
        "question/cancel/recover/complete state transitions",
    ),
    (
        "visual-behavior",
        "geometry/glyph/style/cursor parity comparison",
    ),
];

#[test]
fn ratatui_and_crossterm_lock_versions_preserve_baseline() -> Result<(), Box<dyn Error>> {
    // arrange
    let report = audit_workspace()?;
    let ratatui = report
        .justifications
        .iter()
        .find(|record| record.crate_name == "ratatui")
        .ok_or("ratatui justification is missing")?;
    // act
    let crossterm = report
        .justifications
        .iter()
        .find(|record| record.crate_name == "crossterm")
        .ok_or("crossterm justification is missing")?;

    // assert
    assert!(version_at_least(&ratatui.version, [0, 30, 1]));
    assert!(version_at_least(&crossterm.version, [0, 29, 0]));
    Ok(())
}

#[test]
fn every_harness_tui_direct_dependency_has_a_typed_justification() -> Result<(), Box<dyn Error>> {
    // arrange
    let report = audit_workspace()?;
    // act
    let justified: BTreeSet<&str> = report
        .justifications
        .iter()
        .map(|record| record.crate_name.as_str())
        .collect();

    for dependency in &report.direct_dependencies {
        // assert
        assert!(
            justified.contains(dependency.crate_name.as_str()),
            "missing justification for {}",
            dependency.crate_name
        );
    }
    assert_eq!(
        report.direct_dependencies.len(),
        report.justifications.len()
    );
    Ok(())
}

#[test]
fn task_six_gap_mapping_stub_is_recorded() -> Result<(), Box<dyn Error>> {
    // arrange
    let report = audit_workspace()?;

    if std::env::var_os("HARNESS_TUI_DEPENDENCY_AUDIT_REPORT").is_some() {
        println!(
            "AUDIT_REPORT_JSON={}",
            serde_json::to_string_pretty(&report)?
        );
    }

    for (gap, capability) in TASK_SIX_GAP_MAPPING {
        // act
        let mapping = report
            .baseline_gap_mappings
            .iter()
            .find(|mapping| mapping.gap == *gap)
            .ok_or_else(|| format!("missing task-6 mapping for {gap}"))?;
        // assert
        assert_eq!(mapping.capability, *capability);
    }
    for record in &report.justifications {
        assert!(
            !record.capability_gap.is_empty(),
            "{} has no gap",
            record.crate_name
        );
        assert!(
            !record.owner_test.is_empty(),
            "{} has no owner test",
            record.crate_name
        );
    }
    Ok(())
}

#[test]
fn locked_workspace_build_succeeds() -> Result<(), Box<dyn Error>> {
    // arrange
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    // act
    let status = harness_bin::command("cargo")
        .args(["build", "--workspace", "--locked"])
        .current_dir(root)
        .status()?;
    // assert
    assert!(status.success(), "cargo build --workspace --locked failed");
    Ok(())
}

fn version_at_least(actual: &str, required: [u64; 3]) -> bool {
    let Some(actual) = version_tuple(actual) else {
        return false;
    };
    actual >= required
}

fn version_tuple(version: &str) -> Option<[u64; 3]> {
    let mut parsed = [0; 3];
    for (index, component) in version.split('.').take(3).enumerate() {
        parsed[index] = component.split('-').next()?.parse().ok()?;
    }
    Some(parsed)
}
