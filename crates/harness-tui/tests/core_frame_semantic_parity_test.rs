//! Core-frame semantic parity test (Todo 2).
//!
//! Uses `compare_frames`, `StableFrameTracker`, and exact-span identity
//! normalization to compare Core-8 candidate frames against the pinned
//! reference fixtures produced by Todo 1.
//!
//! ## Test structure
//!
//! - **Comparator reflexivity**: load one reference fixture and verify that
//!   the comparator accepts an identical frame. This checks the comparator;
//!   it is not evidence of Harness-to-reference parity.
//! - **Adversarial mutations**: mutate one glyph, style, cursor, frame order,
//!   identity literal, or settle sequence in a copied fixture; the test
//!   reports exact coordinates/state and fails.
//! - **Stability**: verify `StableFrameTracker` detects settle and rejects
//!   unsettled sequences.
//! - **Incomplete handling**: scenarios with incomplete references are
//!   reported honestly, not silently passed.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

#[path = "support/core_frame_catalog.rs"]
mod core_frame_catalog;

use core_frame_catalog::{
    compare_strict, compare_with_identity, default_identity, first_identity_cell,
    first_non_blank_cell, has_reference, load_receipt, load_reference, mutate_bg, mutate_cursor,
    mutate_cursor_shape, mutate_cursor_visibility, mutate_fg, mutate_glyph, mutate_style,
    new_tracker, no_masks,
};

use harness_testkit::parity::catalog::{
    self, compare_candidate_to_reference, ComparisonOutcome, CORE_SCENARIOS,
};
use harness_testkit::parity::cells::{CursorState, SemanticCell, SemanticFrame};
use harness_testkit::parity::compare::{
    compare_frames, is_settled, CellDiff, IdentityMaskRegistry, StableFrameTracker,
    SETTLE_IDENTICAL_FRAMES,
};
use harness_testkit::parity::identity::IdentitySubstitution;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// ---------------------------------------------------------------------------
// Comparator reflexivity
// ---------------------------------------------------------------------------

#[test]
fn identity_comparator_accepts_an_identical_reference_frame() {
    let scenario = catalog::find_scenario("startup-welcome-120x32").expect("scenario exists");
    let reference = load_reference(scenario).expect("load reference cells.json");
    let outcome = compare_with_identity(scenario, &reference).expect("compare");
    assert!(
        matches!(outcome, ComparisonOutcome::Match),
        "a comparator must be reflexive: {outcome:?}"
    );
}

// ---------------------------------------------------------------------------
// Adversarial: glyph mutation
// ---------------------------------------------------------------------------

#[test]
fn mutated_glyph_fails_with_exact_coordinates() {
    // arrange
    let scenario = catalog::find_scenario("startup-welcome-120x32").expect("scenario");
    let reference = load_reference(scenario).expect("load reference");

    // Find the first non-blank cell to mutate
    let (row, col) = first_non_blank_cell(&reference).expect("found non-blank cell");

    // act: mutate the glyph at that cell
    let mutated = mutate_glyph(&reference, row, col, "X");

    // assert: comparison must fail with a grapheme diff at the exact coordinates
    let outcome = compare_strict(scenario, &mutated).expect("compare");
    match outcome {
        ComparisonOutcome::Differ(diffs) => {
            let grapheme_diff = diffs.iter().find(|d| d.path.contains("grapheme"));
            assert!(
                grapheme_diff.is_some(),
                "must report a grapheme diff, got: {diffs:?}"
            );
            let diff = grapheme_diff.unwrap();
            assert!(
                diff.path.contains(&format!("[{row},{col}]")),
                "diff path must contain exact coordinates [{row},{col}], got: {}",
                diff.path
            );
        }
        _ => panic!("mutated glyph must fail, got: {outcome:?}"),
    }
}

// ---------------------------------------------------------------------------
// Adversarial: style mutation
// ---------------------------------------------------------------------------

#[test]
fn mutated_style_fails_with_exact_field() {
    // arrange
    let scenario = catalog::find_scenario("startup-welcome-120x32").expect("scenario");
    let reference = load_reference(scenario).expect("load reference");
    let (row, col) = first_non_blank_cell(&reference).expect("found non-blank cell");

    // act: mutate the bold modifier
    let mutated = mutate_style(&reference, row, col);

    // assert
    let outcome = compare_strict(scenario, &mutated).expect("compare");
    match outcome {
        ComparisonOutcome::Differ(diffs) => {
            let style_diff = diffs.iter().find(|d| d.path.contains("modifiers"));
            assert!(
                style_diff.is_some(),
                "must report a modifiers diff, got: {diffs:?}"
            );
        }
        _ => panic!("mutated style must fail, got: {outcome:?}"),
    }
}

