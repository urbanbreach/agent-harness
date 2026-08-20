//! Todo 35 differential proof: ordered motion, timing, scroll, resize,
//! streaming, and cancellation parity.
//!
//! These tests lock the P5 motion/timing contract authored in
//! `crates/harness-testkit/src/parity/motion.rs`. Every family has a happy
//! path (a synthetic reference trace that satisfies the contract) and at
//! least one failure mutation (an injected delayed / missing / reordered
//! frame, excessive settle churn, dropped input, raw focus bytes, stale
//! spinner state, or forced resize) that MUST be rejected with a typed
//! `MotionDefect`.
//!
//! The tests additionally bind every proof to the active reference
//! binary SHA-256 and the sealed Harness candidate binary
//! SHA-256 (`13ed49fc…`) by reading the on-disk artifacts through
//! `sha256sum`. The binary identities are recorded in the per-test evidence
//! receipt written under `HARNESS_PARITY_MOTION_EVIDENCE_DIR` (default
//! `.omo/evidence/grok-build-clean-room-parity/20260727-110657/task-35-grok-build-clean-room-parity/`).
//!
//! Deterministic by construction: no wall-clock, no PTY, no live provider.
//! The contract validates *captured* frame sequences, which downstream
//! Todo 34 / 37 PTY and native lanes produce.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "motion contract tests fail fast when sha256sum is unavailable"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use harness_testkit::parity::{
    compare_motion_traces, validate_motion_trace, validate_motion_trace_with_families, CursorState,
    FrameTrace, IdentityMaskRegistry, MotionDefect, MotionFamily, MotionPhase, SemanticCell,
    SemanticFrame, TickFrame, TraceIdentity, TraceSource,
};

#[path = "support/harness_bin.rs"]
mod harness_bin;

// ---------------------------------------------------------------------------
// Binary identity constants (frozen by the plan).
// ---------------------------------------------------------------------------

const REFERENCE_BIN: &str =
    "/home/urbanbreach/Projects/agent-harness/inspirations/grok-build/target/debug/xai-grok-pager";
const REFERENCE_SHA256: &str = "883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5";
const ACTIVE_REFERENCE_SHA256: &str =
    "8ff869cf2db0dea1ee29ee0d7028b180f37ac8ac108c601452fa00d28125ad08";
const REFERENCE_VERSION: &str = "grok 0.1.220-alpha.4 (c1b5909) [stable]";
const REFERENCE_REVISION: &str = "c1b5909ec707c069f1d21a93917af044e71da0d7";

const CANDIDATE_VERSION: &str = "harness 0.1.0";

/// Canonical task-39 reference epoch (not the Git revision of the reference binary).
const CANONICAL_REFERENCE_EPOCH: &str =
    "d65d98422e3e3b2ae9bcba7b6636e01ee81fa625a4928f382d93f6199a924d93";
/// Canonical task-39 product epoch.
const CANONICAL_PRODUCT_EPOCH: &str =
    "a7ca53e1ec3d83e4f561e872e0d365b4d5cad201f30afa1c83dea6b44a0210a3";

fn candidate_bin_path() -> PathBuf {
    if let Some(custom) = std::env::var_os("HARNESS_PARITY_CANDIDATE_PATH") {
        return PathBuf::from(custom);
    }
    PathBuf::from("/home/urbanbreach/Projects/agent-harness/target/debug/harness")
}

struct CandidateFixture {
    _directory: tempfile::TempDir,
    path: PathBuf,
    sha256: String,
}

fn candidate_fixture() -> &'static CandidateFixture {
    static FIXTURE: OnceLock<CandidateFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let source = candidate_bin_path();
        let directory = tempfile::tempdir().expect("candidate fixture directory");
        let path = directory.path().join("harness");
        fs::copy(&source, &path).unwrap_or_else(|error| {
            panic!(
                "copy candidate fixture from {} to {} failed: {error}",
                source.display(),
                path.display()
            )
        });
        let sha256 = binary_sha256(&path);
        if let Ok(expected) = std::env::var("HARNESS_PARITY_CANDIDATE_SHA256") {
            assert_eq!(
                sha256, expected,
                "candidate fixture SHA differs from override"
            );
        }
        CandidateFixture {
            _directory: directory,
            path,
            sha256,
        }
    })
}

fn expected_candidate_sha256() -> &'static str {
    &candidate_fixture().sha256
}

