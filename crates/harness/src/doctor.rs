use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::Args;
use harness_core::agent_catalog::{
    resolve_agent_catalog, SHIPPED_CATEGORY_ROUTES, SHIPPED_PRIMARY_PROFILES, SHIPPED_SUBAGENTS,
};
use harness_core::auth::{CredentialStore, StoredCredentialKind};
use harness_core::config::{
    resolve_model_selection, AgentMode, HarnessConfig, McpServerConfig, PermissionMode,
    ProviderConfig, ShellAllowlistMode,
};
use harness_core::coord::{formatter_status, FormatterStatus, RealFormatterDiscovery};
use harness_core::extension_manifest::EXTENSION_MANIFEST_V1_SCHEMA_VERSION;
use harness_tools::{
    coordinator_registry_with_mcp_and_editing, native_tool_catalog_entries,
    EditingToolSurfaceConfig,
};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::runtime::Runtime;

use crate::auth_cmd;
use crate::readiness::ast_grep_adapter_readiness;
use crate::{CliDeps, CliIo};

#[path = "doctor_metadata.rs"]
mod doctor_metadata;
use self::doctor_metadata::{attach_doctor_model_metadata, skill_readiness_metadata};
mod checks;
const REQUIRED_PRIMARY_AGENTS: [&str; 2] = ["build", "plan"];
const REQUIRED_SUBAGENTS: [&str; 2] = ["explore", "general"];
const REQUIRED_CATEGORY_ROUTES: [&str; 8] = [
    "visual-engineering",
    "artistry",
    "ultrabrain",
    "deep",
    "quick",
    "unspecified-low",
    "unspecified-high",
    "writing",
];
const BUILD_TOOLS: [&str; 5] = [
    "todowrite",
    "task",
    "background_output",
    "plan_enter",
    "edit",
];
const PLAN_TOOLS: [&str; 4] = ["todowrite", "task", "background_output", "plan_exit"];
#[derive(Debug, Args, Clone, Default)]
pub(crate) struct DoctorCommand {
    /// Emit machine-readable JSON instead of text.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl CheckStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
        }
    }
}

#[derive(Debug, Serialize)]
struct DoctorCheck {
    name: String,
    status: CheckStatus,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    config: String,
    no_network_probes: bool,
    provider_execution_proof: bool,
    readiness_scope: &'static str,
    checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    fn has_failures(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == CheckStatus::Fail)
    }

    fn status_counts(&self) -> (usize, usize, usize) {
        let mut passes = 0;
        let mut warnings = 0;
        let mut failures = 0;
        for check in &self.checks {
            match check.status {
                CheckStatus::Pass => passes += 1,
                CheckStatus::Warn => warnings += 1,
                CheckStatus::Fail => failures += 1,
            }
        }
        (passes, warnings, failures)
    }
}

pub(crate) fn execute_with_io(
    command: DoctorCommand,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    let config_context = match deps.config_load_context() {
        Ok(context) => context,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "doctor failed: failed to resolve config context: {err}"
            );
            return 2;
        }
    };

    let Some(loaded) = (match harness_core::config::load_resolved_config_with_context(
        config_path.as_deref(),
        &config_context,
    ) {
        Ok(loaded) => loaded,
        Err(err) => {
            let _ = writeln!(io.stderr, "doctor failed: {err}");
            return 1;
        }
    }) else {
        let _ = writeln!(
            io.stderr,
            "doctor failed: no config file found; pass --config <path>, create ./harness.jsonc or ./harness.json, or start from configs/harness.example.jsonc"
        );
        return 2;
    };

    let config_display = loaded.path_display();
    let mut config = loaded.config;
    config.apply_session_dir_override(session_dir);

    let report = build_report(
        config_display,
        &config,
        &config_context.discovery.current_dir,
        &|name| deps.env_var_is_set(name),
        CredentialStore::from_lookup(&|name| deps.env_var_value(name)),
    );
    if command.json {
        match serde_json::to_string_pretty(&report) {
            Ok(json) => {
                let _ = writeln!(io.stdout, "{json}");
            }
            Err(err) => {
                let _ = writeln!(io.stderr, "doctor failed to render JSON: {err}");
                return 1;
            }
        }
    } else {
        checks::print_text_report(&report, io.stdout);
    }

    if report.has_failures() {
        1
    } else {
        0
    }
}

pub(crate) fn support_report_json(
    config_display: String,
    config: &HarnessConfig,
    workspace_root: &Path,
    env_var_is_set: &dyn Fn(&str) -> bool,
) -> Value {
    let report = build_report(
        config_display.clone(),
        config,
        workspace_root,
        env_var_is_set,
        CredentialStore::from_env(),
    );
    let mut value = serde_json::to_value(report).unwrap_or_else(|err| {
        json!({
            "config": config_display,
            "checks": [],
            "serialization_error": err.to_string(),
        })
    });
    if let Some(object) = value.as_object_mut() {
        object.insert("no_network_probes".to_string(), json!(true));
    }
    value
}

fn build_report(
    config_display: String,
    config: &HarnessConfig,
    workspace_root: &Path,
    env_var_is_set: &dyn Fn(&str) -> bool,
    credential_store: Option<CredentialStore>,
) -> DoctorReport {
    let checks = vec![
        checks::check_provider_catalog(config),
        checks::check_provider_credentials(config, env_var_is_set, &credential_store),
        checks::check_model_references(config),
        checks::check_shipped_profiles(config),
        checks::check_category_routes(config),
        checks::check_resolved_routes(config, workspace_root),
        checks::check_profile_tools(config),
        checks::check_native_tool_catalog(config),
        checks::check_extension_roadmap_readiness(config),
        checks::check_permissions(config),
        checks::check_session_dir(&config.paths.session_dir),
        checks::check_mcp(config),
        checks::check_formatters(config, workspace_root),
    ];

    DoctorReport {
        config: config_display,
        no_network_probes: true,
        provider_execution_proof: false,
        readiness_scope: "local_readiness_only",
        checks,
    }
}
