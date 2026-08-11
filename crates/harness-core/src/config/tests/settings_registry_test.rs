use super::*;
use std::collections::BTreeSet;

use crate::UnwrapOrAbort;

#[test]
fn settings_registry_is_non_empty() {
    // arrange
    // act
    // assert
    assert!(!settings_registry().is_empty());
    assert!(settings_registry().len() >= 15);
}

#[test]
fn every_registry_entry_has_required_metadata() {
    // arrange
    // act
    // assert
    for entry in settings_registry() {
        assert!(
            !entry.setting_id.as_str().is_empty(),
            "setting_id must be non-empty"
        );
        assert!(
            !entry.schema_id.as_str().is_empty(),
            "schema_id must be non-empty for {}",
            entry.setting_id
        );
        match entry.surface {
            SettingSurface::Runtime | SettingSurface::Tui => {}
        }
        match entry.default_scope {
            SettingScope::System
            | SettingScope::User
            | SettingScope::Profile
            | SettingScope::Project
            | SettingScope::Workspace
            | SettingScope::Worktree
            | SettingScope::Session
            | SettingScope::CommandLine
            | SettingScope::Environment => {}
        }
        match entry.sensitivity {
            SettingSensitivity::Public
            | SettingSensitivity::Redacted
            | SettingSensitivity::Secret => {}
        }
        // capability_dependency is Option — either None or non-empty.
        if let Some(dep) = entry.capability_dependency {
            assert!(!dep.is_empty(), "empty capability for {}", entry.setting_id);
        }
        // restart_required is bool — always present; default presence via Option.
        let _ = entry.restart_required;
        let _ = entry.has_default();
        let _ = entry.default_value;
    }
}

#[test]
fn known_public_keys_are_registered() {
    // arrange
    // act
    // assert
    let ids: BTreeSet<&str> = settings_registry()
        .iter()
        .map(|entry| entry.setting_id.as_str())
        .collect();

    assert!(ids.contains("model"), "model must be registered");
    assert!(
        ids.contains("permission.bash"),
        "permission.bash must be registered"
    );
    assert!(
        ids.contains("permission.edit"),
        "permission.edit must be registered"
    );
    assert!(
        ids.contains("runtime.compaction.enabled"),
        "runtime.compaction.enabled must be registered"
    );
    assert!(
        ids.contains("runtime.session_dir"),
        "runtime.session_dir must be registered"
    );
    assert!(ids.contains("keybinds"), "TUI keybinds must be registered");
}

#[test]
fn permission_core_names_are_registered_with_capability_deps() {
    // arrange
    // act
    // assert
    for name in [
        "bash",
        "edit",
        "question",
        "task",
        "webfetch",
        "websearch",
        "codesearch",
        "lsp",
    ] {
        let id = format!("permission.{name}");
        let entry = setting_definition(&id).unwrap_or_else(|| panic!("missing {id}"));
        assert_eq!(entry.surface, SettingSurface::Runtime);
        assert_eq!(entry.capability_dependency, Some(name));
        assert_eq!(entry.sensitivity, SettingSensitivity::Public);
    }
}

#[test]
fn provider_api_key_is_marked_secret() {
    // arrange
    // act
    // assert
    let entry = setting_definition("provider.apiKey").expect("provider.apiKey registered");
    assert_eq!(entry.sensitivity, SettingSensitivity::Secret);
    assert_eq!(entry.surface, SettingSurface::Runtime);
    assert!(!entry.has_default());
}

#[test]
fn tui_settings_use_tui_surface() {
    // arrange
    // act
    // assert
    let keybinds = setting_definition("keybinds").expect("keybinds registered");
    assert_eq!(keybinds.surface, SettingSurface::Tui);
    assert!(keybinds.has_default());

    let schema = setting_definition("$schema").expect("TUI $schema registered");
    assert_eq!(schema.surface, SettingSurface::Tui);
}

#[test]
fn setting_ids_are_unique() {
    // arrange
    // act
    // assert
    let mut seen = BTreeSet::new();
    for entry in settings_registry() {
        assert!(
            seen.insert(entry.setting_id.as_str()),
            "duplicate setting_id {}",
            entry.setting_id
        );
    }
}