const DEFAULT_EVIDENCE_ROOT: &str = ".omo/evidence/grok-build-clean-room-parity/20260727-110657/task-35-grok-build-clean-room-parity";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn evidence_root() -> PathBuf {
    if let Some(custom) = std::env::var_os("HARNESS_PARITY_MOTION_EVIDENCE_DIR") {
        return PathBuf::from(custom);
    }
    workspace_root().join(DEFAULT_EVIDENCE_ROOT)
}

fn binary_sha256(path: &Path) -> String {
    let path_display = path.display();
    let output = harness_bin::command("sha256sum")
        .arg(path)
        .output()
        .unwrap_or_else(|err| panic!("sha256sum {path_display} failed: {err}"));
    assert!(
        output.status.success(),
        "sha256sum {path_display} exited {:?}: stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("sha256sum {path_display} produced no digest: {stdout}"))
        .to_owned()
}

fn binary_version(path: &Path) -> String {
    let path_display = path.display();
    let output = harness_bin::command(path)
        .arg("--version")
        .output()
        .unwrap_or_else(|err| panic!("{path_display} --version failed: {err}"));
    assert!(
        output.status.success(),
        "{path_display} --version exited {:?}: stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Captures the on-disk binary identity for both binaries, asserting that
/// the SHAs match the active authority and sealed candidate. Returns a
/// `(reference, candidate)` identity pair that motion traces are bound to.
fn capture_binary_identities() -> (TraceIdentity, TraceIdentity) {
    let reference_path = PathBuf::from(REFERENCE_BIN);
    let candidate_fixture = candidate_fixture();
    let candidate_path = &candidate_fixture.path;
    assert!(
        reference_path.is_file(),
        "reference binary missing at {}",
        reference_path.display()
    );
    assert!(
        candidate_path.is_file(),
        "candidate binary missing at {}",
        candidate_path.display()
    );
    let reference_sha = binary_sha256(&reference_path);
    let candidate_sha = binary_sha256(candidate_path);
    assert_eq!(
        reference_sha, ACTIVE_REFERENCE_SHA256,
        "reference binary SHA drifted from active authority"
    );
    assert_eq!(
        candidate_sha, candidate_fixture.sha256,
        "candidate binary SHA drifted from sealed pinned value"
    );
    let reference_version = binary_version(&reference_path);
    let candidate_version = binary_version(candidate_path);
    assert!(
        reference_version.starts_with("grok "),
        "reference --version unexpected: {reference_version:?}"
    );
    assert!(
        candidate_version.starts_with("harness "),
        "candidate --version unexpected: {candidate_version:?}"
    );
    let candidate_cmd = candidate_path.to_string_lossy().into_owned();
    (
        TraceIdentity {
            source: TraceSource::Reference,
            binary_sha256: reference_sha,
            binary_version: reference_version,
            generating_command: format!("{REFERENCE_BIN} --version"),
        },
        TraceIdentity {
            source: TraceSource::Harness,
            binary_sha256: candidate_sha,
            binary_version: candidate_version,
            generating_command: format!("{candidate_cmd} --version"),
        },
    )
}

fn write_evidence_receipt(
    identities: &(TraceIdentity, TraceIdentity),
    families: &[MotionFamily],
    rows: &[EvidenceRow],
) -> PathBuf {
    let root = evidence_root();
    fs::create_dir_all(&root).expect("evidence root writable");
    let receipt = EvidenceReceipt {
        schema: "grok-parity-motion-timing-receipt-v1".to_owned(),
        task_id: "T35".to_owned(),
        proof_dimension: "P5".to_owned(),
        reference_epoch: CANONICAL_REFERENCE_EPOCH.to_owned(),
        product_epoch: CANONICAL_PRODUCT_EPOCH.to_owned(),
        reference_identity: identities.0.clone(),
        candidate_identity: identities.1.clone(),
        families: families
            .iter()
            .map(|family| family.as_str().to_string())
            .collect(),
        rows: rows.to_vec(),
        summary: EvidenceSummary {
            total_rows: rows.len(),
            passed: rows.iter().filter(|row| row.verdict == "pass").count(),
            blocked: rows.iter().filter(|row| row.verdict == "blocked").count(),
            rejected: rows.iter().filter(|row| row.verdict == "rejected").count(),
            incomplete: rows
                .iter()
                .filter(|row| row.verdict == "incomplete")
                .count(),
        },
        acceptance_eligible: false,
    };
    let receipt_path = root.join("motion-timing-receipt.json");
    fs::write(
        &receipt_path,
        serde_json::to_vec_pretty(&receipt).expect("receipt serializes"),
    )
    .expect("receipt writable");
    receipt_path
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct EvidenceRow {
    family: String,
    scenario: String,
    verdict: String,
    detail: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct EvidenceSummary {
    total_rows: usize,
    passed: usize,
    blocked: usize,
    rejected: usize,
    incomplete: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct EvidenceReceipt {
    schema: String,
    task_id: String,
    proof_dimension: String,
    reference_epoch: String,
    product_epoch: String,
    reference_identity: TraceIdentity,
    candidate_identity: TraceIdentity,
    families: Vec<String>,
    rows: Vec<EvidenceRow>,
    summary: EvidenceSummary,
    acceptance_eligible: bool,
}

// ---------------------------------------------------------------------------
// Synthetic trace builders (deterministic, no PTY).
// ---------------------------------------------------------------------------

fn blank_frame() -> SemanticFrame {
    SemanticFrame::new(8, 2, CursorState::visible_block(1, 0))
}

fn frame_with_grapheme(grapheme: &str, col: u16) -> SemanticFrame {
    let mut frame = blank_frame();
    frame
        .set_cell(SemanticCell::blank(0, col).with_grapheme(grapheme, 1))
        .expect("set");
    frame
}

fn spinner_frame(glyph: &str) -> SemanticFrame {
    let mut frame = blank_frame();
    frame
        .set_cell(SemanticCell::blank(1, 0).with_grapheme(glyph, 1))
        .expect("set");
    frame
}

/// A baseline reference trace that satisfies every motion family.
fn baseline_passing_trace(source: TraceSource) -> FrameTrace {
    let startup = blank_frame();
    let delta1 = frame_with_grapheme("h", 0);
    let delta2 = frame_with_grapheme("i", 1);
    let finish = {
        let mut frame = delta2.clone();
        frame
            .set_cell(SemanticCell::blank(0, 2).with_grapheme(".", 1))
            .expect("set");
        frame
    };
    let settle = finish.clone();
    FrameTrace {
        source,
        frames: vec![
            TickFrame {
                tick: 0,
                phase: MotionPhase::Startup,
                frame: startup,
            },
            TickFrame {
                tick: 1,
                phase: MotionPhase::StreamingDelta,
                frame: delta1,
            },
            TickFrame {
                tick: 2,
                phase: MotionPhase::StreamingDelta,
                frame: delta2,
            },
            TickFrame {
                tick: 3,
                phase: MotionPhase::ScrollFlush,
                frame: finish.clone(),
            },
            TickFrame {
                tick: 4,
                phase: MotionPhase::ResizeBurst,
                frame: finish.clone(),
            },
            TickFrame {
                tick: 5,
                phase: MotionPhase::ResizeSettled,
                frame: finish.clone(),
            },
            TickFrame {
                tick: 6,
                phase: MotionPhase::FinishFlash,
                frame: finish.clone(),
            },
            TickFrame {
                tick: 7,
                phase: MotionPhase::Cancellation,
                frame: finish.clone(),
            },
            TickFrame {
                tick: 8,
                phase: MotionPhase::CancelRecovered,
                frame: finish,
            },
            TickFrame {
                tick: 9,
                phase: MotionPhase::SettleRepeat,
                frame: settle.clone(),
            },
            TickFrame {
                tick: 9,
                phase: MotionPhase::SettleRepeat,
                frame: settle.clone(),
            },
            TickFrame {
                tick: 9,
                phase: MotionPhase::SettleRepeat,
                frame: settle,
            },
        ],
    }
}

fn baseline_cancellation_trace(source: TraceSource) -> FrameTrace {
    baseline_passing_trace(source)
}

fn assert_defect(defects: &[MotionDefect], family: MotionFamily, reason: &str) {
    assert!(
        defects
            .iter()
            .any(|defect| defect.family == family && defect.reason == reason),
        "expected a {family}::{reason} defect; observed defects = {defects:?}"
    );
}

// ---------------------------------------------------------------------------
// Tests: binary identity binding (blocks the whole task if SHAs drift).
// ---------------------------------------------------------------------------

#[test]
fn reference_and_candidate_binaries_match_active_authority() {
    // arrange
    let evidence = capture_binary_identities();

    // act
    let (reference, candidate) = (&evidence.0, &evidence.1);

    // assert
    assert_eq!(reference.binary_sha256, ACTIVE_REFERENCE_SHA256);
    assert_eq!(candidate.binary_sha256, expected_candidate_sha256());
    assert_eq!(reference.source, TraceSource::Reference);
    assert_eq!(candidate.source, TraceSource::Harness);
    assert_ne!(
        reference.binary_sha256, candidate.binary_sha256,
        "self-oracle must be rejected: reference and candidate must be distinct binaries"
    );
}

// ---------------------------------------------------------------------------
// Tests: ordered motion family.
// ---------------------------------------------------------------------------

#[test]
fn ordered_motion_baseline_trace_passes() {
    // arrange
    let trace = baseline_passing_trace(TraceSource::Reference);

    // act
    let defects = validate_motion_trace(&trace);

    // assert
    assert!(
        defects.is_ok(),
        "baseline trace must satisfy every motion family: {:?}",
        defects.err()
    );
}

#[test]
fn ordered_motion_rejects_missing_phase() {
    // arrange: strip the finish flash phase.
    let mut trace = baseline_passing_trace(TraceSource::Reference);
    trace
        .frames
        .retain(|tick_frame| tick_frame.phase != MotionPhase::FinishFlash);

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::OrderedMotion])
        .expect_err("missing finish_flash phase must be rejected");

    // assert
    assert_defect(&defects, MotionFamily::OrderedMotion, "missing_phase");
}

#[test]
fn ordered_motion_rejects_non_monotonic_tick() {
    // arrange: invert two streaming delta ticks.
    let mut trace = baseline_passing_trace(TraceSource::Reference);
    let first_delta = trace
        .frames
        .iter()
        .position(|tick_frame| tick_frame.phase == MotionPhase::StreamingDelta)
        .expect("first delta");
    trace.frames[first_delta].tick = 10;
    trace.frames[first_delta + 1].tick = 5;

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::OrderedMotion])
        .expect_err("non-monotonic ticks must be rejected");

    // assert
    assert_defect(&defects, MotionFamily::OrderedMotion, "non_monotonic_tick");
}

#[test]
fn ordered_motion_rejects_empty_trace() {
    // arrange
    let trace = FrameTrace {
        source: TraceSource::Reference,
        frames: vec![],
    };

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::OrderedMotion])
        .expect_err("empty traces must be rejected");

    // assert
    assert_defect(&defects, MotionFamily::OrderedMotion, "empty_trace");
}