// ---------------------------------------------------------------------------
// Adversarial: cursor mutation
// ---------------------------------------------------------------------------

#[test]
fn mutated_cursor_position_fails_with_exact_state() {
    // arrange
    let scenario = catalog::find_scenario("startup-welcome-120x32").expect("scenario");
    let reference = load_reference(scenario).expect("load reference");

    // act: move the cursor to a different position
    let new_row = if reference.cursor.row == 0 { 1 } else { 0 };
    let mutated = mutate_cursor(&reference, new_row, reference.cursor.col);

    // assert
    let outcome = compare_strict(scenario, &mutated).expect("compare");
    match outcome {
        ComparisonOutcome::Differ(diffs) => {
            let cursor_diff = diffs.iter().find(|d| d.path.contains("cursor.position"));
            assert!(
                cursor_diff.is_some(),
                "must report a cursor.position diff, got: {diffs:?}"
            );
            let diff = cursor_diff.unwrap();
            assert!(
                diff.path.contains("cursor.position"),
                "diff must name cursor.position, got: {}",
                diff.path
            );
            // The diff must report the exact expected and observed positions
            assert!(
                diff.expected.contains(&reference.cursor.row.to_string()),
                "expected must contain original row {}, got: {}",
                reference.cursor.row,
                diff.expected
            );
            assert!(
                diff.observed.contains(&new_row.to_string()),
                "observed must contain mutated row {}, got: {}",
                new_row,
                diff.observed
            );
        }
        _ => panic!("mutated cursor must fail, got: {outcome:?}"),
    }
}

#[test]
fn mutated_cursor_visibility_fails() {
    let scenario = catalog::find_scenario("startup-welcome-120x32").expect("scenario");
    let reference = load_reference(scenario).expect("load reference");
    let mutated = mutate_cursor_visibility(&reference);
    let outcome = compare_strict(scenario, &mutated).expect("compare");
    match outcome {
        ComparisonOutcome::Differ(diffs) => {
            assert!(
                diffs.iter().any(|d| d.path.contains("cursor.visible")),
                "must report cursor.visible diff, got: {diffs:?}"
            );
        }
        _ => panic!("mutated cursor visibility must fail, got: {outcome:?}"),
    }
}

#[test]
fn mutated_cursor_shape_fails() {
    let scenario = catalog::find_scenario("startup-welcome-120x32").expect("scenario");
    let reference = load_reference(scenario).expect("load reference");
    let mutated = mutate_cursor_shape(&reference);
    let outcome = compare_strict(scenario, &mutated).expect("compare");
    match outcome {
        ComparisonOutcome::Differ(diffs) => {
            assert!(
                diffs.iter().any(|d| d.path.contains("cursor.shape")),
                "must report cursor.shape diff, got: {diffs:?}"
            );
        }
        _ => panic!("mutated cursor shape must fail, got: {outcome:?}"),
    }
}

// ---------------------------------------------------------------------------
// Adversarial: color mutation
// ---------------------------------------------------------------------------

#[test]
fn mutated_fg_color_fails_with_exact_field() {
    let scenario = catalog::find_scenario("startup-welcome-120x32").expect("scenario");
    let reference = load_reference(scenario).expect("load reference");
    let (row, col) = first_non_blank_cell(&reference).expect("found non-blank cell");
    let mutated = mutate_fg(&reference, row, col);
    let outcome = compare_strict(scenario, &mutated).expect("compare");
    match outcome {
        ComparisonOutcome::Differ(diffs) => {
            let fg_diff = diffs.iter().find(|d| d.path.contains(".fg"));
            assert!(fg_diff.is_some(), "must report .fg diff, got: {diffs:?}");
            let diff = fg_diff.unwrap();
            assert!(
                diff.path.contains(&format!("[{row},{col}]")),
                "diff must contain exact coordinates, got: {}",
                diff.path
            );
        }
        _ => panic!("mutated fg must fail, got: {outcome:?}"),
    }
}

