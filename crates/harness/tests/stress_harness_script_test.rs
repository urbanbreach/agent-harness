use std::fs;

mod common;

use common::repo_root;

fn stress_script_body() -> String {
    fs::read_to_string(repo_root().join("scripts/stress-harness.sh")).expect("read stress script")
}

#[test]
fn stress_harness_script_offline_mode_records_expected_stage_contract() {
    let script = stress_script_body();

    assert!(script.contains("stage_config_validate"));
    assert!(script.contains("stage_prompt_mock_smoke"));
    assert!(script.contains("stage_run_golden_path"));
    assert!(script.contains("stage_run_golden_path_interactive"));
    assert!(script.contains("if [[ \"$mode\" == \"offline\" || \"$mode\" == \"all\" ]]; then"));
    assert!(script
        .contains("record_stage_result prompt_mock_smoke \"$stage_ok\" \"offline_prompt_path\""));
    assert!(
        script.contains("record_stage_result run_golden_path \"$stage_ok\" \"offline_run_path\"")
    );
    assert!(script.contains("record_stage_result run_golden_path_interactive \"$stage_ok\" \"interactive_permission_path\""));
    assert!(script.contains("verify_patterns \"$events_path\" \"$LAST_STAGE_VERIFICATION_PATH\" '\"event_type\":\"task_completed\"' 'Hello world'"));
    assert!(script.contains("verify_patterns \"$events_path\" \"$LAST_STAGE_VERIFICATION_PATH\" '\"event_type\":\"tool_call_finished\"' '\"status\":\"succeeded\"' '\"event_type\":\"run_finished\"'"));
}

#[test]
fn stress_harness_script_reports_missing_option_values_cleanly() {
    let script = stress_script_body();

    assert!(script.contains("require_option_value()"));
    assert!(script.contains("printf 'Missing value for %s\\n' \"$flag\" >&2"));
    assert!(script.contains("exit 2"));
    for flag in ["--mode", "--artifact-dir", "--config", "--harness-bin"] {
        assert!(
            script.contains("require_option_value \"$1\" \"${2-}\"\n"),
            "{flag} should call require_option_value before reading its value"
        );
    }
}

#[test]
fn stress_harness_script_accepts_relative_artifact_dir_with_missing_parent() {
    let script = stress_script_body();

    assert!(script.contains("artifact_root=\"$(abspath \"$artifact_root\")\""));
    assert!(script.contains("mkdir -p \"$artifact_root/stages\" \"$artifact_root/sessions\""));
    assert!(script.contains("summary_path=\"${artifact_root}/summary.txt\""));
    assert!(script.contains("stage_dir=\"$(stage_dir_for prompt_mock_smoke)\""));
    assert!(script.contains("mkdir -p \"$stage_dir\""));
}