#[test]
fn schema_ids_are_unique() {
    // arrange
    // act
    // assert
    let mut seen = BTreeSet::new();
    for entry in settings_registry() {
        assert!(
            seen.insert(entry.schema_id.as_str()),
            "duplicate schema_id {}",
            entry.schema_id
        );
    }
}

#[test]
fn session_dir_and_compaction_defaults_are_present() {
    // arrange
    // act
    // assert
    let session_dir = setting_definition("runtime.session_dir").expect("session_dir");
    assert!(session_dir.has_default());
    assert_eq!(session_dir.default_value, Some(".agent-harness/sessions"));
    assert!(session_dir.restart_required);

    let compaction = setting_definition("runtime.compaction.enabled").expect("compaction");
    assert_eq!(compaction.default_value, Some("true"));
    assert!(!compaction.restart_required);
}

#[test]
fn every_registry_setting_maps_to_public_path_or_is_metadata_only() {
    // arrange
    // act
    // assert
    let contract = public_config_contract();
    let runtime_roots: BTreeSet<&str> = contract
        .runtime_top_level_keys
        .iter()
        .map(|key| key.name)
        .collect();
    let tui_roots: BTreeSet<&str> = contract
        .tui_top_level_keys
        .iter()
        .map(|key| key.name)
        .collect();

    for entry in settings_registry() {
        let id = entry.setting_id.as_str();
        if is_metadata_only_setting(id) {
            assert!(
                setting_definition(id).is_some(),
                "metadata-only id {id} must still be registered"
            );
            continue;
        }
        let root = id
            .split('.')
            .next()
            .expect("setting_id must have a path segment");
        match entry.surface {
            SettingSurface::Runtime => {
                assert!(
                    runtime_roots.contains(root),
                    "runtime setting `{id}` root `{root}` is not a known public runtime key"
                );
            }
            SettingSurface::Tui => {
                assert!(
                    tui_roots.contains(root),
                    "tui setting `{id}` root `{root}` is not a known public tui key"
                );
            }
        }
    }
}

#[test]
fn worktree_product_defaults_are_metadata_only_stubs() {
    // arrange
    // act
    // assert
    let relative = setting_definition("worktree.relative_base").expect("worktree.relative_base");
    assert!(is_metadata_only_setting("worktree.relative_base"));
    assert_eq!(relative.default_scope, SettingScope::Worktree);
    assert_eq!(
        relative.default_value,
        Some(crate::worktree::DEFAULT_WORKTREE_RELATIVE_BASE)
    );

    let prefix = setting_definition("worktree.branch_prefix").expect("worktree.branch_prefix");
    assert!(is_metadata_only_setting("worktree.branch_prefix"));
    assert_eq!(prefix.default_scope, SettingScope::Worktree);
    assert_eq!(
        prefix.default_value,
        Some(crate::worktree::WORKTREE_BRANCH_PREFIX)
    );
}

#[test]
fn settings_registry_json_lists_metadata_without_secret_values() {
    // arrange
    // act
    // assert
    let json = settings_registry_json().expect("settings registry json");
    let value: serde_json::Value =
        serde_json::from_str(&json).expect("settings registry json parses");

    assert_eq!(value["schema_version"], "harness-settings-registry-v1");
    assert_eq!(
        usize::try_from(value["setting_count"].as_u64().expect("setting_count")).unwrap_or_abort(),
        settings_registry().len()
    );

    let settings = value["settings"].as_array().expect("settings array");
    assert_eq!(settings.len(), settings_registry().len());

    let mut saw_secret = false;
    let mut saw_metadata_only = false;
    for entry in settings {
        let object = entry.as_object().expect("setting object");
        assert!(object.contains_key("setting_id"));
        assert!(object.contains_key("schema_id"));
        assert!(object.contains_key("surface"));
        assert!(object.contains_key("sensitivity"));
        assert!(object.contains_key("metadata_only"));
        assert!(
            !object.contains_key("default_value"),
            "registry JSON must not emit default values (avoids secret leakage)"
        );
        if object["sensitivity"] == "secret" {
            saw_secret = true;
        }
        if object["metadata_only"] == true {
            saw_metadata_only = true;
        }
    }
    assert!(saw_secret, "expected at least one secret-marked setting");
    assert!(
        saw_metadata_only,
        "expected at least one metadata-only setting"
    );
    assert!(
        !json.contains("sk-"),
        "registry JSON must not contain secret-looking values"
    );
}

