//! CLI contract matrix tests for Task 23.
//!
//! Covers happy/error/unknown/conflict/JSON schema paths and persisted effects
//! for config, settings, provider/model selection, doctor, completions, and
//! the model-alias resolver contract from plan §1.1.
//!
//! These tests run in-process via `harness::run()` — no binary spawn, no
//! network calls. They verify CLI exit codes, output formats, and persisted
//! effects on disk.

use harness::CliDeps;
use harness::CliIo;
use harness::ExitOutcome;
use harness_core::config::{is_model_alias, known_model_aliases, resolve_model_alias, ModelAlias};
use harness_providers::UnwrapOrAbort;
use std::fs;
use std::io::Cursor;

fn run_cli(args: &[&str], deps: CliDeps) -> (i32, String, String) {
    let args: Vec<&str> = std::iter::once("harness")
        .chain(args.iter().copied())
        .collect();
    let mut stdin = Cursor::new(Vec::new());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
    let ExitOutcome { code, .. } = harness::run(args, &mut io, deps);
    (
        code,
        String::from_utf8_lossy(&stdout).to_string(),
        String::from_utf8_lossy(&stderr).to_string(),
    )
}

fn run_cli_in_workspace(args: &[&str]) -> (i32, String, String) {
    run_cli(args, CliDeps::real())
}

fn run_cli_in_temp(args: &[&str]) -> (i32, String, String) {
    let temp = tempfile::tempdir().unwrap_or_abort();
    let deps = CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env("HOME", temp.path().to_string_lossy())
        .with_env(
            "XDG_CONFIG_HOME",
            temp.path().join("config").to_string_lossy(),
        );
    run_cli(args, deps)
}

fn write_config(dir: &std::path::Path, config: &str) -> std::path::PathBuf {
    let config_path = dir.join("harness.jsonc");
    fs::write(&config_path, config).unwrap_or_abort();
    config_path
}

include!("cli_contract_matrix/01_config_models_doctor_test.rs");
include!("cli_contract_matrix/02_leaf_sessions_settings_test.rs");
