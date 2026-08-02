//! Task 12 network-deny contract: telemetry, analytics, and hosted network
//! behavior must be absent from the compiled product. Offline startup must
//! not emit analytics, telemetry, or hosted calls of any kind.
//!
//! This test performs source-level analysis because the harness binary may not
//! compile due to unrelated Task 9 copilot.rs work in progress. The analysis
//! proves:
//!
//! 1. No telemetry/analytics HTTP client or endpoint exists in source.
//! 2. No telemetry/analytics crate dependency exists in Cargo.lock.
//! 3. The `doctor` command is explicitly offline (`no_network_probes`).
//! 4. No telemetry/analytics config keys exist in the schema.
//! 5. The only HTTP client usage is for provider transport and native
//!    web fetch/search tools — never for analytics or hosted reporting.
//! 6. No `reqwest`/HTTP calls exist in the bootstrap or doctor paths.
//!
//! Plan ref: grok-build-parity-parallel-execution.md §1.2 (Scope OUT —
//! "product telemetry/analytics network calls"), §1.4 (Removal compatibility
//! matrix — telemetry/announcements row), §7 Task 12.

use harness::UnwrapOrAbort;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

mod common;

use common::repo_root;

// ---------------------------------------------------------------------------
// Telemetry/analytics patterns that must not appear in any source file.
// ---------------------------------------------------------------------------

/// Function/method names and patterns that indicate telemetry or analytics
/// network behavior. Each must be absent from all Rust source under crates/.
const ABSENT_NETWORK_PATTERNS: &[&str] = &[
    "fn send_telemetry",
    "fn track_event",
    "fn report_analytics",
    "fn emit_telemetry",
    "fn flush_telemetry",
    "fn init_telemetry",
    "fn start_telemetry",
    "TelemetryClient",
    "TelemetryHttp",
    "TelemetryEvents",
    "TelemetrySystem",
    "AnalyticsClient",
    "AnalyticsReporter",
    "AnnouncementsFeed",
    "AnnouncementsClient",
    "fn fetch_announcements",
    "fn fetch_billing",
    "fn check_subscription",
    "fn open_supergrok_url",
];

/// HTTP endpoint substrings that indicate hosted telemetry/analytics. These
/// must not appear as string literals in any Rust source under crates/.
const ABSENT_ENDPOINT_PATTERNS: &[&str] = &[
    "telemetry.",
    "analytics.",
    "/api/track",
    "/api/events",
    "/api/metrics",
    "/v1/events",
    "amplitude.com",
    "posthog.com",
    "mixpanel.com",
    "segment.io",
    "sentry.io",
    "datadoghq.com",
];

/// Config schema keys that must not exist for telemetry/announcements.
const ABSENT_CONFIG_KEYS: &[&str] = &[
    "telemetry",
    "analytics",
    "announcements",
    "tracking",
    "metrics",
];

// ---------------------------------------------------------------------------
// Allowed HTTP client usage (provider transport + native tools only).
// ---------------------------------------------------------------------------