#[test]
fn mutated_bg_color_fails_with_exact_field() {
    let scenario = catalog::find_scenario("startup-welcome-120x32").expect("scenario");
    let reference = load_reference(scenario).expect("load reference");
    let (row, col) = first_non_blank_cell(&reference).expect("found non-blank cell");
    let mutated = mutate_bg(&reference, row, col);
    let outcome = compare_strict(scenario, &mutated).expect("compare");
    match outcome {
        ComparisonOutcome::Differ(diffs) => {
            let bg_diff = diffs.iter().find(|d| d.path.contains(".bg"));
            assert!(bg_diff.is_some(), "must report .bg diff, got: {diffs:?}");
        }
        _ => panic!("mutated bg must fail, got: {outcome:?}"),
    }
}

// ---------------------------------------------------------------------------
// Adversarial: frame order mutation (cell swap)
// ---------------------------------------------------------------------------

#[test]
fn mutated_frame_order_fails() {
    // arrange
    let scenario = catalog::find_scenario("startup-welcome-120x32").expect("scenario");
    let reference = load_reference(scenario).expect("load reference");

    // act: swap two non-blank cells to simulate frame order mutation
    let non_blank_indices: Vec<usize> = reference
        .cells
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.grapheme.is_empty() && !c.continuation)
        .map(|(i, _)| i)
        .collect();
    assert!(
        non_blank_indices.len() >= 2,
        "need at least 2 non-blank cells for swap test"
    );
    let idx_a = non_blank_indices[0];
    let idx_b = non_blank_indices[1];

    // Swap the grapheme content of two cells (simulates frame order mutation)
    let mut mutated = reference.clone();
    let grapheme_a = mutated.cells[idx_a].grapheme.clone();
    let grapheme_b = mutated.cells[idx_b].grapheme.clone();
    mutated.cells[idx_a].grapheme = grapheme_b;
    mutated.cells[idx_b].grapheme = grapheme_a;

    // assert: comparison must fail with grapheme diffs at both swapped cells
    let outcome = compare_strict(scenario, &mutated).expect("compare");
    match outcome {
        ComparisonOutcome::Differ(diffs) => {
            let grapheme_diffs: Vec<_> = diffs
                .iter()
                .filter(|d| d.path.contains("grapheme"))
                .collect();
            assert!(
                !grapheme_diffs.is_empty(),
                "must report grapheme diffs for swapped cells, got: {diffs:?}"
            );
        }
        _ => panic!("mutated frame order must fail, got: {outcome:?}"),
    }
}

// ---------------------------------------------------------------------------
// Adversarial: identity literal mutation
// ---------------------------------------------------------------------------

#[test]
fn mutated_identity_literal_fails_when_not_normalized() {
    // arrange: load a reference fixture and find an identity cell
    let scenario = catalog::find_scenario("startup-welcome-120x32").expect("scenario");
    let reference = load_reference(scenario).expect("load reference");

    // Find a cell with identity text (or any non-blank cell if no identity text)
    let target = first_identity_cell(&reference)
        .or_else(|| first_non_blank_cell(&reference))
        .expect("found target cell");
    let (row, col) = target;

    // act: mutate the identity literal to a different identity value
    // (e.g. change "Harness" to "Something" — this is a functional change,
    // not an identity change, so it must fail even with identity normalization)
    let mutated = mutate_glyph(&reference, row, col, "Z");

    // assert: with strict comparison, this must fail
    let outcome = compare_strict(scenario, &mutated).expect("compare");
    match outcome {
        ComparisonOutcome::Differ(diffs) => {
            assert!(
                diffs.iter().any(|d| d.path.contains("grapheme")),
                "must report grapheme diff for mutated identity literal, got: {diffs:?}"
            );
        }
        _ => panic!("mutated identity literal must fail strict comparison, got: {outcome:?}"),
    }
}

#[test]
fn identity_normalization_allows_product_title_substitution() {
    // arrange: create Harness and reference identity frames.
    let mut frame_a = SemanticFrame::new(20, 1, CursorState::visible_block(0, 0));
    frame_a
        .set_cell(SemanticCell::blank(0, 0).with_grapheme("Harness", 1))
        .expect("set");

    let mut frame_b = SemanticFrame::new(20, 1, CursorState::visible_block(0, 0));
    frame_b
        .set_cell(SemanticCell::blank(0, 0).with_grapheme("Grok", 1))
        .expect("set");

    // act: normalize both frames and compare
    let normalized_a = catalog::normalize_frame_identity(&frame_a);
    let normalized_b = catalog::normalize_frame_identity(&frame_b);

    // assert: after normalization, both cells should have the placeholder
    assert_eq!(normalized_a.cell(0, 0).unwrap().grapheme, "[PRODUCT]");
    // The reference title is not a registered identity title, so it stays as-is.
    assert_eq!(normalized_b.cell(0, 0).unwrap().grapheme, "Grok");

    // The frames should differ because only the Harness identity normalizes.
    let result = compare_frames(&normalized_a, &normalized_b, &no_masks());
    assert!(
        result.is_err(),
        "reference identity vs [PRODUCT] must differ"
    );
}

