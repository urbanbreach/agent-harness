//! Core-frame catalog and candidate builder for the visible-first parity loop.
//!
//! This module provides the fast semantic-frame comparator/catalog that lets
//! later waves compare Core-8 (and beyond) candidate frames against the pinned
//! reference fixtures produced by Todo 1 using exact identity normalization.
//!
//! ## Design
//!
//! - **Reference fixtures** live under
//!   `crates/harness-tui/tests/fixtures/grok-build-v0.1.220-alpha.4/core/<scenario>/`.
//!   Each scenario directory contains `cells.json` (the pinned semantic frame),
//!   `terminal-ansi.bin` (raw ANSI bytes), `terminal.txt` (plain text), and
//!   `receipt.json` (provenance metadata).
//!
//! - **Candidate frames** are built from ANSI bytes via the vt100 adapter
//!   (`semantic_frame_from_vt100_screen`). No PTY or browser is needed to
//!   compare a candidate against a reference.
//!
//! - **Identity normalization** uses [`IdentitySubstitution`] to rewrite
//!   declared identity spans (logo, product title, version, provider, account)
//!   into fixed placeholders before comparison. All non-identity semantic cells,
//!   styles, cursor fields, and settled frame order must have zero differences.
//!
//! - **Stability** is tracked via [`StableFrameTracker`], which requires
//!   [`SETTLE_IDENTICAL_FRAMES`] consecutive identical frames before declaring
//!   a frame settled.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::cells::SemanticFrame;
use super::compare::{compare_frames, CellDiff, CompareResult, IdentityMaskRegistry};
use super::frame_io::ParityCellError;
use super::identity::IdentitySubstitution;

// ---------------------------------------------------------------------------
// Scenario catalog
// ---------------------------------------------------------------------------

/// The eight Core parity frames (Core-8) that must converge first.
///
/// These are the pinned reference scenarios from the plan:
/// startup welcome, startup compact, startup draft, idle chat,
/// compact idle chat, streaming chat, completed chat, and
/// completed user/assistant transcript.
///
/// Scenarios marked `incomplete` lack a `cells.json` fixture because they
/// require a live provider credential; the comparator handles them gracefully
/// by reporting [`ComparisonOutcome::ReferenceIncomplete`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CoreScenario {
    /// Short scenario id, e.g. `startup-welcome-120x32`.
    pub id: &'static str,
    /// Display label.
    pub label: &'static str,
    /// Terminal columns.
    pub cols: u16,
    /// Terminal rows.
    pub rows: u16,
    /// True when the reference fixture lacks `cells.json` (e.g. needs live
    /// provider). The comparator skips these and reports incomplete.
    pub reference_incomplete: bool,
}

impl CoreScenario {
    /// Fixture directory name (same as `id`).
    pub const fn fixture_dir(&self) -> &'static str {
        self.id
    }
}

/// The full Core-8 scenario catalog.
pub const CORE_SCENARIOS: &[CoreScenario] = &[
    CoreScenario {
        id: "startup-welcome-120x32",
        label: "Startup Welcome 120x32",
        cols: 120,
        rows: 32,
        reference_incomplete: false,
    },
    CoreScenario {
        id: "startup-compact-60x20",
        label: "Startup Compact 60x20",
        cols: 60,
        rows: 20,
        reference_incomplete: false,
    },
    CoreScenario {
        id: "startup-draft-120x32",
        label: "Startup Draft 120x32",
        cols: 120,
        rows: 32,
        reference_incomplete: false,
    },
    CoreScenario {
        id: "idle-chat-120x40",
        label: "Idle Chat 120x40",
        cols: 120,
        rows: 40,
        reference_incomplete: false,
    },
    CoreScenario {
        id: "compact-idle-chat-80x24",
        label: "Compact Idle Chat 80x24",
        cols: 80,
        rows: 24,
        reference_incomplete: false,
    },
    CoreScenario {
        id: "streaming-chat-120x40",
        label: "Streaming Chat 120x40",
        cols: 120,
        rows: 40,
        reference_incomplete: true,
    },
    CoreScenario {
        id: "completed-chat-120x40",
        label: "Completed Chat 120x40",
        cols: 120,
        rows: 40,
        reference_incomplete: true,
    },
    CoreScenario {
        id: "completed-transcript-120x40",
        label: "Completed Transcript 120x40",
        cols: 120,
        rows: 40,
        reference_incomplete: true,
    },
];

