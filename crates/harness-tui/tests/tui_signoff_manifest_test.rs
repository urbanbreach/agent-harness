use harness_tui::UnwrapOrAbort;
use serde_json::Value;

const TUI_SIGNOFF_MANIFEST: &str =
    include_str!("../../../docs/testing/tui-signoff-manifest.v1.json");

#[test]
#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn tui_signoff_manifest_covers_required_release_flows() {
    // arrange
    let manifest: Value = serde_json::from_str(TUI_SIGNOFF_MANIFEST).unwrap_or_abort();
    assert_eq!(
        manifest["schema_version"],
        "harness-tui-signoff-manifest-v1"
    );
    assert_eq!(manifest["native_visual_policy"]["lane"], "signoff-native");
    assert_eq!(
        manifest["native_visual_policy"]["missing_env_status"],
        "documented_gap"
    );

    let flows = manifest["flows"].as_array().unwrap_or_abort();
    let required_flow_ids = [
        "shell_topology",
        "startup",
        "command_palette",
        "session_picker_resume",
        "permission_question",
        "provider_tool_failure",
        "diff_review",
        "file_mention",
    ];

    // act
    let flow_ids = flows
        .iter()
        .filter_map(|flow| flow["id"].as_str())
        .collect::<Vec<_>>();

    // assert
    for flow_id in required_flow_ids {
        assert!(
            flow_ids.contains(&flow_id),
            "missing signoff flow {flow_id}"
        );
        let flow = flows
            .iter()
            .find(|flow| flow["id"] == flow_id)
            .unwrap_or_else(|| {
                let _ = flow_id;
                panic!("abort");
            });
        assert_non_empty_array(flow, "deterministic_tests");
        assert_non_empty_array(flow, "pty_stages");
        assert_non_empty_array(flow, "required_markers");
    }

    assert_flow_names_test(flows, "shell_topology", "shell_topology_contract_test");
    assert_flow_names_test(
        flows,
        "startup",
        "startup_shell_is_compose_first_without_pty",
    );
    assert_flow_names_test(
        flows,
        "command_palette",
        "command_palette_renders_without_pty",
    );
    assert_flow_names_test(
        flows,
        "session_picker_resume",
        "startup_session_history_picker_renders_without_pty",
    );
    assert_flow_names_test(
        flows,
        "permission_question",
        "question_permission_prompt_renders_without_pty",
    );
    assert_flow_names_test(
        flows,
        "provider_tool_failure",
        "replay_failure_state_renders_without_pty",
    );
    assert_flow_names_test(
        flows,
        "diff_review",
        "diff_hunk_navigation_advances_and_retreats_between_hunks",
    );
    assert_flow_names_test(
        flows,
        "file_mention",
        "file_mention_at_opens_picker_and_inserts_real_workspace_file",
    );

    assert_flow_pty_owner(
        flows,
        "permission_question",
        "pty_permission_overlay_resolves_and_preserves_draft",
    );
    assert_flow_pty_owner(
        flows,
        "command_palette",
        "pty_status_dialog_opens_without_sidebar_copy",
    );
    assert_flow_pty_owner(
        flows,
        "shell_topology",
        "pty_status_dialog_opens_without_sidebar_copy",
    );
    assert_flow_pty_owner(
        flows,
        "startup",
        "pty_smoke_starts_accepts_input_resizes_and_exits",
    );

    for flow in flows {
        let pty_stages = flow["pty_stages"].as_array().unwrap_or_abort();
        for stage in pty_stages {
            let stage = stage.as_str().unwrap_or_abort();
            assert!(
                !stage.contains("happy_path_pty")
                    && !stage.contains("pty_onboarding_auth_drives_required_screens"),
                "flow {} must not claim phantom PTY stage: {stage}",
                flow["id"]
            );
            assert!(
                stage.contains("pty_smoke_starts_accepts_input_resizes_and_exits")
                    || stage.contains("pty_connect_auth_drives_provider_connection")
                    || stage.contains("pty_permission_overlay_resolves_and_preserves_draft")
                    || stage.contains("pty_status_dialog_opens_without_sidebar_copy")
                    || stage.contains("pty_draft_esc_esc_clears_composer")
                    || stage.contains("harness_tui_pty_e2e"),
                "flow {} must name a real PTY owner: {stage}",
                flow["id"]
            );
        }
    }

    assert_eq!(manifest["reference_image_policy"], "not_required");
    assert!(
        flows
            .iter()
            .all(|flow| flow.get("reference_assets").is_none()),
        "reference-image comparison must not be required"
    );
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

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn assert_flow_names_test(flows: &[Value], flow_id: &str, test_name: &str) {
    let flow = flows
        .iter()
        .find(|flow| flow["id"] == flow_id)
        .unwrap_or_else(|| {
            let _ = flow_id;
            panic!("abort");
        });
    let deterministic_tests = flow["deterministic_tests"].as_array().unwrap_or_abort();
    assert!(
        deterministic_tests
            .iter()
            .any(|test| test.as_str().is_some_and(|test| test.contains(test_name))),
        "flow {flow_id} must name deterministic owner {test_name}"
    );
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn assert_flow_pty_owner(flows: &[Value], flow_id: &str, pty_test: &str) {
    let flow = flows
        .iter()
        .find(|flow| flow["id"] == flow_id)
        .unwrap_or_else(|| {
            let _ = flow_id;
            panic!("abort");
        });
    let pty_stages = flow["pty_stages"].as_array().unwrap_or_abort();
    assert!(
        pty_stages
            .iter()
            .any(|stage| stage.as_str().is_some_and(|stage| stage.contains(pty_test))),
        "flow {flow_id} must name PTY owner {pty_test}"
    );
}

// ===========================================================================
// Reference-parity manifest regression (docs/tui-reference-parity-manifest.v1.json)
//
// Task 1 of grok-build-visible-first-parity. Rejects self-oracle paths, stale
// SHA, oversized identity spans, historical visual pass values, and restored
// rows not marked incomplete. Validates that all eight Core fixture receipts
// name the pinned reference SHA.
// ===========================================================================

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use harness_testkit::parity::SemanticFrame;

const REFERENCE_PARITY_MANIFEST: &str =
    include_str!("../../../docs/reference/tui-reference-parity-manifest.v1.json");

const PINNED_REFERENCE_SHA256: &str =
    "883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5";
const PINNED_REFERENCE_VERSION: &str = "grok 0.1.220-alpha.4 (c1b5909) [stable]";
const PINNED_SOURCE_REVISION: &str = "c1b5909ec707c069f1d21a93917af044e71da0d7";

const VISUAL_SURFACES: &[&str] = &[
    "startup",
    "shell",
    "composer",
    "chrome",
    "transcript",
    "overlay",
    "responsive",
];
const RESTORED_ROWS: &[&str] = &["TX-TOOL", "TX-DIFF", "OVL-PALETTE", "OVL-SESSION"];
const EXTERNAL_EXCLUSION_IDS: &[&str] = &[
    "sandbox.seatbelt_windows",
    "mcp.oauth_remote_transports",
    "plugins.marketplace_install",
    "remote.workspace_hub",
    "auth.browser_oidc_sso",
    "cli.share",
    "provider.non_openai_live_proof",
];
const APPROVED_IDENTITY_FIELDS: &[&str] = &[
    "logo_glyphs",
    "product_title_text",
    "version_text",
    "breadcrumb",
    "breadcrumb_path_or_project_label",
    "model_badge",
    "model_badge_text",
    "session_path_labels",
    "run_id",
    "session_id",
    "configured_provider",
    "provider_label",
    "filesystem_paths",
    "timestamp",
];

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Resolve a manifest path string (e.g. `crates/harness-tui/tests/...` or a
/// `tests/...`/`core/...` relative path) to an absolute filesystem path under
/// the harness-tui crate directory.
fn resolve_under_crate(crate_dir: &Path, p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("crates/harness-tui/") {
        crate_dir.join(rest)
    } else if p.starts_with("tests/") || p.starts_with("core/") {
        crate_dir.join(p)
    } else {
        crate_dir.join("../..").join(p)
    }
}

/// Recursively scan a JSON value for stale lane paths or Harness self-oracle
/// paths, recording `path: value` strings for each hit.
fn scan_stale_strings(value: &Value, path: &str, hits: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            if s.contains("target/test-lanes/latest") || s.contains("/actual/harness-") {
                hits.push(format!("{path}: {s}"));
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                scan_stale_strings(v, &p, hits);
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                scan_stale_strings(v, &format!("{path}[{i}]"), hits);
            }
        }
        _ => {}
    }
}

