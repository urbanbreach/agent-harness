//! Voice/dictation absence contract — Task 8.
//!
//! Verifies that no voice/dictation/STT/Whisper public surface, dependency,
//! capability row, config key, help entry, palette command, or TUI source
//! reference remains in the codebase. Old voice config keys must fail clearly.

#![allow(clippy::panic, reason = "absence contract tests use fail-fast asserts")]

mod common;

use common::repo_root;
use harness::UnwrapOrAbort;
use serde_json::Value;
use std::path::{Path, PathBuf};

const VOICE_TERMS: &[&str] = &[
    "voice_affordances",
    "VoiceCommand",
    "VoiceCapture",
    "VoiceStt",
    "VoicePipeline",
    "VoiceConfig",
    "VoiceToggle",
    "VoiceStop",
    "EnableVoiceMode",
    "SetVoiceCaptureMode",
    "SetVoiceSttLanguage",
    "ToggleVoiceInput",
    "ToggleVoiceOutput",
    "VoiceServiceUnavailable",
    "voice_capture_mode",
    "voice_stt_language",
    "VOICE_CAPTURE_MODE_CHOICES",
    "VOICE_STT_LANGUAGE_CHOICES",
];

const WHISPER_TERMS: &[&str] = &["whisper", "Whisper"];

const ALLOWED_VOICE_FILES: &[&str] = &[
    "docs/scope-removal-ledger.v1.json",
    "crates/harness/tests/voice_absence_test.rs",
    "crates/harness/tests/capability_inventory_contract_test.rs",
    "crates/harness/tests/core_subsystem_disposition_test.rs",
    "scripts/parity_task_qa.py",
    "scripts/check-parity-reference-crosswalk.py",
    "grok-build-parity-parallel-execution.md",
    "docs/grok-build-parity-loop-contract.md",
    "docs/grok-reference-interaction-inventory.v1.json",
];

fn collect_source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files_recursive(root, &mut files);
    files
}

fn collect_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(
                name,
                "target"
                    | ".git"
                    | "sessions"
                    | "artifacts"
                    | ".harness"
                    | ".gnhf"
                    | ".sisyphus"
                    | ".omx"
                    | ".omo"
                    | ".codex"
                    | "inspirations"
                    | "node_modules"
                    | "ATTEMPT"
            ) {
                continue;
            }
            collect_files_recursive(&path, files);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if matches!(ext, "rs" | "toml" | "json" | "jsonc" | "md") {
                files.push(path);
            }
        }
    }
}

fn is_allowed_file(root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    ALLOWED_VOICE_FILES
        .iter()
        .any(|allowed| rel_str == *allowed)
}