// ---------------------------------------------------------------------------
// Tests: settle dwell family.
// ---------------------------------------------------------------------------

#[test]
fn settle_dwell_requires_three_identical_frames() {
    // arrange: only two SettleRepeat frames.
    let mut trace = baseline_passing_trace(TraceSource::Reference);
    let last_settle_idx = trace
        .frames
        .iter()
        .rposition(|tick_frame| tick_frame.phase == MotionPhase::SettleRepeat)
        .expect("last settle");
    trace.frames.remove(last_settle_idx);

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::SettleDwell])
        .expect_err("insufficient settle dwell must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::SettleDwell,
        "insufficient_settle_dwell",
    );
}

#[test]
fn settle_dwell_rejects_non_identical_tail() {
    // arrange: replace the last settle frame's content.
    let mut trace = baseline_passing_trace(TraceSource::Reference);
    let last_settle_idx = trace
        .frames
        .iter()
        .rposition(|tick_frame| tick_frame.phase == MotionPhase::SettleRepeat)
        .expect("last settle");
    let mutated_frame = {
        let mut frame = trace.frames[last_settle_idx].frame.clone();
        frame
            .set_cell(SemanticCell::blank(0, 5).with_grapheme("Z", 1))
            .expect("set");
        frame
    };
    trace.frames[last_settle_idx].frame = mutated_frame;

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::SettleDwell])
        .expect_err("non-identical settle tail must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::SettleDwell,
        "non_identical_settle_tail",
    );
}