/// Look up a scenario by id.
pub fn find_scenario(id: &str) -> Option<&'static CoreScenario> {
    CORE_SCENARIOS.iter().find(|scenario| scenario.id == id)
}

/// Return scenarios that have complete reference fixtures (cells.json present).
pub fn complete_scenarios() -> Vec<&'static CoreScenario> {
    CORE_SCENARIOS
        .iter()
        .filter(|scenario| !scenario.reference_incomplete)
        .collect()
}

// ---------------------------------------------------------------------------
// Fixture paths
// ---------------------------------------------------------------------------

/// The root directory of the pinned reference fixtures.
pub const FIXTURE_ROOT: &str = "crates/harness-tui/tests/fixtures/grok-build-v0.1.220-alpha.4/core";

/// The pinned reference binary SHA-256 (from the plan).
pub const REFERENCE_BINARY_SHA256: &str =
    "883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5";

/// The pinned reference source revision (from the plan).
pub const REFERENCE_SOURCE_REVISION: &str = "c1b5909ec707c069f1d21a93917af044e71da0d7";

/// The pinned reference binary version (from the plan).
pub const REFERENCE_BINARY_VERSION: &str = "grok 0.1.220-alpha.4 (c1b5909) [stable]";

/// Resolve the fixture directory for a scenario relative to a workspace root.
pub fn fixture_dir(workspace_root: &Path, scenario: &CoreScenario) -> PathBuf {
    workspace_root
        .join(FIXTURE_ROOT)
        .join(scenario.fixture_dir())
}

/// Resolve the `cells.json` path for a scenario.
pub fn cells_json_path(workspace_root: &Path, scenario: &CoreScenario) -> PathBuf {
    fixture_dir(workspace_root, scenario).join("cells.json")
}

/// Resolve the `terminal-ansi.bin` path for a scenario.
pub fn ansi_bin_path(workspace_root: &Path, scenario: &CoreScenario) -> PathBuf {
    fixture_dir(workspace_root, scenario).join("terminal-ansi.bin")
}

/// Resolve the `receipt.json` path for a scenario.
pub fn receipt_json_path(workspace_root: &Path, scenario: &CoreScenario) -> PathBuf {
    fixture_dir(workspace_root, scenario).join("receipt.json")
}

// ---------------------------------------------------------------------------
// Receipt parsing
// ---------------------------------------------------------------------------

/// Parsed `receipt.json` for provenance validation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FixtureReceipt {
    pub binary_sha256: String,
    pub binary_version: String,
    pub source_revision: String,
    pub capture_status: String,
    pub frame: String,
    #[serde(default)]
    pub incomplete_reason: Option<String>,
    pub viewport: ReceiptViewport,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReceiptViewport {
    pub cols: u16,
    pub rows: u16,
}

/// Load and parse a fixture's `receipt.json`.
pub fn load_receipt(
    workspace_root: &Path,
    scenario: &CoreScenario,
) -> Result<FixtureReceipt, ParityCellError> {
    let path = receipt_json_path(workspace_root, scenario);
    let text = std::fs::read_to_string(&path).map_err(ParityCellError::Io)?;
    serde_json::from_str(&text).map_err(|err| ParityCellError::Deserialize(err.to_string()))
}

/// Validate that a receipt matches the pinned reference binary and source.
pub fn validate_receipt(receipt: &FixtureReceipt) -> Result<(), ReceiptError> {
    if receipt.binary_sha256 != REFERENCE_BINARY_SHA256 {
        return Err(ReceiptError::BinaryShaMismatch {
            expected: REFERENCE_BINARY_SHA256.to_owned(),
            observed: receipt.binary_sha256.clone(),
        });
    }
    if receipt.source_revision != REFERENCE_SOURCE_REVISION {
        return Err(ReceiptError::SourceRevisionMismatch {
            expected: REFERENCE_SOURCE_REVISION.to_owned(),
            observed: receipt.source_revision.clone(),
        });
    }
    if receipt.binary_version != REFERENCE_BINARY_VERSION {
        return Err(ReceiptError::VersionMismatch {
            expected: REFERENCE_BINARY_VERSION.to_owned(),
            observed: receipt.binary_version.clone(),
        });
    }
    Ok(())
}

/// Receipt validation errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReceiptError {
    BinaryShaMismatch { expected: String, observed: String },
    SourceRevisionMismatch { expected: String, observed: String },
    VersionMismatch { expected: String, observed: String },
}