fn check_no_voice_in_source(root: &Path) {
    let files = collect_source_files(root);
    let mut violations = Vec::new();
    for file in &files {
        if is_allowed_file(root, file) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(file) else {
            continue;
        };
        for term in VOICE_TERMS {
            if content.contains(term) {
                let rel = file.strip_prefix(root).unwrap_or(file);
                violations.push(format!("{}: found `{}`", rel.display(), term));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "voice surface found in source files:\n{}",
        violations.join("\n")
    );
}

fn check_no_whisper_in_source(root: &Path) {
    let files = collect_source_files(root);
    let mut violations = Vec::new();
    for file in &files {
        if is_allowed_file(root, file) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(file) else {
            continue;
        };
        let lower = content.to_lowercase();
        for term in WHISPER_TERMS {
            if lower.contains(term) {
                let rel = file.strip_prefix(root).unwrap_or(file);
                violations.push(format!("{}: found `{}`", rel.display(), term));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "whisper/STT reference found in source files:\n{}",
        violations.join("\n")
    );
}

fn check_no_voice_in_capability_inventory(root: &Path) {
    let path = root.join("docs/reference/capability-inventory.v1.json");
    let raw = std::fs::read_to_string(&path).unwrap_or_abort();
    let doc: Value = serde_json::from_str(&raw).unwrap_or_abort();
    let rows = doc["capabilities"].as_array().unwrap_or_abort();
    for row in rows {
        let cap_id = row["capability_id"].as_str().unwrap_or_abort();
        assert!(
            !cap_id.contains("voice"),
            "voice capability row still present: {cap_id}"
        );
    }
}

fn check_no_voice_in_cargo_manifests(root: &Path) {
    let manifest_paths = [
        root.join("Cargo.toml"),
        root.join("crates/harness/Cargo.toml"),
        root.join("crates/harness-core/Cargo.toml"),
        root.join("crates/harness-providers/Cargo.toml"),
        root.join("crates/harness-tools/Cargo.toml"),
        root.join("crates/harness-tui/Cargo.toml"),
        root.join("crates/harness-testkit/Cargo.toml"),
    ];
    for path in &manifest_paths {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let lower = content.to_lowercase();
        for term in WHISPER_TERMS {
            assert!(
                !lower.contains(term),
                "whisper dependency found in {}: {}",
                path.display(),
                term
            );
        }
        assert!(
            !lower.contains("voice"),
            "voice dependency found in {}",
            path.display()
        );
        assert!(
            !lower.contains("dictation"),
            "dictation dependency found in {}",
            path.display()
        );
        assert!(
            !lower.contains("speech-to-text") && !lower.contains("speech_to_text"),
            "STT dependency found in {}",
            path.display()
        );
    }
}

fn check_no_voice_in_config_examples(root: &Path) {
    let config_paths = [
        root.join("configs/harness.example.jsonc"),
        root.join("configs/tui.example.jsonc"),
        root.join("configs/config.json"),
        root.join("configs/tui.json"),
    ];
    for path in &config_paths {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let lower = content.to_lowercase();
        assert!(
            !lower.contains("voice"),
            "voice config key found in {}",
            path.display()
        );
        assert!(
            !lower.contains("dictation"),
            "dictation config key found in {}",
            path.display()
        );
    }
}

fn check_no_voice_in_generated_schema(root: &Path) {
    let schema_path = root.join("configs/config.json");
    let Ok(content) = std::fs::read_to_string(&schema_path) else {
        return;
    };
    let lower = content.to_lowercase();
    assert!(
        !lower.contains("voice"),
        "voice key found in generated config schema"
    );
    let tui_schema_path = root.join("configs/tui.json");
    let Ok(tui_content) = std::fs::read_to_string(&tui_schema_path) else {
        return;
    };
    let tui_lower = tui_content.to_lowercase();
    assert!(
        !tui_lower.contains("voice"),
        "voice key found in generated TUI schema"
    );
}

fn check_old_voice_config_key_rejected(root: &Path) {
    let schema_path = root.join("configs/config.json");
    let raw = std::fs::read_to_string(&schema_path).unwrap_or_abort();
    let schema: Value = serde_json::from_str(&raw).unwrap_or_abort();

    let properties = schema["properties"].as_object().unwrap_or_abort();
    assert!(
        !properties.contains_key("voice_capture_mode"),
        "voice_capture_mode must not be a recognized config key"
    );
    assert!(
        !properties.contains_key("voice_stt_language"),
        "voice_stt_language must not be a recognized config key"
    );

    let additional = schema.get("additionalProperties");
    assert!(
        additional.is_some_and(|v| v == false),
        "PublicRuntimeConfig must reject unknown keys (additionalProperties must be false)"
    );
}

#[test]
fn voice_absent_from_source() {
    // arrange
    let root = repo_root();
    // act
    // assert
    check_no_voice_in_source(&root);
}

#[test]
fn whisper_absent_from_source() {
    // arrange
    let root = repo_root();
    // act
    // assert
    check_no_whisper_in_source(&root);
}

#[test]
fn voice_absent_from_capability_inventory() {
    // arrange
    let root = repo_root();
    // act
    // assert
    check_no_voice_in_capability_inventory(&root);
}

#[test]
fn voice_absent_from_cargo_manifests() {
    // arrange
    let root = repo_root();
    // act
    // assert
    check_no_voice_in_cargo_manifests(&root);
}

#[test]
fn voice_absent_from_config_examples() {
    // arrange
    let root = repo_root();
    // act
    // assert
    check_no_voice_in_config_examples(&root);
}

#[test]
fn voice_absent_from_generated_schema() {
    // arrange
    let root = repo_root();
    // act
    // assert
    check_no_voice_in_generated_schema(&root);
}

#[test]
fn old_voice_config_key_is_rejected_by_serde_deny_unknown_fields() {
    // arrange
    let root = repo_root();
    // act
    // assert
    check_old_voice_config_key_rejected(&root);
}

#[test]
fn voice_absent_from_tui_leaf_actions() {
    // arrange
    let root = repo_root();
    let group_e = root.join("crates/harness-tui/src/leaf_actions/group_e_media.rs");
    // act
    let content = std::fs::read_to_string(&group_e).unwrap_or_abort();
    // assert
    assert!(
        !content.contains("voice"),
        "voice reference still in group_e_media.rs"
    );
    assert!(
        !content.contains("Voice"),
        "Voice reference still in group_e_media.rs"
    );
}

#[test]
fn voice_absent_from_tui_cross_group_ownership_test() {
    // arrange
    let root = repo_root();
    let test_file = root.join("crates/harness-tui/tests/cross_group_ownership_test.rs");
    // act
    let content = std::fs::read_to_string(&test_file).unwrap_or_abort();
    // assert
    assert!(
        !content.contains("voice_affordances"),
        "tui.voice_affordances still in cross-group ownership test"
    );
}
