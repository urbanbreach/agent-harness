//! Cross-group ownership test for Todo 26.
//!
//! Verifies that all eight retained groups (B-I) have unique group IDs, no overlapping
//! capability IDs, and every group names a real backend owner path. This is
//! the duplicate-group-ownership TDD failure case at the aggregate level.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "contract tests use fail-fast asserts for missing leaf state"
)]

#[path = "../src/leaf_actions/group_b_composer_modes.rs"]
mod group_b_composer_modes;
#[path = "../src/leaf_actions/group_c_screen_modes.rs"]
mod group_c_screen_modes;
#[path = "../src/leaf_actions/group_d_dashboard.rs"]
mod group_d_dashboard;
#[path = "../src/leaf_actions/group_e_media.rs"]
mod group_e_media;
#[path = "../src/leaf_actions/group_f_notices.rs"]
mod group_f_notices;
#[path = "../src/leaf_actions/group_g_extensions.rs"]
mod group_g_extensions;
#[path = "../src/leaf_actions/group_h_navigation.rs"]
mod group_h_navigation;
#[path = "../src/leaf_actions/group_i_preferences.rs"]
mod group_i_preferences;

use std::collections::HashSet;

/// All retained group IDs are unique (B through I).
#[test]
fn all_group_ids_are_unique() {
    let ids = [
        group_b_composer_modes::group_id(),
        group_c_screen_modes::group_id(),
        group_d_dashboard::group_id(),
        group_e_media::group_id(),
        group_f_notices::group_id(),
        group_g_extensions::group_id(),
        group_h_navigation::group_id(),
        group_i_preferences::group_id(),
    ];
    let unique: HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(
        unique.len(),
        8,
        "expected 8 unique group IDs, got {unique:?}"
    );
    for expected in ["B", "C", "D", "E", "F", "G", "H", "I"] {
        assert!(unique.contains(expected), "missing group ID {expected}");
    }
}

/// No capability ID appears in more than one group.
#[test]
fn no_overlapping_capability_ids_across_groups() {
    let all_ids: Vec<&'static str> = [
        group_b_composer_modes::capability_ids(),
        group_c_screen_modes::capability_ids(),
        group_d_dashboard::capability_ids(),
        group_e_media::capability_ids(),
        group_f_notices::capability_ids(),
        group_g_extensions::capability_ids(),
        group_h_navigation::capability_ids(),
        group_i_preferences::capability_ids(),
    ]
    .iter()
    .flat_map(|ids| ids.iter().copied())
    .collect();

    let mut seen = HashSet::new();
    for id in &all_ids {
        assert!(
            seen.insert(*id),
            "duplicate capability id across groups: {id}"
        );
    }
}

/// Every group can resolve at least one capability to a real backend owner.
#[test]
fn every_group_resolves_to_real_backend_owner() {
    let samples: &[(&str, &str)] = &[
        ("B", "tui.vim_mode"),
        ("C", "tui.minimal_mode"),
        ("D", "cli.dashboard"),
        ("E", "tui.inline_media"),
        ("F", "tui.notifications"),
        ("G", "tui.extensions_plugins_ui"),
        ("H", "tui.foreign_import_ui_journey"),
        ("I", "tui.theme_auto_system"),
    ];

    for (group, cap_id) in samples {
        let backend_owner: &str = match *group {
            "B" => group_b_composer_modes::resolve(cap_id)
                .map(|r| r.backend_owner)
                .unwrap_or_else(|| panic!("group B must resolve {cap_id}")),
            "C" => group_c_screen_modes::resolve(cap_id)
                .map(|r| r.backend_owner)
                .unwrap_or_else(|| panic!("group C must resolve {cap_id}")),
            "D" => group_d_dashboard::resolve(cap_id)
                .map(|r| r.backend_owner)
                .unwrap_or_else(|| panic!("group D must resolve {cap_id}")),
            "E" => group_e_media::resolve(cap_id)
                .map(|r| r.backend_owner)
                .unwrap_or_else(|| panic!("group E must resolve {cap_id}")),
            "F" => group_f_notices::resolve(cap_id)
                .map(|r| r.backend_owner)
                .unwrap_or_else(|| panic!("group F must resolve {cap_id}")),
            "G" => group_g_extensions::resolve(cap_id)
                .map(|r| r.backend_owner)
                .unwrap_or_else(|| panic!("group G must resolve {cap_id}")),
            "H" => group_h_navigation::resolve(cap_id)
                .map(|r| r.backend_owner)
                .unwrap_or_else(|| panic!("group H must resolve {cap_id}")),
            "I" => group_i_preferences::resolve(cap_id)
                .map(|r| r.backend_owner)
                .unwrap_or_else(|| panic!("group I must resolve {cap_id}")),
            _ => panic!("unknown group {group}"),
        };
        assert!(
            !backend_owner.is_empty(),
            "group {group} backend_owner must not be empty"
        );
        assert!(
            backend_owner.starts_with("crates/"),
            "group {group} backend_owner must be a real crate path: {backend_owner}"
        );
    }
}

/// The eight implement-disposition capability rows (excluding tui.session_tree)
/// are all covered by exactly one group.
#[test]
fn eight_implement_rows_covered_excluding_session_tree() {
    let implement_rows: &[&str] = &[
        "cli.dashboard",
        "tui.inline_media",
        "tui.vim_mode",
        "tui.minimal_mode",
        "tui.theme_auto_system",
        "tui.notifications",
        "tui.tips",
        "tui.extensions_plugins_ui",
    ];

    for row in implement_rows {
        let found = [
            group_b_composer_modes::resolve(row).is_some(),
            group_c_screen_modes::resolve(row).is_some(),
            group_d_dashboard::resolve(row).is_some(),
            group_e_media::resolve(row).is_some(),
            group_f_notices::resolve(row).is_some(),
            group_g_extensions::resolve(row).is_some(),
            group_h_navigation::resolve(row).is_some(),
            group_i_preferences::resolve(row).is_some(),
        ];
        let count = found.iter().filter(|&&f| f).count();
        assert_eq!(
            count, 1,
            "implement row {row} must be covered by exactly one group, found {count}"
        );
    }

    // tui.session_tree is NOT covered by any group (belongs to Todo 24).
    let session_tree_found = [
        group_b_composer_modes::resolve("tui.session_tree").is_some(),
        group_c_screen_modes::resolve("tui.session_tree").is_some(),
        group_d_dashboard::resolve("tui.session_tree").is_some(),
        group_e_media::resolve("tui.session_tree").is_some(),
        group_f_notices::resolve("tui.session_tree").is_some(),
        group_g_extensions::resolve("tui.session_tree").is_some(),
        group_h_navigation::resolve("tui.session_tree").is_some(),
        group_i_preferences::resolve("tui.session_tree").is_some(),
    ];
    let count = session_tree_found.iter().filter(|&&f| f).count();
    assert_eq!(
        count, 0,
        "tui.session_tree must NOT be covered by any group"
    );
}