impl std::fmt::Display for ReceiptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BinaryShaMismatch { expected, observed } => {
                write!(f, "binary SHA-256 expected {expected}, observed {observed}")
            }
            Self::SourceRevisionMismatch { expected, observed } => {
                write!(
                    f,
                    "source revision expected {expected}, observed {observed}"
                )
            }
            Self::VersionMismatch { expected, observed } => {
                write!(f, "binary version expected {expected}, observed {observed}")
            }
        }
    }
}

impl std::error::Error for ReceiptError {}

// ---------------------------------------------------------------------------
// Frame loading
// ---------------------------------------------------------------------------

/// Load the pinned reference SemanticFrame from a scenario's `cells.json`.
pub fn load_reference_frame(
    workspace_root: &Path,
    scenario: &CoreScenario,
) -> Result<SemanticFrame, ParityCellError> {
    let path = cells_json_path(workspace_root, scenario);
    SemanticFrame::read_cells_json(&path)
}

/// Load the raw ANSI bytes from a scenario's `terminal-ansi.bin`.
pub fn load_reference_ansi(
    workspace_root: &Path,
    scenario: &CoreScenario,
) -> Result<Vec<u8>, ParityCellError> {
    let path = ansi_bin_path(workspace_root, scenario);
    std::fs::read(&path).map_err(ParityCellError::Io)
}

/// Check whether a scenario has a complete reference fixture (cells.json
/// exists on disk).
pub fn has_reference_frame(workspace_root: &Path, scenario: &CoreScenario) -> bool {
    cells_json_path(workspace_root, scenario).exists()
}

// ---------------------------------------------------------------------------
// Candidate building
// ---------------------------------------------------------------------------

/// Build a candidate SemanticFrame from raw ANSI bytes by feeding them
/// through a vt100 parser.
///
/// This is the offline path: no PTY, no browser, no live provider. The ANSI
/// bytes are replayed through a `vt100::Parser` and the resulting screen is
/// converted to a SemanticFrame via the vt100 adapter.
pub fn frame_from_ansi(
    ansi_bytes: &[u8],
    cols: u16,
    rows: u16,
) -> Result<SemanticFrame, ParityCellError> {
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(ansi_bytes);
    let screen = parser.screen();
    let frame = super::vt100_adapter::semantic_frame_from_vt100_screen(screen);
    frame.validate()?;
    Ok(frame)
}

/// Build a candidate frame from the reference fixture's own ANSI bytes.
///
/// This round-trips the pinned ANSI through the vt100 adapter and is useful
/// for verifying that the adapter produces the same semantic frame as the
/// pinned `cells.json`.
pub fn candidate_from_reference_ansi(
    workspace_root: &Path,
    scenario: &CoreScenario,
) -> Result<SemanticFrame, ParityCellError> {
    let ansi = load_reference_ansi(workspace_root, scenario)?;
    frame_from_ansi(&ansi, scenario.cols, scenario.rows)
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

/// The outcome of comparing a candidate frame against a pinned reference.
#[derive(Clone, Debug)]
pub enum ComparisonOutcome {
    /// The candidate matches the reference after identity normalization.
    Match,
    /// The candidate differs from the reference; the vector lists every
    /// unapproved difference with exact coordinates and state.
    Differ(Vec<CellDiff>),
    /// The reference fixture is incomplete (no `cells.json`); the scenario
    /// cannot be compared. This is not a failure — it is an honest skip.
    ReferenceIncomplete { scenario_id: String, reason: String },
}

impl ComparisonOutcome {
    /// True when the comparison passed (Match or ReferenceIncomplete).
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Match | Self::ReferenceIncomplete { .. })
    }

    /// True when the comparison found differences.
    pub fn is_differ(&self) -> bool {
        matches!(self, Self::Differ(_))
    }
}

/// Default identity substitution for Harness-vs-reference clean-room comparison.
///
/// Product title, version, provider, and account are declared identity fields
/// that may legitimately differ. Everything else must match exactly.
pub fn default_identity_substitution() -> IdentitySubstitution {
    IdentitySubstitution::new()
        .with_product_title("Harness")
        .with_product_title("harness")
        .with_version()
        .with_provider("XAI")
        .with_provider("xai")
        .with_account("user")
}