// ---------------------------------------------------------------------------
// Adversarial: settle sequence mutation
// ---------------------------------------------------------------------------

#[test]
fn stable_frame_tracker_detects_settle_after_three_identical() {
    // arrange
    let mut tracker = new_tracker();
    let frame = SemanticFrame::new(10, 5, CursorState::visible_block(0, 0));

    // act: observe three identical frames
    assert!(!tracker.observe(frame.clone()), "first frame not settled");
    assert!(!tracker.observe(frame.clone()), "second frame not settled");
    assert!(
        tracker.observe(frame.clone()),
        "third identical frame must settle"
    );

    // assert
    assert!(tracker.is_settled(), "tracker must be settled");
    assert_eq!(
        tracker.consecutive_identical(),
        SETTLE_IDENTICAL_FRAMES,
        "must have 3 consecutive identical"
    );
}

#[test]
fn stable_frame_tracker_resets_on_different_frame() {
    // arrange
    let mut tracker = new_tracker();
    let frame_a = SemanticFrame::new(10, 5, CursorState::visible_block(0, 0));
    let mut frame_b = frame_a.clone();
    frame_b
        .set_cell(SemanticCell::blank(0, 0).with_grapheme("X", 1))
        .expect("set");

    // act: observe two identical, then a different frame
    tracker.observe(frame_a.clone());
    tracker.observe(frame_a.clone());
    assert!(!tracker.is_settled(), "not settled after 2 identical");

    // Different frame resets the counter
    tracker.observe(frame_b.clone());
    assert!(
        !tracker.is_settled(),
        "different frame must reset settle counter"
    );
    assert_eq!(
        tracker.consecutive_identical(),
        1,
        "counter must reset to 1 after different frame"
    );

    // Two more identical frames to settle again
    tracker.observe(frame_b.clone());
    assert!(!tracker.is_settled(), "not settled after 2 identical");
    tracker.observe(frame_b.clone());
    assert!(tracker.is_settled(), "settled after 3 identical");
}

#[test]
fn mutated_settle_sequence_fails() {
    // arrange: build a settle sequence with an injected different frame
    let mut tracker = new_tracker();
    let frame = SemanticFrame::new(10, 5, CursorState::visible_block(0, 0));
    let mut mutated_frame = frame.clone();
    mutated_frame
        .set_cell(SemanticCell::blank(0, 0).with_grapheme("Z", 1))
        .expect("set");

    // act: observe two identical, then a mutated frame, then two more identical
    tracker.observe(frame.clone());
    tracker.observe(frame.clone());
    tracker.observe(mutated_frame); // inject mutation
    assert!(
        !tracker.is_settled(),
        "mutated frame in settle sequence must prevent settle"
    );
    tracker.observe(frame.clone());
    tracker.observe(frame.clone());
    tracker.observe(frame.clone());

    // assert: the tracker should now be settled (3 identical after the mutation)
    assert!(
        tracker.is_settled(),
        "3 identical after mutation must settle"
    );
}

#[test]
fn is_settled_rejects_short_sequences() {
    let frame = SemanticFrame::new(10, 5, CursorState::visible_block(0, 0));
    assert!(
        !is_settled(std::slice::from_ref(&frame)),
        "1 frame not settled"
    );
    assert!(
        !is_settled(&[frame.clone(), frame.clone()]),
        "2 identical not settled"
    );
    assert!(
        is_settled(&[frame.clone(), frame.clone(), frame.clone()]),
        "3 identical must be settled"
    );
}

#[test]
fn is_settled_rejects_non_identical_tail() {
    let frame = SemanticFrame::new(10, 5, CursorState::visible_block(0, 0));
    let mut other = frame.clone();
    other
        .set_cell(SemanticCell::blank(0, 0).with_grapheme("X", 1))
        .expect("set");
    // Last 3 frames are not all identical
    let seq = vec![frame.clone(), frame.clone(), frame.clone(), other];
    assert!(!is_settled(&seq), "non-identical tail must not settle");
}

// ---------------------------------------------------------------------------
// Catalog structure
// ---------------------------------------------------------------------------

#[test]
fn catalog_has_eight_core_scenarios() {
    assert_eq!(CORE_SCENARIOS.len(), 8, "Core-8 must have 8 scenarios");
}