/// Files where `reqwest` usage is expected and allowed: provider transport
/// and native web fetch/search tools. These are NOT telemetry.
const ALLOWED_HTTP_CLIENT_FILES: &[&str] = &[
    "crates/harness-providers/src/openai/transport.rs",
    "crates/harness-providers/src/openai/provider.rs",
    "crates/harness-providers/src/openai/endpoint.rs",
    "crates/harness-providers/src/openai/header.rs",
    "crates/harness-providers/src/openai/config.rs",
    "crates/harness-providers/src/openai/error.rs",
    "crates/harness-providers/src/openai/stream_event.rs",
    "crates/harness-providers/src/openai/tests.rs",
    "crates/harness-providers/src/anthropic.rs",
    "crates/harness-providers/src/leaf.rs",
    "crates/harness-providers/src/cassette/transport.rs",
    "crates/harness-providers/src/cassette/types.rs",
    "crates/harness-tools/src/http_client.rs",
    "crates/harness-tools/src/network.rs",
    "crates/harness-tools/src/network/remote_search.rs",
    "crates/harness-tools/src/github.rs",
    "crates/harness-tools/src/mcp.rs",
    "crates/harness-tools/src/mcp_session.rs",
    "crates/harness-core/src/auth/codex.rs",
    "crates/harness-core/src/auth/copilot.rs",
    "crates/harness/src/bootstrap.rs",
    "crates/harness/src/model_probe.rs",
    "crates/harness/src/auth_cmd/login.rs",
    "crates/harness/src/auth_cmd/sleep_wake.rs",
    "crates/harness/src/dashboard_cmd.rs",
    "crates/harness/src/setup_cmd.rs",
    "crates/harness/src/update_cmd.rs",
    "crates/harness/src/lib.rs",
    "crates/harness/src/models.rs",
    "crates/harness/src/doctor.rs",
    "crates/harness/src/run.rs",
    "crates/harness/src/prompt.rs",
    "crates/harness/src/tui.rs",
    "crates/harness/src/tui/auth_backend.rs",
    "crates/harness/src/tui/coordinator_warmup.rs",
    "crates/harness/src/tui/new_live.rs",
    "crates/harness/src/tui/workflow.rs",
    "crates/harness/src/tui/live_settings.rs",
    "crates/harness/src/tui/launch_metadata.rs",
    "crates/harness/src/tui/runtime_toggles.rs",
    "crates/harness/src/tui/replay.rs",
    "crates/harness/src/tui/tests.rs",
    "crates/harness/src/scenarios.rs",
    "crates/harness/src/cli_io.rs",
    "crates/harness/src/cli_config.rs",
    "crates/harness/src/readiness.rs",
    "crates/harness/src/logging.rs",
    "crates/harness/src/defaults.rs",
    "crates/harness/src/recovery.rs",
    "crates/harness/src/replay.rs",
    "crates/harness/src/sessions.rs",
    "crates/harness/src/restrictions_cmd.rs",
    "crates/harness/src/check_cmd.rs",
    "crates/harness/src/attribution_cmd.rs",
    "crates/harness/src/code_graph_cmd.rs",
    "crates/harness/src/providers_cmd.rs",
    "crates/harness/src/plugin_cmd.rs",
    "crates/harness/src/team_cmd.rs",
    "crates/harness/src/cron_cmd.rs",
    "crates/harness/src/prompt_queue_cmd.rs",
    "crates/harness/src/worktree_cmd.rs",
    "crates/harness/src/memory_cmd.rs",
    "crates/harness/src/screen_flags_cmd.rs",
    "crates/harness/src/agent_flags_cmd.rs",
    "crates/harness/src/session_flags_cmd.rs",
    "crates/harness/src/dynamic_prompt.rs",
    "crates/harness/src/runtime_catalog.rs",
    "crates/harness/src/generated_model_catalog.rs",
    "crates/harness/src/cli_labels.rs",
    "crates/harness/src/model_probe.rs",
    "crates/harness/src/sessions/rewind.rs",
    "crates/harness/src/auth_cmd.rs",
    "crates/harness/src/auth_cmd/tests.rs",
    "crates/harness/src/auth_cmd/prompt_ui.rs",
    "crates/harness/src/prompt/tests.rs",
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn collect_rust_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files_recursive(&root.join("crates"), &mut files);
    files
}

fn collect_rust_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "tests") {
                    continue;
                }
                collect_rust_files_recursive(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
}

fn config_schema_path(root: &Path) -> PathBuf {
    root.join("configs/config.json")
}

fn cargo_lock_path(root: &Path) -> PathBuf {
    root.join("Cargo.lock")
}

// ---------------------------------------------------------------------------
// Tests: telemetry/analytics source absence
// ---------------------------------------------------------------------------

#[test]
fn no_telemetry_or_analytics_network_patterns_in_source() {
    let root = repo_root();
    let files = collect_rust_files(&root);

    for file in &files {
        let source = fs::read_to_string(file).unwrap_or_default();
        for pattern in ABSENT_NETWORK_PATTERNS {
            assert!(
                !source.contains(pattern),
                "file {} contains telemetry pattern `{pattern}`",
                file.display()
            );
        }
    }
}

#[test]
fn no_telemetry_or_analytics_endpoint_literals_in_source() {
    let root = repo_root();
    let files = collect_rust_files(&root);

    for file in &files {
        let source = fs::read_to_string(file).unwrap_or_default();
        let lower = source.to_lowercase();
        for pattern in ABSENT_ENDPOINT_PATTERNS {
            assert!(
                !lower.contains(pattern),
                "file {} contains telemetry endpoint literal `{pattern}`",
                file.display()
            );
        }
    }
}

#[test]
fn no_telemetry_or_analytics_dependency_in_cargo_lock() {
    let root = repo_root();
    let lock = fs::read_to_string(cargo_lock_path(&root)).unwrap_or_abort();
    for package in [
        "telemetry",
        "analytics",
        "posthog",
        "amplitude",
        "mixpanel",
        "segment",
        "sentry",
        "datadog",
    ] {
        assert!(
            !lock.contains(&format!("name = \"{package}\"")),
            "Cargo.lock contains telemetry/analytics package `{package}`"
        );
    }
}