/// Validate a reference-parity manifest value. Returns a list of human-readable
/// errors; each error that is row-specific names the requirement id so the
/// failing row is identifiable. An empty vec means the manifest is honest.
#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn validate_reference_parity_manifest(manifest: &Value, crate_dir: &Path) -> Vec<String> {
    let mut errs = Vec::new();

    // --- Pinned reference binary provenance ---
    let bin_sha = manifest["reference"]["binary_sha256"]
        .as_str()
        .unwrap_or("");
    if bin_sha != PINNED_REFERENCE_SHA256 {
        errs.push(format!(
            "reference.binary_sha256 {bin_sha:?} does not match pinned reference SHA"
        ));
    }
    if manifest["reference"]["binary_version"]
        .as_str()
        .unwrap_or("")
        != PINNED_REFERENCE_VERSION
    {
        errs.push("reference.binary_version does not match pinned version".to_string());
    }
    if manifest["reference"]["source_revision"]
        .as_str()
        .unwrap_or("")
        != PINNED_SOURCE_REVISION
    {
        errs.push("reference.source_revision does not match pinned source revision".to_string());
    }

    // --- Eight Core fixture receipts must name the pinned reference SHA ---
    let core = match manifest["reference"]["core_frames"].as_array() {
        Some(a) => a,
        None => {
            errs.push("reference.core_frames missing or not an array".to_string());
            return errs;
        }
    };
    if core.len() != 8 {
        errs.push(format!("reference.core_frames count {} != 8", core.len()));
    }
    for cf in core {
        let frame = cf["frame"].as_str().unwrap_or("<missing>");
        let receipt_rel = cf["fixture_receipt"].as_str().unwrap_or("");
        let receipt_path = resolve_under_crate(crate_dir, receipt_rel);
        let receipt_txt = match fs::read_to_string(&receipt_path) {
            Ok(s) => s,
            Err(e) => {
                errs.push(format!(
                    "core frame {frame}: receipt not readable at {}: {e}",
                    receipt_path.display()
                ));
                continue;
            }
        };
        let receipt: Value = match serde_json::from_str(&receipt_txt) {
            Ok(v) => v,
            Err(e) => {
                errs.push(format!("core frame {frame}: receipt not valid JSON: {e}"));
                continue;
            }
        };
        let rsha = receipt["binary_sha256"].as_str().unwrap_or("");
        if rsha != PINNED_REFERENCE_SHA256 {
            errs.push(format!(
                "core frame {frame}: receipt binary_sha256 {rsha:?} does not name pinned reference SHA"
            ));
        }
        // Captured frames must ship a schema-valid SemanticFrame cells.json.
        let status = cf["capture_status"].as_str().unwrap_or("");
        if status == "captured" {
            let cells_rel = cf["fixture_cells"].as_str().unwrap_or("");
            let cells_path = resolve_under_crate(crate_dir, cells_rel);
            match SemanticFrame::read_cells_json(&cells_path) {
                Ok(frame_obj) => {
                    if u64::from(frame_obj.cols) != cf["viewport"]["cols"].as_u64().unwrap_or(0)
                        || u64::from(frame_obj.rows) != cf["viewport"]["rows"].as_u64().unwrap_or(0)
                    {
                        errs.push(format!("core frame {frame}: cells.json viewport mismatch"));
                    }
                }
                Err(e) => errs.push(format!(
                    "core frame {frame}: cells.json not a valid SemanticFrame at {}: {e}",
                    cells_path.display()
                )),
            }
        }
    }

    // --- Visual rows: no self-oracle, no stale lane, no pass, no excluded ---
    let visual: HashSet<&str> = VISUAL_SURFACES.iter().copied().collect();
    let approved: HashSet<&str> = APPROVED_IDENTITY_FIELDS.iter().copied().collect();
    let restored: HashSet<&str> = RESTORED_ROWS.iter().copied().collect();
    let rows = match manifest["rows"].as_array() {
        Some(r) => r,
        None => {
            errs.push("rows missing or not an array".to_string());
            return errs;
        }
    };
    for row in rows {
        let rid = row["requirement_id"].as_str().unwrap_or("<missing>");
        let surface = row["surface"].as_str().unwrap_or("");
        if !visual.contains(surface) {
            continue;
        }
        let status = row["status"].as_str().unwrap_or("");
        if status == "pass" {
            errs.push(format!(
                "row {rid}: visual surface {surface} must not carry historical pass status"
            ));
        }
        if status == "excluded" {
            errs.push(format!(
                "row {rid}: visual surface {surface} must not be excluded (only external exclusions are excluded)"
            ));
        }
        if restored.contains(rid) && status != "incomplete" {
            errs.push(format!(
                "row {rid}: restored local visual row must be incomplete, observed {status:?}"
            ));
        }
        if let Some(artifact) = row["expected_semantic_cell_artifact"].as_str() {
            if artifact.contains("/actual/harness-") {
                errs.push(format!(
                    "row {rid}: expected_semantic_cell_artifact is a self-oracle path ({artifact})"
                ));
            }
            if artifact.contains("target/test-lanes/latest") {
                errs.push(format!(
                    "row {rid}: expected_semantic_cell_artifact reuses stale target/test-lanes/latest ({artifact})"
                ));
            }
        }
        // Oversized identity span: identity fields must be approved identity only.
        if let Some(fields) = row["identity_substitution"]["fields"].as_array() {
            for f in fields {
                if let Some(name) = f.as_str() {
                    if !approved.contains(name) {
                        errs.push(format!(
                            "row {rid}: oversized identity span; field {name:?} is not an approved identity token"
                        ));
                    }
                }
            }
        }
        // Stale_state: reject any reuse of target/test-lanes/latest or a Harness
        // self-oracle path anywhere in this visual row (recursive scan).
        let mut stale_hits: Vec<String> = Vec::new();
        scan_stale_strings(row, "", &mut stale_hits);
        for hit in stale_hits {
            errs.push(format!("row {rid}: stale/self-oracle path at {hit}"));
        }
    }

    // --- Exactly the seven external exclusions, all excluded ---
    let ext = manifest["external_exclusions"].as_array();
    let ext = match ext {
        Some(a) => a,
        None => {
            errs.push("external_exclusions missing or not an array".to_string());
            return errs;
        }
    };
    let expected: HashSet<&str> = EXTERNAL_EXCLUSION_IDS.iter().copied().collect();
    let mut seen: HashSet<&str> = HashSet::new();
    for ex in ext {
        let cid = ex["capability_id"].as_str().unwrap_or("<missing>");
        if !expected.contains(cid) {
            errs.push(format!(
                "external_exclusion {cid}: not one of the seven retained exclusions"
            ));
        }
        if !seen.insert(cid) {
            errs.push(format!("external_exclusion {cid}: duplicated"));
        }
        let st = ex["status"].as_str().unwrap_or("");
        if st != "excluded" {
            errs.push(format!(
                "external_exclusion {cid}: must remain excluded, observed {st:?}"
            ));
        }
    }
    for missing in expected.difference(&seen) {
        errs.push(format!(
            "external_exclusion {missing}: missing from external_exclusions"
        ));
    }

    errs
}

