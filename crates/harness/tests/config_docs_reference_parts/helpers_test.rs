use std::collections::{BTreeMap, BTreeSet};

use harness_tools::discover_skill_catalog;

use crate::common::repo_root;

pub const V1_PROMPT_PROFILES: &str = "build plan general explore visual-engineering artistry ultrabrain deep quick unspecified-low unspecified-high writing";

pub fn documented_table_keys(doc: &str, heading: &str) -> BTreeSet<String> {
    let mut section = doc
        .split(&format!("## {heading}\n"))
        .nth(1)
        .unwrap_or_else(|| panic!("missing `{heading}` section"));
    if let Some((current, _rest)) = section.split_once("\n## ") {
        section = current;
    }

    section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with("| `") {
                return None;
            }
            let after_tick = &trimmed[3..];
            let key = after_tick.split('`').next()?;
            Some(key.to_string())
        })
        .collect()
}

pub fn documented_tui_default_bindings(doc: &str) -> BTreeMap<String, String> {
    let mut section = doc
        .split("## TUI default bindings\n")
        .nth(1)
        .expect("missing `TUI default bindings` section");
    if let Some((current, _rest)) = section.split_once("\n## ") {
        section = current;
    }

    section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with("| `") || trimmed.contains("| ---") {
                return None;
            }
            let cells = trimmed
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect::<Vec<_>>();
            let action = markdown_code_cell(cells.first()?)?;
            let binding = markdown_code_cell(cells.get(1)?)?;
            Some((action, binding))
        })
        .collect()
}

pub fn markdown_code_cell(cell: &str) -> Option<String> {
    cell.strip_prefix('`')
        .and_then(|value| value.strip_suffix('`'))
        .map(str::to_string)
}

pub fn read_doc(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path))
        .unwrap_or_else(|err| panic!("read {path}: {err}"))
}

pub fn markdown_table_rows(doc: &str) -> Vec<Vec<String>> {
    doc.lines()
        .filter(|line| line.trim_start().starts_with('|'))
        .filter(|line| !line.contains("|---"))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect::<Vec<_>>()
        })
        .filter(|row| row.len() >= 2)
        .collect()
}

pub fn shipped_builtin_skill_entries() -> Vec<(String, String)> {
    let catalog = discover_skill_catalog(&repo_root()).expect("discover shipped skill catalog");
    let mut entries = catalog
        .entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.name.as_str(),
                "git-master" | "review-work" | "frontend-ui-ux"
            )
        })
        .map(|entry| (entry.name.clone(), entry.stable_id.clone()))
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries.len(),
        3,
        "expected the three V1 built-in skill candidates in the catalog"
    );
    entries
}

pub fn maintained_claim_phrases(matrix: &str) -> Vec<String> {
    matrix
        .split("## Maintained claim phrase list")
        .nth(1)
        .expect("claim matrix has maintained phrase list")
        .lines()
        .skip_while(|line| !line.trim_start().starts_with("- "))
        .take_while(|line| line.trim_start().starts_with("- "))
        .map(|line| line.trim_start().trim_start_matches("- ").to_string())
        .collect()
}
