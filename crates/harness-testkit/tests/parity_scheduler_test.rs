#![allow(
    clippy::expect_used,
    reason = "test fixtures fail fast when the scheduler source is unavailable"
)]

use std::path::PathBuf;

fn scheduler_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("scripts/parity_task_qa.py");
    std::fs::read_to_string(path).expect("scheduler source is readable")
}

#[test]
fn scheduler_declares_all_required_mutations() {
    // arrange
    let source = scheduler_source();

    // act
    let declared = [
        "dependency-incomplete",
        "reservation-overlap",
        "out-of-write-set",
        "duplicate-patch-application",
        "omitted-task-key",
    ];

    // assert
    for mutation in declared {
        assert!(
            source.contains(mutation),
            "missing scheduler mutation: {mutation}"
        );
    }
}

#[test]
fn scheduler_declares_complete_task_catalog_and_receipt_schema() {
    // arrange
    let source = scheduler_source();

    // act
    let task_declarations = (1..=42).map(|task| format!("TaskSpec({task},"));

    // assert
    for declaration in task_declarations {
        assert!(
            source.contains(&declaration),
            "missing task QA declaration: {declaration}"
        );
    }
    for field in [
        "expected_external_postcondition",
        "observed_external_postcondition",
        "dependency_receipts",
        "qa_passed",
        "result",
    ] {
        assert!(source.contains(field), "missing receipt field: {field}");
    }
}
