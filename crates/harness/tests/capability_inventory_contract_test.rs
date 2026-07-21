//! Fail-closed contract for docs/capability-inventory.v1.json (A-CAPABILITIES / §4.5).
//!
//! The inventory is the machine-readable capability floor for Grok Build parity.
//! Missing file, missing families, invalid enums, or `pass` without a real backend
//! owner path must fail the test.

use harness::UnwrapOrAbort;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

mod common;

use common::repo_root;

const INVENTORY_REL: &str = "docs/capability-inventory.v1.json";
const SCHEMA_VERSION: &str = "harness-capability-inventory-v1";

const ALLOWED_STATUSES: &[&str] = &["incomplete", "blocked", "pass", "diverged"];
const ALLOWED_DISPOSITIONS: &[&str] = &[
    "implement",
    "rework",
    "retain",
    "placeholder-excluded",
    "user-divergence-needed",
];
const ALLOWED_SIDE_EFFECTS: &[&str] = &[
    "none",
    "filesystem",
    "git",
    "network",
    "process",
    "session",
    "config",
    "provider",
    "mixed",
];

/// §4.5 capability family floor (must each have ≥1 inventory row).
const REQUIRED_FAMILIES: &[&str] = &[
    "workspace-worktree-isolation-vcs",
    "sessions-memory-persistence-rewind-crash-recovery",
    "agents-background-orchestration-scheduler-queue",
    "integrations-plugins-acp-remote-mcp",
    "providers-identity-auth-update",
    "code-intelligence-terminal-input",
    "tui-shell-composer-transcript-overlays",
    "config-schema-settings",
];

/// Known-closed product surfaces that must remain inventory `pass`.
/// Oracle (ses_089f6a0f9ffecFkOly2z7H1PGL): foundation/unavailable MVPs must NOT be required pass.
const REQUIRED_PASS_IDS: &[&str] = &[
    "worktree.create",
    "config.show.effective",
    "config.sources",
    "config.explain",
    "permission.always_approve_mode",
    "permission.four_option_with_session_grant",
    "orchestration.wait_any",
    "orchestration.wait_all",
    "terminal.clipboard_hyperlink",
    "workspace.folder_trust",
    "memory.durable_product_surface",
    "tui.feedback_help",
    "workspace.cow_worktree_fastpath",
    "sessions.crash_recovery_ux",
    "config.settings_registry",
    "provider.auto_fallback",
    "tui.settings_editor",
    "tui.view_plan",
    "tui.composer",
    "tui.transcript",
    "tui.overlays",
    "tui.home_shell",
    "tui.session_status_dashboard",
    "permission.dock_ui",
    "terminal.capability_presentation",
    "sessions.foreign_import",
    "vcs.edit_attribution",
    "orchestration.foreground_demote_background",
    "sandbox.os_profiles",
];

/// Wave 2 demotions: these rows were overclaimed `pass` (probe-only or missing
/// public surface/consumer) and must stay non-pass until the gap is closed.
const REQUIRED_NON_PASS_IDS: &[&str] = &[
    "worktree.list_select_cleanup",
    "vcs.jujutsu",
    "sessions.prompt_rewind_atomic",
    "sessions.prompt_queue_persistence",
    "sessions.mid_turn_interjection",
    "scheduler.cron_recurring",
    "orchestration.multi_agent_team",
    "mcp.oauth_remote_transports",
    "plugins.runtime_lifecycle",
    "acp.agent_mode",
    "remote.workspace_hub",
    "auth.browser_oidc_sso",
    "auth.sleep_wake_credential_refresh",
    "provider.non_openai_protocols",
    "platform.binary_update",
    "code.persistent_graph",
    "sessions.prompt_rewind_projection",
    "plugins.descriptor_manifest",
];
const MAX_ROWS: usize = 200;

fn inventory_path(root: &Path) -> PathBuf {
    root.join(INVENTORY_REL)
}

fn load_inventory(root: &Path) -> Value {
    let path = inventory_path(root);
    assert!(
        path.is_file(),
        "missing required inventory file: {INVENTORY_REL}"
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_abort();
    serde_json::from_str(&raw).unwrap_or_abort()
}

#[allow(clippy::panic, reason = "fail-closed inventory validation helper")]
fn require_str(value: &Value, field: &str, capability_id: &str) -> String {
    value[field]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("capability {capability_id}: missing/non-string field `{field}`"))
}

fn owner_path_exists(root: &Path, owner: &str) -> bool {
    if owner == "none" {
        return false;
    }
    // Accept either a file or directory path under the repo root.
    let path = root.join(owner);
    path.exists()
}

#[test]
fn capability_inventory_file_exists_with_schema_version() {
    // arrange
    let root = repo_root();

    // act
    let doc = load_inventory(&root);

    // assert
    assert_eq!(
        doc["schema_version"].as_str(),
        Some(SCHEMA_VERSION),
        "schema_version must be {SCHEMA_VERSION}"
    );
}

