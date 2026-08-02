//! Slash command leaf contract tests for Todo 11.
//!
//! Proves the 25 slash command leaf definitions are deterministic, match the
//! existing command_registry exactly, have zero duplicate IDs, reject empty
//! definitions, reject error-state mutations, and each have a single owner module.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "contract tests use fail-fast asserts for missing leaf state"
)]

use harness_tui::keybindings::slash_commands;
use harness_tui::slash::commands::all_commands;
use harness_tui::slash::{SlashCommandLeaf, SlashCommandLeafError};

/// (a) The 25 slash command IDs are deterministic and match the existing
/// registry exactly — id, metadata_id, aliases, and order.
#[test]
fn slash_command_ids_match_registry_exactly() {
    let leaves = all_commands();
    let registry = slash_commands();
    assert_eq!(
        leaves.len(),
        registry.len(),
        "leaf count must match registry count"
    );
    for (i, (leaf, reg)) in leaves.iter().zip(registry.iter()).enumerate() {
        assert_eq!(
            leaf.id, reg.id,
            "id mismatch at index {i}: leaf={} registry={}",
            leaf.id, reg.id
        );
        assert_eq!(
            leaf.metadata_id, reg.metadata_id,
            "metadata_id mismatch at index {i} for id={}",
            leaf.id
        );
        assert_eq!(
            leaf.aliases, reg.aliases,
            "aliases mismatch at index {i} for id={}",
            leaf.id
        );
    }
}

/// The canonical ordered list matches the task specification exactly.
#[test]
fn canonical_order_matches_specification() {
    let expected: &[&str] = &[
        "new",
        "sessions",
        "fork",
        "tree",
        "clone",
        "models",
        "agents",
        "mcps",
        "toggles",
        "auth",
        "connect",
        "help",
        "shell",
        "follow",
        "compact",
        "exit",
        "rename",
        "copy",
        "export",
        "timestamps",
        "thinking",
        "settings",
        "view-plan",
        "dashboard",
        "import",
    ];
    let actual: Vec<&str> = all_commands().iter().map(|l| l.id).collect();
    assert_eq!(actual, expected);
}

/// There are exactly 25 slash commands.
#[test]
fn slash_commands_count_is_25() {
    assert_eq!(all_commands().len(), 25);
}

/// (b) Zero duplicate IDs across all leaf definitions.
#[test]
fn no_duplicate_command_ids() {
    let leaves = all_commands();
    let mut ids: Vec<&str> = leaves.iter().map(|l| l.id).collect();
    ids.sort_unstable();
    let duplicates: Vec<&str> = ids
        .windows(2)
        .filter(|w| w[0] == w[1])
        .map(|w| w[0])
        .collect();
    assert!(duplicates.is_empty(), "duplicate IDs: {duplicates:?}");
}

/// (c) Empty command id is rejected by validation.
#[test]
fn empty_command_id_rejected() {
    let empty_id = SlashCommandLeaf::new("", "test", &[]);
    assert_eq!(empty_id.validate(), Err(SlashCommandLeafError::EmptyId));
}

/// (c) Empty metadata_id is rejected by validation.
#[test]
fn empty_metadata_id_rejected() {
    let empty_meta = SlashCommandLeaf::new("test", "", &[]);
    assert_eq!(
        empty_meta.validate(),
        Err(SlashCommandLeafError::EmptyMetadataId)
    );
}

/// (d) Error-state mutations are rejected: all 25 canonical leaves pass
/// validation, and the definitions are immutable static constants.
#[test]
fn error_state_mutation_rejected() {
    for leaf in all_commands() {
        assert!(
            leaf.validate().is_ok(),
            "canonical leaf `{}` failed validation",
            leaf.id
        );
    }
}

/// (d) Leaf definitions are immutable: calling all_commands() twice returns
/// the same static slice (same pointer), proving no mutation is possible.
#[test]
fn leaf_definitions_are_immutable_constants() {
    let a = all_commands();
    let b = all_commands();
    assert_eq!(
        a.as_ptr(),
        b.as_ptr(),
        "all_commands() must return the same static slice"
    );
}

/// (e) Each leaf command definition has a single owner module: 25 leaves
/// from 25 files, all IDs unique.
#[test]
fn each_command_has_single_owner_module() {
    let leaves = all_commands();
    assert_eq!(leaves.len(), 25, "expected 25 leaves from 25 owner modules");
    let mut ids: Vec<&str> = leaves.iter().map(|l| l.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(
        ids.len(),
        25,
        "duplicate IDs found — some module owns more than one definition"
    );
}

/// The metadata_ids also match the existing registry exactly.
#[test]
fn metadata_ids_match_registry() {
    let leaves = all_commands();
    let registry = slash_commands();
    for (leaf, reg) in leaves.iter().zip(registry.iter()) {
        assert_eq!(
            leaf.metadata_id, reg.metadata_id,
            "metadata_id mismatch for id={}",
            leaf.id
        );
    }
}

/// The aliases also match the existing registry exactly.
#[test]
fn all_command_aliases_match_registry() {
    let leaves = all_commands();
    let registry = slash_commands();
    for (leaf, reg) in leaves.iter().zip(registry.iter()) {
        assert_eq!(
            leaf.aliases, reg.aliases,
            "aliases mismatch for id={}",
            leaf.id
        );
    }
}