fn parse_reference_parity_manifest() -> Value {
    serde_json::from_str(REFERENCE_PARITY_MANIFEST).unwrap_or_abort()
}

#[test]
#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn tui_reference_parity_manifest_is_honest() {
    // arrange
    // act
    let manifest = parse_reference_parity_manifest();
    let errs = validate_reference_parity_manifest(&manifest, &crate_dir());
    // assert
    assert!(
        errs.is_empty(),
        "reference-parity manifest is not honest:\n{}",
        errs.join("\n")
    );
}

#[test]
#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn rejects_self_oracle_expected_cell_path() {
    // arrange
    // act
    let mut manifest = parse_reference_parity_manifest();
    // Inject a self-oracle path on a visual row.
    manifest["rows"][0]["expected_semantic_cell_artifact"] = serde_json::Value::String(
        "target/test-lanes/latest/signoff-parity/evidence/actual/harness-x/terminal.txt"
            .to_string(),
    );
    let rid = manifest["rows"][0]["requirement_id"]
        .as_str()
        .unwrap_or("?")
        .to_string();
    let errs = validate_reference_parity_manifest(&manifest, &crate_dir());
    let hit = errs
        .iter()
        .any(|e| e.contains(&rid) && e.contains("self-oracle"));
    // assert
    assert!(
        hit,
        "validator must reject self-oracle path with row id {rid}; errs:\n{}",
        errs.join("\n")
    );
}

