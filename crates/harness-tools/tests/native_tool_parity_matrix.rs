use std::collections::BTreeSet;

use harness_core::config::ShellAllowlist;
use harness_tools::{
    canonical_tool_id_for, coordinator_registry, native_and_alias_tool_ids,
    native_tool_parity_matrix, NativeToolMigrationStatus, NativeToolParityEntry,
    NativeToolPermissionClass, NativeToolProviderExposure,
};

#[test]
fn native_tool_parity_matrix_is_complete() {
    let expected = [
        NativeToolParityEntry {
            canonical_id: "user.question",
            aliases: &["question"],
            permission_class: NativeToolPermissionClass::UserInput,
            provider_exposure: NativeToolProviderExposure::AliasOnly,
            migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
        },
        NativeToolParityEntry {
            canonical_id: "tool.invalid",
            aliases: &["invalid"],
            permission_class: NativeToolPermissionClass::ControlPlane,
            provider_exposure: NativeToolProviderExposure::AliasOnly,
            migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
        },
        NativeToolParityEntry {
            canonical_id: "fs.write",
            aliases: &["write"],
            permission_class: NativeToolPermissionClass::WorkspaceWrite,
            provider_exposure: NativeToolProviderExposure::AliasOnly,
            migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
        },
        NativeToolParityEntry {
            canonical_id: "web.fetch",
            aliases: &["webfetch"],
            permission_class: NativeToolPermissionClass::Network,
            provider_exposure: NativeToolProviderExposure::AliasOnly,
            migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
        },
        NativeToolParityEntry {
            canonical_id: "todo.write",
            aliases: &["todowrite"],
            permission_class: NativeToolPermissionClass::ControlPlane,
            provider_exposure: NativeToolProviderExposure::AliasOnly,
            migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
        },
        NativeToolParityEntry {
            canonical_id: "todo.read",
            aliases: &["todoread"],
            permission_class: NativeToolPermissionClass::ControlPlane,
            provider_exposure: NativeToolProviderExposure::AliasOnly,
            migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
        },
        NativeToolParityEntry {
            canonical_id: "skill.load",
            aliases: &["skill"],
            permission_class: NativeToolPermissionClass::ControlPlane,
            provider_exposure: NativeToolProviderExposure::AliasOnly,
            migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
        },
        NativeToolParityEntry {
            canonical_id: "search.web",
            aliases: &["websearch"],
            permission_class: NativeToolPermissionClass::Network,
            provider_exposure: NativeToolProviderExposure::AliasOnly,
            migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
        },
        NativeToolParityEntry {
            canonical_id: "search.code",
            aliases: &["codesearch"],
            permission_class: NativeToolPermissionClass::Network,
            provider_exposure: NativeToolProviderExposure::AliasOnly,
            migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
        },
        NativeToolParityEntry {
            canonical_id: "code.lsp",
            aliases: &["lsp"],
            permission_class: NativeToolPermissionClass::ReadOnly,
            provider_exposure: NativeToolProviderExposure::AliasOnly,
            migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
        },
        NativeToolParityEntry {
            canonical_id: "code.lsp.rename",
            aliases: &[],
            permission_class: NativeToolPermissionClass::WorkspaceWrite,
            provider_exposure: NativeToolProviderExposure::ExplicitOptIn,
            migration_status: NativeToolMigrationStatus::NativeStable,
        },
        NativeToolParityEntry {
            canonical_id: "tool.batch",
            aliases: &["batch"],
            permission_class: NativeToolPermissionClass::ControlPlane,
            provider_exposure: NativeToolProviderExposure::AliasOnly,
            migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
        },
        NativeToolParityEntry {
            canonical_id: "plan.exit",
            aliases: &["plan_exit"],
            permission_class: NativeToolPermissionClass::ControlPlane,
            provider_exposure: NativeToolProviderExposure::AliasOnly,
            migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
        },
        NativeToolParityEntry {
            canonical_id: "agent.spawn",
            aliases: &["task"],
            permission_class: NativeToolPermissionClass::AgentSpawn,
            provider_exposure: NativeToolProviderExposure::ExplicitOptIn,
            migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
        },
        NativeToolParityEntry {
            canonical_id: "shell.run",
            aliases: &["bash"],
            permission_class: NativeToolPermissionClass::Shell,
            provider_exposure: NativeToolProviderExposure::ExplicitOptIn,
            migration_status: NativeToolMigrationStatus::NativeStable,
        },
        NativeToolParityEntry {
            canonical_id: "fs.read",
            aliases: &["read"],
            permission_class: NativeToolPermissionClass::ReadOnly,
            provider_exposure: NativeToolProviderExposure::ExplicitOptIn,
            migration_status: NativeToolMigrationStatus::NativeStable,
        },
        NativeToolParityEntry {
            canonical_id: "fs.glob",
            aliases: &["glob"],
            permission_class: NativeToolPermissionClass::ReadOnly,
            provider_exposure: NativeToolProviderExposure::ExplicitOptIn,
            migration_status: NativeToolMigrationStatus::NativeStable,
        },
        NativeToolParityEntry {
            canonical_id: "fs.grep",
            aliases: &["grep"],
            permission_class: NativeToolPermissionClass::ReadOnly,
            provider_exposure: NativeToolProviderExposure::ExplicitOptIn,
            migration_status: NativeToolMigrationStatus::NativeStable,
        },
    ];

    assert_eq!(native_tool_parity_matrix(), expected.as_slice());

    let canonical_ids = native_tool_parity_matrix()
        .iter()
        .map(|entry| entry.canonical_id)
        .collect::<Vec<_>>();
    assert_eq!(
        canonical_ids.len(),
        canonical_ids.iter().collect::<BTreeSet<_>>().len()
    );

    let alias_ids = native_tool_parity_matrix()
        .iter()
        .flat_map(|entry| entry.aliases.iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(
        alias_ids.len(),
        alias_ids.iter().collect::<BTreeSet<_>>().len()
    );

    let expected_ids = expected
        .iter()
        .flat_map(NativeToolParityEntry::all_ids)
        .collect::<Vec<_>>();
    assert_eq!(native_and_alias_tool_ids(), expected_ids);
    assert_eq!(
        expected_ids.len(),
        expected_ids.iter().collect::<BTreeSet<_>>().len()
    );

    for entry in native_tool_parity_matrix() {
        assert_eq!(
            canonical_tool_id_for(entry.canonical_id),
            Some(entry.canonical_id)
        );
        for alias in entry.aliases {
            assert_eq!(canonical_tool_id_for(alias), Some(entry.canonical_id));
        }
    }

    let registry = coordinator_registry(ShellAllowlist::default());
    for entry in native_tool_parity_matrix() {
        assert_eq!(
            registry.get(entry.canonical_id).is_some(),
            entry.registers_canonical_id(),
            "canonical registry presence drifted for {}",
            entry.canonical_id
        );

        for alias in entry.aliases {
            assert_eq!(
                registry.get(alias).is_some(),
                entry.exposes_aliases(),
                "alias registry presence drifted for {alias} -> {}",
                entry.canonical_id
            );
        }
    }
}
