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
            if trimmed.is_empty() || trimmed.starts_with("#") {
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

#[test]
fn architecture_event_docs_match_event_v1_variants() {
    let root = repo_root();
    let event_source =
        std::fs::read_to_string(root.join("crates/harness-core/src/event.rs")).unwrap_or_abort();
    let architecture_doc =
        std::fs::read_to_string(root.join("docs/architecture.md")).unwrap_or_abort();

    assert_eq!(
        documented_event_variants(&architecture_doc),
        event_variants_from_source(&event_source),
        "docs/architecture.md Event Types drifted from EventV1"
    );
}