#[test]
fn capability_inventory_covers_section_4_5_families_and_row_shape() {
    // arrange
    let root = repo_root();
    let doc = load_inventory(&root);
    let rows = doc["capabilities"]
        .as_array()
        .unwrap_or_else(|| panic!("capabilities must be a non-empty array"));
    assert!(!rows.is_empty(), "capabilities array must not be empty");
    assert!(
        rows.len() <= MAX_ROWS,
        "capability inventory has {} rows; keep under {MAX_ROWS}",
        rows.len()
    );

    let mut ids = BTreeSet::new();
    let mut families_seen = BTreeSet::new();
    let mut status_counts: BTreeMap<String, usize> = BTreeMap::new();

    // act + assert per-row contract
    for row in rows {
        let capability_id = require_str(row, "capability_id", "<unknown>");
        assert!(
            ids.insert(capability_id.clone()),
            "duplicate capability_id: {capability_id}"
        );

        let family = require_str(row, "family", &capability_id);
        families_seen.insert(family);

        let _surface = require_str(row, "surface", &capability_id);

        let status = require_str(row, "status", &capability_id);
        assert!(
            ALLOWED_STATUSES.contains(&status.as_str()),
            "capability {capability_id}: invalid status `{status}`"
        );
        *status_counts.entry(status.clone()).or_default() += 1;

        let backend_owner = require_str(row, "backend_owner", &capability_id);
        assert!(
            !backend_owner.is_empty(),
            "capability {capability_id}: backend_owner must not be empty"
        );

        // visible_action_id may be null or a non-empty string
        match &row["visible_action_id"] {
            Value::Null => {}
            Value::String(s) => assert!(
                !s.is_empty(),
                "capability {capability_id}: visible_action_id string must be non-empty when present"
            ),
            other => panic!(
                "capability {capability_id}: visible_action_id must be string or null, got {other}"
            ),
        }

        let side_effect = require_str(row, "side_effect_kind", &capability_id);
        assert!(
            ALLOWED_SIDE_EFFECTS.contains(&side_effect.as_str()),
            "capability {capability_id}: invalid side_effect_kind `{side_effect}`"
        );

        let disposition = require_str(row, "disposition", &capability_id);
        assert!(
            ALLOWED_DISPOSITIONS.contains(&disposition.as_str()),
            "capability {capability_id}: invalid disposition `{disposition}`"
        );

        let notes = require_str(row, "notes", &capability_id);
        assert!(
            !notes.is_empty(),
            "capability {capability_id}: notes must be non-empty"
        );

        // Fail-closed: pass requires a real on-disk backend owner path.
        if status == "pass" {
            assert_ne!(
                backend_owner, "none",
                "capability {capability_id}: pass status forbids backend_owner=none"
            );
            assert!(
                owner_path_exists(&root, &backend_owner),
                "capability {capability_id}: pass backend_owner `{backend_owner}` does not exist on disk"
            );
        }
    }

    // assert family floor
    for family in REQUIRED_FAMILIES {
        assert!(
            families_seen.contains(*family),
            "§4.5 family missing from inventory: {family}"
        );
    }

    // assert known closed seeds
    for id in REQUIRED_PASS_IDS {
        assert!(ids.contains(*id), "required pass capability missing: {id}");
        let row = rows
            .iter()
            .find(|r| r["capability_id"].as_str() == Some(*id))
            .unwrap_or_abort();
        assert_eq!(
            row["status"].as_str(),
            Some("pass"),
            "required closed capability {id} must have status=pass"
        );
    }

    // assert known incomplete seeds are not falsely pass
    for id in REQUIRED_NON_PASS_IDS {
        assert!(
            ids.contains(*id),
            "required non-pass capability missing: {id}"
        );
        let row = rows
            .iter()
            .find(|r| r["capability_id"].as_str() == Some(*id))
            .unwrap_or_abort();
        let status = row["status"].as_str().unwrap_or_abort();
        assert_ne!(
            status, "pass",
            "capability {id} must not be marked pass while backend is incomplete"
        );
        assert!(
            status == "incomplete" || status == "blocked" || status == "diverged",
            "capability {id}: expected incomplete|blocked|diverged, got {status}"
        );
    }

    // Keep counts in the failure message for operators.
    let summary: String = status_counts
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ");
    assert!(
        !summary.is_empty(),
        "status summary should not be empty; got: {summary}"
    );
}

#[test]
fn capability_inventory_rejects_pass_without_backend_owner_path() {
    // arrange
    // act
    // assert
    // arrange — structural negative control using the loaded document shape
    let root = repo_root();
    let doc = load_inventory(&root);
    let rows = doc["capabilities"].as_array().unwrap_or_abort();

    // act + assert: every pass row's owner resolves
    let mut pass_count = 0usize;
    for row in rows {
        if row["status"].as_str() != Some("pass") {
            continue;
        }
        pass_count += 1;
        let id = row["capability_id"].as_str().unwrap_or_abort();
        let owner = row["backend_owner"].as_str().unwrap_or_abort();
        assert!(
            owner_path_exists(&root, owner),
            "pass capability {id} owner missing on disk: {owner}"
        );
    }
    assert!(
        pass_count >= REQUIRED_PASS_IDS.len(),
        "expected at least {} pass rows, found {pass_count}",
        REQUIRED_PASS_IDS.len()
    );
}