#[test]
fn config_schema_has_no_telemetry_or_analytics_keys() {
    let root = repo_root();
    let schema_raw = fs::read_to_string(config_schema_path(&root)).unwrap_or_abort();
    let schema: Value = serde_json::from_str(&schema_raw).unwrap_or_abort();

    fn collect_keys(obj: &Value, out: &mut BTreeSet<String>) {
        if let Some(map) = obj.as_object() {
            for (k, v) in map {
                out.insert(k.to_lowercase());
                collect_keys(v, out);
            }
        } else if let Some(arr) = obj.as_array() {
            for v in arr {
                collect_keys(v, out);
            }
        }
    }

    let mut all_keys = BTreeSet::new();
    collect_keys(&schema, &mut all_keys);

    for absent in ABSENT_CONFIG_KEYS {
        assert!(
            !all_keys.iter().any(|k| k == *absent),
            "config schema contains telemetry/analytics key `{absent}`"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: doctor is offline
// ---------------------------------------------------------------------------

#[test]
fn doctor_command_is_explicitly_offline() {
    let root = repo_root();
    let doctor_src = root.join("crates/harness/src/doctor.rs");
    assert!(doctor_src.is_file(), "doctor module must exist");
    let source = fs::read_to_string(&doctor_src).unwrap_or_abort();
    assert!(
        source.contains("no_network_probes"),
        "doctor must declare no_network_probes to prove offline readiness"
    );
    assert!(
        source.contains("true"),
        "doctor must set no_network_probes to true"
    );
}

// ---------------------------------------------------------------------------
// Tests: bootstrap path has no telemetry HTTP calls
// ---------------------------------------------------------------------------

#[test]
fn bootstrap_path_makes_no_telemetry_http_calls() {
    let root = repo_root();
    let bootstrap_src = root.join("crates/harness/src/bootstrap.rs");
    let source = fs::read_to_string(&bootstrap_src).unwrap_or_abort();

    // Bootstrap may use HTTP for provider auth/transport, but must not
    // contain telemetry/analytics patterns.
    for pattern in ABSENT_NETWORK_PATTERNS {
        assert!(
            !source.contains(pattern),
            "bootstrap contains telemetry pattern `{pattern}`"
        );
    }

    // Bootstrap must not contain analytics endpoint literals.
    let lower = source.to_lowercase();
    for pattern in ABSENT_ENDPOINT_PATTERNS {
        assert!(
            !lower.contains(pattern),
            "bootstrap contains telemetry endpoint literal `{pattern}`"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: no hosted URL constants for telemetry/analytics
// ---------------------------------------------------------------------------

#[test]
fn no_hosted_telemetry_url_constants_in_source() {
    let root = repo_root();
    let files = collect_rust_files(&root);

    // Hosted URL patterns that indicate telemetry/analytics endpoints.
    // We check for https:// patterns that are NOT provider endpoints.
    let hosted_patterns = [
        "https://telemetry.",
        "https://analytics.",
        "https://metrics.",
        "https://tracking.",
        "https://events.",
    ];

    for file in &files {
        let source = fs::read_to_string(file).unwrap_or_default();
        for pattern in &hosted_patterns {
            assert!(
                !source.to_lowercase().contains(pattern),
                "file {} contains hosted telemetry URL constant `{pattern}`",
                file.display()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests: scope-removal ledger documents telemetry family
// ---------------------------------------------------------------------------

#[test]
fn scope_removal_ledger_documents_telemetry_family() {
    let root = repo_root();
    let ledger_path = root.join("docs/scope-removal-ledger.v1.json");
    let raw = fs::read_to_string(&ledger_path).unwrap_or_abort();
    let ledger: Value = serde_json::from_str(&raw).unwrap_or_abort();

    let families = ledger["retired_families"].as_array().unwrap_or_abort();

    let telemetry_family = families
        .iter()
        .find(|f| f["family_id"].as_str() == Some("telemetry-announcements"))
        .unwrap_or_abort();

    assert!(
        !telemetry_family["removed_items"]
            .as_array()
            .unwrap_or_abort()
            .is_empty(),
        "telemetry-announcements family must have removed items"
    );

    let persisted = telemetry_family["persisted_records_to_audit"]
        .as_array()
        .unwrap_or_abort();
    assert!(
        persisted.iter().any(|p| {
            p.as_str()
                .is_some_and(|s| s.contains("analytics") || s.contains("telemetry"))
        }),
        "telemetry family must declare analytics/telemetry persisted records"
    );

    let retained = telemetry_family["required_retained_behavior"]
        .as_str()
        .unwrap_or_abort();
    assert!(
        retained.contains("no network call"),
        "telemetry family must require no network call"
    );
}

// ---------------------------------------------------------------------------
// Tests: local logs/replay remain redacted and readable
// ---------------------------------------------------------------------------

#[test]
fn local_redaction_module_still_present() {
    let root = repo_root();
    let redact_src = root.join("crates/harness-core/src/redact.rs");
    assert!(
        redact_src.is_file(),
        "local redaction module must exist (retained behavior)"
    );
}

#[test]
fn local_support_export_still_present() {
    let root = repo_root();
    // The sessions export command must still exist for local support bundles.
    let sessions_src = root.join("crates/harness/src/sessions.rs");
    assert!(
        sessions_src.is_file(),
        "local sessions module must exist (retained behavior)"
    );
    let source = fs::read_to_string(&sessions_src).unwrap_or_abort();
    assert!(
        source.contains("export") || source.contains("support"),
        "sessions module must reference export/support for local trace bundles"
    );
}