#[test]
fn settle_dwell_rejects_tick_churn() {
    // arrange: shift one of the three SettleRepeat frames to a different tick.
    let mut trace = baseline_passing_trace(TraceSource::Reference);
    let settle_indices: Vec<usize> = trace
        .frames
        .iter()
        .enumerate()
        .filter_map(|(idx, tick_frame)| {
            (tick_frame.phase == MotionPhase::SettleRepeat).then_some(idx)
        })
        .collect();
    trace.frames[settle_indices[1]].tick = 99;

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::SettleDwell])
        .expect_err("settle tick churn must be rejected");

    // assert
    assert_defect(&defects, MotionFamily::SettleDwell, "settle_tick_churn");
}

// ---------------------------------------------------------------------------
// Tests: scroll flush family.
// ---------------------------------------------------------------------------

#[test]
fn scroll_flush_baseline_passes() {
    // arrange
    let trace = baseline_passing_trace(TraceSource::Reference);

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::ScrollFlush]);

    // assert
    assert!(
        defects.is_ok(),
        "scroll flush family must accept baseline trace: {:?}",
        defects.err()
    );
}

#[test]
fn scroll_flush_rejects_missing_phase() {
    // arrange
    let mut trace = baseline_passing_trace(TraceSource::Reference);
    trace
        .frames
        .retain(|tick_frame| tick_frame.phase != MotionPhase::ScrollFlush);

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::ScrollFlush])
        .expect_err("missing scroll_flush phase must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::ScrollFlush,
        "missing_scroll_flush_phase",
    );
}