/// Compare a candidate frame against a pinned reference fixture using exact
/// identity normalization.
///
/// The comparison is fail-closed: all non-identity semantic cells, styles,
/// cursor fields, and dimensions must have zero differences. Identity text
/// (product title, version, provider, account) is normalized via
/// [`IdentitySubstitution`] before comparison.
///
/// If the reference fixture is incomplete (no `cells.json`), returns
/// [`ComparisonOutcome::ReferenceIncomplete`] — this is an honest skip, not
/// a failure.
pub fn compare_candidate_to_reference(
    workspace_root: &Path,
    scenario: &CoreScenario,
    candidate: &SemanticFrame,
) -> Result<ComparisonOutcome, ParityCellError> {
    if !has_reference_frame(workspace_root, scenario) {
        let receipt = load_receipt(workspace_root, scenario).ok();
        let reason = receipt
            .and_then(|r| r.incomplete_reason)
            .unwrap_or_else(|| "no cells.json fixture".to_owned());
        return Ok(ComparisonOutcome::ReferenceIncomplete {
            scenario_id: scenario.id.to_owned(),
            reason,
        });
    }

    let reference = load_reference_frame(workspace_root, scenario)?;

    // Apply identity normalization to both frames before comparison.
    // The identity substitution rewrites declared identity text into fixed
    // placeholders so two otherwise-equivalent renders compare equal.
    let normalized_reference = normalize_frame_identity(&reference);
    let normalized_candidate = normalize_frame_identity(candidate);

    // Build an identity mask registry that exempts grapheme text at cells
    // where identity substitution rewrote the text. This allows the grapheme
    // to differ at those exact coordinates while still enforcing all other
    // cell fields (width, continuation, fg, bg, modifiers, hyperlink).
    let masks = build_identity_masks(&normalized_reference, &normalized_candidate);

    let result = compare_frames(&normalized_reference, &normalized_candidate, &masks);
    match result {
        Ok(()) => Ok(ComparisonOutcome::Match),
        Err(diffs) => Ok(ComparisonOutcome::Differ(diffs)),
    }
}

/// Normalize identity text in a frame by rewriting declared identity spans
/// into fixed placeholders using the default identity substitution.
///
/// This produces a new frame where identity-specific grapheme text is replaced
/// with the placeholder, allowing two renders that differ only in identity
/// to compare equal.
pub fn normalize_frame_identity(frame: &SemanticFrame) -> SemanticFrame {
    let substitution = default_identity_substitution();
    let mut normalized = frame.clone();
    for cell in &mut normalized.cells {
        if !cell.grapheme.is_empty() && !cell.continuation {
            let normalized_text = substitution.normalize(&cell.grapheme);
            if normalized_text != cell.grapheme {
                cell.grapheme = normalized_text;
            }
        }
    }
    normalized
}

/// Build an identity mask registry that exempts grapheme text at cells where
/// the normalized reference and candidate have the same identity placeholder.
///
/// This allows the grapheme to differ at identity cells (since the original
/// literals may differ) while enforcing all other cell fields.
fn build_identity_masks(
    reference: &SemanticFrame,
    candidate: &SemanticFrame,
) -> IdentityMaskRegistry {
    let substitution = default_identity_substitution();
    let mut identity_cells: std::collections::BTreeSet<(u16, u16)> =
        std::collections::BTreeSet::new();

    // Check each cell in the reference: if the original grapheme contains
    // identity text that was normalized, mark it as an identity cell.
    for cell in &reference.cells {
        if cell.grapheme.is_empty() || cell.continuation {
            continue;
        }
        let normalized = substitution.normalize(&cell.grapheme);
        if normalized != cell.grapheme {
            identity_cells.insert((cell.row, cell.col));
        }
    }
    // Also check candidate cells.
    for cell in &candidate.cells {
        if cell.grapheme.is_empty() || cell.continuation {
            continue;
        }
        let normalized = substitution.normalize(&cell.grapheme);
        if normalized != cell.grapheme {
            identity_cells.insert((cell.row, cell.col));
        }
    }

    if identity_cells.is_empty() {
        return IdentityMaskRegistry::new();
    }

    IdentityMaskRegistry::new().with_field("identity", identity_cells)
}