#[test]
fn every_registry_entry_has_merge_strategy_and_mutability() {
    // arrange
    // act
    // assert
    for entry in settings_registry() {
        match entry.merge_strategy {
            SettingMergeStrategy::Replace | SettingMergeStrategy::DeepMergeMap => {}
        }
        match entry.mutability {
            SettingMutability::ReadOnly | SettingMutability::Editable => {}
        }
        if entry.is_secret() {
            assert!(
                !entry.is_editable(),
                "secret setting {} must not be editable",
                entry.setting_id
            );
            assert_eq!(entry.mutability, SettingMutability::ReadOnly);
        }
    }
}

#[test]
fn expanded_high_value_keys_are_registered() {
    // arrange
    // act
    // assert
    let ids: BTreeSet<&str> = settings_registry()
        .iter()
        .map(|entry| entry.setting_id.as_str())
        .collect();
    for id in [
        "lsp",
        "disabled_providers",
        "enabled_providers",
        "shell",
        "logging",
        "ui",
        "permission.read",
        "permission.external_directory",
        "permission.doom_loop",
        "permission.shell_allowlist",
        "runtime.compaction.keep_recent_tokens",
        "runtime.compaction.fallback_input_tokens",
        "runtime.compaction.auto_retry_overflow",
    ] {
        assert!(ids.contains(id), "expected expanded key {id} in registry");
    }
    assert!(
        settings_registry().len() >= 37,
        "registry should grow with high-value expansion"
    );
}

#[test]
fn map_settings_use_deep_merge_strategy() {
    // arrange
    // act
    // assert
    for id in [
        "agent",
        "provider",
        "mcp",
        "skills",
        "model_profile",
        "keybinds",
        "ui",
    ] {
        let entry = setting_definition(id).unwrap_or_else(|| panic!("missing {id}"));
        assert_eq!(
            entry.merge_strategy,
            SettingMergeStrategy::DeepMergeMap,
            "{id} should deep-merge maps"
        );
    }
}

#[test]
fn settings_registry_json_includes_merge_and_mutability() {
    // arrange
    // act
    // assert
    let json = settings_registry_json().expect("settings registry json");
    let value: serde_json::Value =
        serde_json::from_str(&json).expect("settings registry json parses");
    let settings = value["settings"].as_array().expect("settings array");
    let mut saw_editable = false;
    let mut saw_deep_merge = false;
    for entry in settings {
        let object = entry.as_object().expect("setting object");
        assert!(object.contains_key("merge_strategy"));
        assert!(object.contains_key("mutability"));
        if object["mutability"] == "editable" {
            saw_editable = true;
        }
        if object["merge_strategy"] == "deep_merge_map" {
            saw_deep_merge = true;
        }
    }
    assert!(saw_editable);
    assert!(saw_deep_merge);
}

#[test]
fn provider_api_key_is_read_only_secret() {
    // arrange
    // act
    // assert
    let entry = setting_definition("provider.apiKey").expect("provider.apiKey");
    assert_eq!(entry.mutability, SettingMutability::ReadOnly);
    assert!(!entry.is_editable());
}

#[test]
fn hashline_edit_is_editable_public_scalar() {
    // arrange
    // act
    // assert
    let entry = setting_definition("hashline_edit").expect("hashline_edit");
    assert_eq!(entry.surface, SettingSurface::Runtime);
    assert_eq!(entry.sensitivity, SettingSensitivity::Public);
    assert_eq!(entry.mutability, SettingMutability::Editable);
    assert_eq!(entry.merge_strategy, SettingMergeStrategy::Replace);
    assert!(entry.is_editable());
    assert_eq!(entry.default_value, Some("true"));
}