#[test]
#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn rejects_stale_reference_sha() {
    // arrange
    // act
    let mut manifest = parse_reference_parity_manifest();
    manifest["reference"]["binary_sha256"] = serde_json::Value::String("deadbeef".repeat(8));
    let errs = validate_reference_parity_manifest(&manifest, &crate_dir());
    // assert
    assert!(
        errs.iter().any(|e| e.contains("pinned reference SHA")),
        "validator must reject stale reference SHA; errs:\n{}",
        errs.join("\n")
    );
}

#[test]
#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn rejects_oversized_identity_span() {
    // arrange
    // act
    let mut manifest = parse_reference_parity_manifest();
    // Add a non-identity (content) field to a visual row's identity mask.
    manifest["rows"][0]["identity_substitution"]["fields"] =
        serde_json::json!(["logo_glyphs", "composer_content"]);
    let rid = manifest["rows"][0]["requirement_id"]
        .as_str()
        .unwrap_or("?")
        .to_string();
    let errs = validate_reference_parity_manifest(&manifest, &crate_dir());
    let hit = errs
        .iter()
        .any(|e| e.contains(&rid) && e.contains("oversized identity span"));
    // assert
    assert!(
        hit,
        "validator must reject oversized identity span with row id {rid}; errs:\n{}",
        errs.join("\n")
    );
}