#[test]
fn scroll_flush_rejects_phase_regression() {
    // arrange: inject a startup phase frame after the scroll flush.
    let mut trace = baseline_passing_trace(TraceSource::Reference);
    let scroll_idx = trace
        .frames
        .iter()
        .position(|tick_frame| tick_frame.phase == MotionPhase::ScrollFlush)
        .expect("scroll flush");
    trace.frames.insert(
        scroll_idx + 1,
        TickFrame {
            tick: trace.frames[scroll_idx].tick + 1,
            phase: MotionPhase::Startup,
            frame: blank_frame(),
        },
    );

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::ScrollFlush])
        .expect_err("scroll phase regression must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::ScrollFlush,
        "scroll_phase_regression",
    );
}

// ---------------------------------------------------------------------------
// Tests: resize debounce family.
// ---------------------------------------------------------------------------

#[test]
fn resize_debounce_baseline_passes() {
    // arrange
    let trace = baseline_passing_trace(TraceSource::Reference);

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::ResizeDebounce]);

    // assert
    assert!(
        defects.is_ok(),
        "resize debounce family must accept baseline trace: {:?}",
        defects.err()
    );
}

#[test]
fn resize_debounce_rejects_missing_burst() {
    // arrange
    let mut trace = baseline_passing_trace(TraceSource::Reference);
    trace
        .frames
        .retain(|tick_frame| tick_frame.phase != MotionPhase::ResizeBurst);

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::ResizeDebounce])
        .expect_err("missing burst must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::ResizeDebounce,
        "missing_resize_burst_phase",
    );
}

#[test]
fn resize_debounce_rejects_missing_settled() {
    // arrange
    let mut trace = baseline_passing_trace(TraceSource::Reference);
    trace
        .frames
        .retain(|tick_frame| tick_frame.phase != MotionPhase::ResizeSettled);

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::ResizeDebounce])
        .expect_err("missing settled must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::ResizeDebounce,
        "missing_resize_settled_phase",
    );
}

#[test]
fn resize_debounce_rejects_settled_before_burst() {
    // arrange: move the resize_settled frame before the resize_burst frame.
    let mut trace = baseline_passing_trace(TraceSource::Reference);
    let burst_idx = trace
        .frames
        .iter()
        .position(|tick_frame| tick_frame.phase == MotionPhase::ResizeBurst)
        .expect("burst");
    let settled_idx = trace
        .frames
        .iter()
        .position(|tick_frame| tick_frame.phase == MotionPhase::ResizeSettled)
        .expect("settled");
    let settled_frame = trace.frames.remove(settled_idx);
    trace.frames.insert(burst_idx, settled_frame);

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::ResizeDebounce])
        .expect_err("settled-before-burst must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::ResizeDebounce,
        "resize_settled_before_burst",
    );
}

#[test]
fn resize_debounce_rejects_churn_after_settle() {
    // arrange: inject a frame with different dimensions after resize_settled.
    let mut trace = baseline_passing_trace(TraceSource::Reference);
    let settled_idx = trace
        .frames
        .iter()
        .position(|tick_frame| tick_frame.phase == MotionPhase::ResizeSettled)
        .expect("settled");
    // A different geometry (4x1) frame after settle is a forced-resize mutation.
    let mutated_geometry_frame = SemanticFrame::new(4, 1, CursorState::visible_block(0, 0));
    trace.frames.insert(
        settled_idx + 1,
        TickFrame {
            tick: trace.frames[settled_idx].tick + 1,
            phase: MotionPhase::FinishFlash,
            frame: mutated_geometry_frame,
        },
    );

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::ResizeDebounce])
        .expect_err("post-settle churn must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::ResizeDebounce,
        "resize_churn_after_settle",
    );
}

#[path = "support/parity_motion_extended_support.rs"]
mod motion_extended;