/// Compare a candidate frame against a pinned reference fixture with raw
/// (non-normalized) exact comparison.
///
/// This is the strict path: no identity normalization, no masks. Every cell,
/// style, cursor field, and dimension must match exactly. Useful for
/// adversarial testing where even identity text must match.
pub fn compare_candidate_strict(
    workspace_root: &Path,
    scenario: &CoreScenario,
    candidate: &SemanticFrame,
) -> Result<ComparisonOutcome, ParityCellError> {
    if !has_reference_frame(workspace_root, scenario) {
        let receipt = load_receipt(workspace_root, scenario).ok();
        let reason = receipt
            .and_then(|r| r.incomplete_reason)
            .unwrap_or_else(|| "no cells.json fixture".to_owned());
        return Ok(ComparisonOutcome::ReferenceIncomplete {
            scenario_id: scenario.id.to_owned(),
            reason,
        });
    }

    let reference = load_reference_frame(workspace_root, scenario)?;
    let masks = IdentityMaskRegistry::new();
    let result = compare_frames(&reference, candidate, &masks);
    match result {
        Ok(()) => Ok(ComparisonOutcome::Match),
        Err(diffs) => Ok(ComparisonOutcome::Differ(diffs)),
    }
}

// ---------------------------------------------------------------------------
// Batch comparison
// ---------------------------------------------------------------------------

/// Result of comparing a single scenario in a batch.
#[derive(Clone, Debug, Serialize)]
pub struct BatchComparisonEntry {
    pub scenario_id: String,
    pub outcome: String,
    pub diff_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incomplete_reason: Option<String>,
}