#[test]
#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn rejects_historical_visual_pass() {
    // arrange
    // act
    let mut manifest = parse_reference_parity_manifest();
    manifest["rows"][0]["status"] = serde_json::Value::String("pass".to_string());
    let rid = manifest["rows"][0]["requirement_id"]
        .as_str()
        .unwrap_or("?")
        .to_string();
    let errs = validate_reference_parity_manifest(&manifest, &crate_dir());
    let hit = errs
        .iter()
        .any(|e| e.contains(&rid) && e.contains("historical pass"));
    // assert
    assert!(
        hit,
        "validator must reject historical visual pass with row id {rid}; errs:\n{}",
        errs.join("\n")
    );
}

#[test]
#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn rejects_restored_row_not_incomplete() {
    // arrange
    // act
    let mut manifest = parse_reference_parity_manifest();
    // Find the TX-TOOL row and flip it to pass.
    let row = manifest["rows"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["requirement_id"].as_str() == Some("TX-TOOL"))
        .expect("TX-TOOL row present");
    row["status"] = serde_json::Value::String("pass".to_string());
    let errs = validate_reference_parity_manifest(&manifest, &crate_dir());
    let hit = errs
        .iter()
        .any(|e| e.contains("TX-TOOL") && e.contains("restored"));
    // assert
    assert!(
        hit,
        "validator must reject a restored row not marked incomplete; errs:\n{}",
        errs.join("\n")
    );
}

