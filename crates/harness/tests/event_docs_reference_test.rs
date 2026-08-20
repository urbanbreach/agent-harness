use harness::UnwrapOrAbort;
use std::collections::BTreeSet;

mod common;

use common::repo_root;

fn event_variants_from_source(source: &str) -> BTreeSet<String> {
    let enum_body = source
        .split("pub enum EventV1 {")
        .nth(1)
        .and_then(|tail| tail.split_once("}\n"))
        .map(|(body, _)| body)
        .unwrap_or_abort();

    enum_body
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("#") || !trimmed.contains('(') {
                return None;
            }
            trimmed
                .split('(')
                .next()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        })
        .collect()
}

fn documented_event_variants(doc: &str) -> BTreeSet<String> {
    let mut section = doc.split("### Event Types\n").nth(1).unwrap_or_abort();
    if let Some((current, _rest)) = section.split_once("\n## ") {
        section = current;
    }

    section
        .split('`')
        .enumerate()
        .filter_map(|(index, token)| (index % 2 == 1).then_some(token))
        .filter(|token| token.chars().next().is_some_and(char::is_uppercase))
        .map(str::to_string)
        .collect()
}

fn struct_fields(source: &str, name: &str) -> BTreeSet<String> {
    source
        .split(&format!("pub struct {name} {{"))
        .nth(1)
        .and_then(|tail| tail.split_once("}\n"))
        .map(|(body, _)| body)
        .unwrap_or_abort()
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("pub ")
                .and_then(|field| field.split_once(':'))
                .map(|(name, _)| name.to_string())
        })
        .collect()
}

fn documented_task_scheduled_fields(doc: &str) -> BTreeSet<String> {
    doc.lines()
        .find(|line| line.starts_with("- `TaskScheduled`"))
        .unwrap_or_abort()
        .split('`')
        .enumerate()
        .filter_map(|(index, token)| (index % 2 == 1).then_some(token))
        .filter(|token| *token != "TaskScheduled" && !token.contains('.'))
        .map(str::to_string)
        .collect()
}

#[test]
fn architecture_event_docs_match_event_v1_variants() {
    // arrange
    // act
    // assert
    let root = repo_root();
    let event_source =
        std::fs::read_to_string(root.join("crates/harness-core/src/event.rs")).unwrap_or_abort();
    let architecture_doc =
        std::fs::read_to_string(root.join("docs/architecture/architecture.md")).unwrap_or_abort();

    assert_eq!(
        documented_event_variants(&architecture_doc),
        event_variants_from_source(&event_source),
        "docs/architecture/architecture.md Event Types drifted from EventV1"
    );
}

#[test]
fn architecture_task_scheduled_fields_match_public_event_shape() {
    // arrange: the public event source and architecture contract.
    let root = repo_root();
    let event_source =
        std::fs::read_to_string(root.join("crates/harness-core/src/event.rs")).unwrap_or_abort();
    let architecture_doc =
        std::fs::read_to_string(root.join("docs/architecture/architecture.md")).unwrap_or_abort();

    // act: source and documented TaskScheduled fields are extracted.
    let source_fields = struct_fields(&event_source, "TaskScheduledEvent");
    let documented_fields = documented_task_scheduled_fields(&architecture_doc);

    // assert: docs name every serialized top-level field exactly once.
    assert_eq!(documented_fields, source_fields);
}