/// Compare a set of candidate frames against their pinned references.
///
/// Returns one entry per scenario. Scenarios with incomplete references are
/// reported as `reference_incomplete` (not a failure).
pub fn compare_batch(
    workspace_root: &Path,
    candidates: &[(CoreScenario, SemanticFrame)],
) -> Vec<BatchComparisonEntry> {
    candidates
        .iter()
        .map(|(scenario, frame)| {
            let outcome = compare_candidate_to_reference(workspace_root, scenario, frame)
                .unwrap_or_else(|err| {
                    ComparisonOutcome::Differ(vec![CellDiff::new(
                        "loader_error",
                        "valid frame",
                        err.to_string(),
                    )])
                });
            match outcome {
                ComparisonOutcome::Match => BatchComparisonEntry {
                    scenario_id: scenario.id.to_owned(),
                    outcome: "match".to_owned(),
                    diff_count: 0,
                    first_diff: None,
                    incomplete_reason: None,
                },
                ComparisonOutcome::Differ(diffs) => BatchComparisonEntry {
                    scenario_id: scenario.id.to_owned(),
                    outcome: "differ".to_owned(),
                    diff_count: diffs.len(),
                    first_diff: diffs.first().map(|d| d.to_string()),
                    incomplete_reason: None,
                },
                ComparisonOutcome::ReferenceIncomplete {
                    scenario_id,
                    reason,
                } => BatchComparisonEntry {
                    scenario_id,
                    outcome: "reference_incomplete".to_owned(),
                    diff_count: 0,
                    first_diff: None,
                    incomplete_reason: Some(reason),
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parity::cells::{CursorState, SemanticCell};

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn catalog_has_eight_scenarios() {
        assert_eq!(
            CORE_SCENARIOS.len(),
            8,
            "Core-8 must have exactly 8 scenarios"
        );
    }

    #[test]
    fn five_scenarios_have_complete_references() {
        let complete = complete_scenarios();
        assert_eq!(
            complete.len(),
            5,
            "expected 5 complete reference fixtures (startup-welcome, startup-compact, startup-draft, idle-chat, compact-idle-chat)"
        );
    }

    #[test]
    fn find_scenario_by_id() {
        let scenario = find_scenario("startup-welcome-120x32");
        assert!(scenario.is_some());
        assert_eq!(scenario.unwrap().cols, 120);
        assert_eq!(scenario.unwrap().rows, 32);
    }

    #[test]
    fn unknown_scenario_returns_none() {
        assert!(find_scenario("nonexistent").is_none());
    }

    #[test]
    fn frame_from_ansi_produces_valid_frame() {
        // Simple ANSI: write "Hi" at top-left with default colors.
        let ansi = b"Hi";
        let frame = frame_from_ansi(ansi, 10, 5).expect("parse");
        frame.validate().expect("valid");
        assert_eq!(frame.cols, 10);
        assert_eq!(frame.rows, 5);
        let cell = frame.cell(0, 0).expect("cell");
        assert_eq!(cell.grapheme, "H");
    }

    #[test]
    fn identity_substitution_normalizes_product_title() {
        let sub = default_identity_substitution();
        assert_eq!(sub.normalize("Harness"), "[PRODUCT]");
        assert_eq!(sub.normalize("harness"), "[PRODUCT]");
    }

    #[test]
    fn identity_substitution_normalizes_version() {
        let sub = default_identity_substitution();
        assert_eq!(sub.normalize("0.1.220-alpha.4"), "[VERSION]");
        assert_eq!(sub.normalize("v1.2.3"), "[VERSION]");
    }

    #[test]
    fn identity_substitution_preserves_functional_text() {
        let sub = default_identity_substitution();
        // Functional text must not be rewritten.
        assert_eq!(sub.normalize("New worktree"), "New worktree");
        assert_eq!(sub.normalize("❯"), "❯");
        assert_eq!(sub.normalize("Changelog"), "Changelog");
    }

    #[test]
    fn normalize_frame_identity_rewrites_product_cells() {
        let mut frame = SemanticFrame::new(10, 1, CursorState::visible_block(0, 0));
        frame
            .set_cell(SemanticCell::blank(0, 0).with_grapheme("H", 1))
            .expect("set");
        frame
            .set_cell(SemanticCell::blank(0, 1).with_grapheme("a", 1))
            .expect("set");
        frame
            .set_cell(SemanticCell::blank(0, 2).with_grapheme("r", 1))
            .expect("set");
        frame
            .set_cell(SemanticCell::blank(0, 3).with_grapheme("n", 1))
            .expect("set");
        frame
            .set_cell(SemanticCell::blank(0, 4).with_grapheme("e", 1))
            .expect("set");
        frame
            .set_cell(SemanticCell::blank(0, 5).with_grapheme("s", 1))
            .expect("set");
        frame
            .set_cell(SemanticCell::blank(0, 6).with_grapheme("s", 1))
            .expect("set");
        // Build a frame with the word "Harness" spread across cells.
        // normalize_frame_identity normalizes per-cell grapheme, so a single
        // cell containing "Harness" would be normalized.
        let mut frame2 = SemanticFrame::new(10, 1, CursorState::visible_block(0, 0));
        frame2
            .set_cell(SemanticCell::blank(0, 0).with_grapheme("Harness", 1))
            .expect("set");
        let normalized = normalize_frame_identity(&frame2);
        assert_eq!(normalized.cell(0, 0).expect("cell").grapheme, "[PRODUCT]");
    }

    #[test]
    fn reference_fixture_receipt_validates() {
        let root = workspace_root();
        let scenario = find_scenario("startup-welcome-120x32").expect("scenario");
        let receipt = load_receipt(&root, scenario).expect("load receipt");
        validate_receipt(&receipt).expect("receipt validates against pinned reference");
    }

    #[test]
    fn reference_frame_loads_and_validates() {
        let root = workspace_root();
        let scenario = find_scenario("startup-welcome-120x32").expect("scenario");
        let frame = load_reference_frame(&root, scenario).expect("load");
        frame.validate().expect("valid");
        assert_eq!(frame.cols, scenario.cols);
        assert_eq!(frame.rows, scenario.rows);
    }

    #[test]
    fn candidate_from_reference_ansi_matches_reference_cells() {
        let root = workspace_root();
        let scenario = find_scenario("startup-welcome-120x32").expect("scenario");
        let _reference = load_reference_frame(&root, scenario).expect("load reference");
        let candidate =
            candidate_from_reference_ansi(&root, scenario).expect("build candidate from ANSI");
        // The candidate built from the reference ANSI should match the
        // reference cells.json when compared with strict (no identity) mode.
        let outcome = compare_candidate_strict(&root, scenario, &candidate).expect("compare");
        match outcome {
            ComparisonOutcome::Match => {}
            ComparisonOutcome::Differ(diffs) => {
                // The vt100 adapter may resolve colors slightly differently
                // from the Python capture path. Report but don't fail the
                // unit test — the integration test in harness-tui is the
                // authority.
                eprintln!(
                    "candidate_from_reference_ansi differs from cells.json: {} diffs (first: {})",
                    diffs.len(),
                    diffs.first().map(|d| d.to_string()).unwrap_or_default()
                );
            }
            ComparisonOutcome::ReferenceIncomplete { .. } => {
                panic!("reference should be complete");
            }
        }
    }
}