#[test]
#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn rejects_external_exclusion_not_excluded() {
    // arrange
    // act
    let mut manifest = parse_reference_parity_manifest();
    if let Some(ex) = manifest["external_exclusions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e["capability_id"].as_str() == Some("cli.share"))
    {
        ex["status"] = serde_json::Value::String("incomplete".to_string());
    }
    let errs = validate_reference_parity_manifest(&manifest, &crate_dir());
    let hit = errs
        .iter()
        .any(|e| e.contains("cli.share") && e.contains("must remain excluded"));
    // assert
    assert!(
        hit,
        "validator must reject an external exclusion not excluded; errs:\n{}",
        errs.join("\n")
    );
}

// Manual QA (task 1): render one wide and one compact Core fixture through the
// existing semantic-frame loader and inspect cell contents/cursor. Always
// asserts the frames load with the right viewport, cursor, and non-default
// content. When HARNESS_QA_FRAMES_DIR is set, also writes the loader's debug
// render (`to_cells_txt`) and a qa.json summary to that evidence directory.
#[test]
#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn core_frame_loader_renders_wide_and_compact() {
    // arrange
    // act
    let crate_dir = crate_dir();
    // (frame, cols, rows): one wide (120x32) and one compact (60x20) Core frame.
    let cases: &[(&str, u16, u16)] = &[
        ("startup-welcome-120x32", 120, 32),
        ("startup-compact-60x20", 60, 20),
    ];
    let mut frames_out: Vec<Value> = Vec::new();
    let qa_dir = std::env::var("HARNESS_QA_FRAMES_DIR").ok();
    if let Some(dir) = &qa_dir {
        let _ = fs::create_dir_all(dir);
    }
    for (frame, cols, rows) in cases {
        let cells_rel = format!(
            "crates/harness-tui/tests/fixtures/grok-build-v0.1.220-alpha.4/core/{frame}/cells.json"
        );
        let cells_path = resolve_under_crate(&crate_dir, &cells_rel);
        let loaded = SemanticFrame::read_cells_json(&cells_path)
            .unwrap_or_else(|err| panic!("loader must read {frame} cells.json: {err}"));
        // assert
        assert_eq!(loaded.cols, *cols, "{frame} cols mismatch");
        assert_eq!(loaded.rows, *rows, "{frame} rows mismatch");
        let debug = loaded.to_cells_txt();
        assert!(
            debug.contains("cursor="),
            "{frame} loader debug render must expose cursor state"
        );
        let non_default = loaded
            .cells
            .iter()
            .filter(|c| !c.grapheme.is_empty())
            .count();
        assert!(
            non_default > 0,
            "{frame} loader must observe non-default cell content"
        );
        if let Some(dir) = &qa_dir {
            let _ = fs::write(Path::new(dir).join(format!("{frame}.cells.txt")), &debug);
        }
        frames_out.push(serde_json::json!({
            "frame": frame,
            "cols": loaded.cols,
            "rows": loaded.rows,
            "cursor": {
                "row": loaded.cursor.row,
                "col": loaded.cursor.col,
                "visible": loaded.cursor.visible,
            },
            "alternate_screen": loaded.alternate_screen,
            "non_default_cells": non_default,
        }));
    }
    if let Some(dir) = &qa_dir {
        let qa = serde_json::json!({
            "qa": "semantic-frame loader render of one wide (startup-welcome-120x32) and one compact (startup-compact-60x20) Core fixture",
            "loader": "harness_testkit::parity::SemanticFrame::read_cells_json + to_cells_txt",
            "frames": frames_out,
        });
        let _ = fs::write(
            Path::new(dir).join("qa.json"),
            format!("{}\n", serde_json::to_string_pretty(&qa).unwrap()),
        );
    }
}