#[test]
fn five_scenarios_have_complete_reference_fixtures() {
    let complete: Vec<_> = CORE_SCENARIOS.iter().filter(|s| has_reference(s)).collect();
    assert_eq!(
        complete.len(),
        5,
        "expected 5 complete reference fixtures, got {}",
        complete.len()
    );
}

#[test]
fn incomplete_scenarios_report_honestly() {
    // The streaming/completed chat scenarios require a live provider and
    // are marked incomplete. The comparator must report them as
    // ReferenceIncomplete, not Match or Differ.
    for scenario in CORE_SCENARIOS.iter() {
        if scenario.reference_incomplete {
            let dummy_frame =
                SemanticFrame::new(scenario.cols, scenario.rows, CursorState::hidden(0, 0));
            let outcome = compare_with_identity(scenario, &dummy_frame).expect("compare");
            match outcome {
                ComparisonOutcome::ReferenceIncomplete {
                    scenario_id,
                    reason,
                } => {
                    assert_eq!(
                        scenario_id, scenario.id,
                        "incomplete scenario must report its id"
                    );
                    assert!(!reason.is_empty(), "incomplete reason must be non-empty");
                }
                _ => panic!(
                    "incomplete scenario {} must report ReferenceIncomplete, got: {outcome:?}",
                    scenario.id
                ),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Receipt validation
// ---------------------------------------------------------------------------

#[test]
fn all_complete_receipts_validate_against_pinned_reference() {
    for scenario in CORE_SCENARIOS.iter() {
        if !has_reference(scenario) {
            continue;
        }
        let receipt = load_receipt(scenario).expect("load receipt");
        catalog::validate_receipt(&receipt).unwrap_or_else(|err| {
            panic!(
                "receipt for {} must validate against pinned reference: {err}",
                scenario.id
            )
        });
        assert_eq!(
            receipt.capture_status, "captured",
            "receipt for {} must have capture_status=captured",
            scenario.id
        );
        assert_eq!(
            receipt.viewport.cols, scenario.cols,
            "receipt viewport cols must match scenario for {}",
            scenario.id
        );
        assert_eq!(
            receipt.viewport.rows, scenario.rows,
            "receipt viewport rows must match scenario for {}",
            scenario.id
        );
    }
}

// ---------------------------------------------------------------------------
// Identity normalization correctness
// ---------------------------------------------------------------------------

#[test]
fn identity_substitution_rewrites_version_strings() {
    let sub = default_identity();
    assert_eq!(sub.normalize("0.1.220-alpha.4"), "[VERSION]");
    assert_eq!(sub.normalize("v1.2.3"), "[VERSION]");
    assert_eq!(sub.normalize("1.0.0"), "[VERSION]");
}

#[test]
fn identity_substitution_preserves_ip_addresses() {
    let sub = default_identity();
    // IP addresses must NOT be rewritten (dotted-numeric guard)
    assert_eq!(sub.normalize("10.0.0.1"), "10.0.0.1");
    assert_eq!(sub.normalize("192.168.1.100"), "192.168.1.100");
}

#[test]
fn identity_substitution_preserves_token_counts() {
    let sub = default_identity();
    assert_eq!(sub.normalize("1.7K"), "1.7K");
    assert_eq!(sub.normalize("262K"), "262K");
}

#[test]
fn identity_substitution_preserves_functional_text() {
    let sub = default_identity();
    assert_eq!(sub.normalize("New worktree"), "New worktree");
    assert_eq!(sub.normalize("Resume session"), "Resume session");
    assert_eq!(sub.normalize("Quit"), "Quit");
    assert_eq!(sub.normalize("❯"), "❯");
    assert_eq!(sub.normalize("Changelog"), "Changelog");
}

// ---------------------------------------------------------------------------
// Batch comparison
// ---------------------------------------------------------------------------

#[test]
fn batch_comparison_of_all_complete_scenarios() {
    let root = workspace_root();
    let candidates: Vec<_> = CORE_SCENARIOS
        .iter()
        .filter(|s| has_reference(s))
        .map(|s| {
            let frame = load_reference(s).expect("load reference");
            (s.clone(), frame)
        })
        .collect();

    let results = catalog::compare_batch(&root, &candidates);

    // All complete scenarios should match (comparing reference to itself)
    for entry in &results {
        assert_eq!(
            entry.outcome, "match",
            "self-comparison of {} must match: {:?}",
            entry.scenario_id, entry
        );
    }
}