#[test]
fn settings_registry_summary_counts_composition() {
    // arrange
    // act
    // assert
    // Given the static settings registry
    // When summarizing composition for operator surfaces
    // Then total equals surface/mutability partitions and one_line is stable
    let summary = summarize_settings_registry();
    assert_eq!(summary.total, settings_registry().len());
    assert_eq!(summary.runtime + summary.tui, summary.total);
    assert_eq!(summary.editable + summary.read_only, summary.total);
    assert!(summary.has_editable());
    assert!(summary.secret > 0);
    assert!(summary.with_default > 0);
    assert!(summary.metadata_only > 0);
    assert!(summary.one_line().starts_with("settings registry: "));
    assert!(summary.one_line().contains("editable="));
}

#[test]
fn resolve_setting_id_applies_compat_migrations() {
    // arrange
    // act
    // assert
    // Given: legacy camelCase and kebab hashline ids
    // When: resolving through compat migrations
    // Then: both map to hashline_edit; unknown stays None
    assert_eq!(resolve_setting_id("hashline_edit"), Some("hashline_edit"));
    assert_eq!(resolve_setting_id("hashlineEdit"), Some("hashline_edit"));
    assert_eq!(resolve_setting_id("hashline-edit"), Some("hashline_edit"));
    assert_eq!(resolve_setting_id("not.a.setting"), None);
    assert!(!settings_compat_migrations().is_empty());
    assert!(settings_compat_migrations()
        .iter()
        .any(|migration| migration.legacy_id == "hashlineEdit"
            && migration.canonical_id == "hashline_edit"));
}

#[test]
fn explain_setting_flags_secret_settings_without_value_leakage() {
    // arrange — the registered secret setting
    let entry = setting_definition("provider.apiKey").expect("provider.apiKey registered");
    assert_eq!(entry.sensitivity, SettingSensitivity::Secret);

    // act
    let explanation = explain_setting("provider.apiKey").expect("provider.apiKey explained");

    // assert — secret sensitivity surfaces; the record shape carries no value field
    assert_eq!(explanation.setting_id, "provider.apiKey");
    assert_eq!(explanation.sensitivity, "secret");
    assert!(!explanation.has_default);
    assert!(explanation.default_value.is_none());
}

#[test]
fn explain_setting_covers_writable_and_worktree_scopes() {
    // arrange
    // act
    // assert
    // Given: writable runtime scalar + worktree metadata-only defaults
    // When: explaining each setting
    // Then: surface/scope/merge/write flags and defaults are bound without secrets
    let hashline = explain_setting("hashline_edit").expect("hashline_edit");
    assert_eq!(hashline.setting_id, "hashline_edit");
    assert_eq!(hashline.surface, "runtime");
    assert_eq!(hashline.default_scope, "project");
    assert_eq!(hashline.merge_strategy, "replace");
    assert_eq!(hashline.mutability, "editable");
    assert!(!hashline.metadata_only);
    assert!(hashline.project_write_supported);
    assert_eq!(hashline.default_value.as_deref(), Some("true"));
    assert!(hashline.one_line().contains("write=true"));

    let legacy = explain_setting("hashlineEdit").expect("legacy hashlineEdit");
    assert_eq!(legacy.setting_id, "hashline_edit");
    assert_eq!(legacy.resolved_from_legacy.as_deref(), Some("hashlineEdit"));
    assert!(legacy.project_write_supported);

    let relative = explain_setting("worktree.relative_base").expect("worktree.relative_base");
    assert_eq!(relative.default_scope, "worktree");
    assert!(relative.metadata_only);
    assert!(!relative.project_write_supported);
    assert_eq!(
        relative.default_value.as_deref(),
        Some(crate::worktree::DEFAULT_WORKTREE_RELATIVE_BASE)
    );

    let secret = explain_setting("provider.apiKey").expect("provider.apiKey");
    assert_eq!(secret.sensitivity, "secret");
    assert!(secret.default_value.is_none());
    assert!(!secret.project_write_supported);

    assert!(explain_setting("missing.setting").is_none());
}
