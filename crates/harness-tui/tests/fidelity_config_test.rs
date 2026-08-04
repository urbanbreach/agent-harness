use harness_tui::capability_matrix::{CapabilityMatrix, well_known_profiles};
use harness_tui::fidelity_config::*;
use harness_tui::theme_family::ThemeChoice;

#[test]
fn defaults_have_current_fidelity_values() {
    let config = FidelityConfig::from_defaults();
    assert_eq!(config.schema_version, "fidelity-v1");
    assert_eq!(config.theme, ThemeChoice::Auto);
    assert_eq!(config.motion, MotionMode::Full);
    assert_eq!(config.notification, NotificationMode::Native);
    assert!(config.inline_images);
    assert!(!config.inline_video);
}

#[test]
fn config_round_trips_through_json() {
    let config = FidelityConfig::from_defaults();
    let json = config.to_json().expect("serialization should succeed");
    let parsed = FidelityConfig::from_json(&json).expect("parse should succeed");
    assert_eq!(parsed, config);
}

#[test]
fn validation_accepts_current_and_rejects_unknown_schema() {
    let valid = FidelityConfig::from_defaults();
    assert_eq!(valid.validate(), Ok(()));
    let mut invalid = valid;
    invalid.schema_version = "fidelity-v9".to_string();
    assert!(matches!(
        invalid.validate(),
        Err(ConfigValidationError::UnknownSchema(_))
    ));
}

#[test]
fn v0_migrates_to_current_contract() {
    let raw =
        r#"{"schema_version":"fidelity-v0","theme":"dark","reduced_motion":true,"graphics":true}"#;
    let config = ConfigMigration::migrate(raw).expect("v0 should migrate");
    assert_eq!(config.schema_version, "fidelity-v1");
    assert_eq!(config.theme, ThemeChoice::Dark);
    assert_eq!(config.motion, MotionMode::Reduced);
    assert!(config.inline_images);
    assert!(!config.inline_video);
    assert_eq!(config.input_mode, InputMode::Auto);
}

#[test]
fn migration_reports_unknown_and_malformed_input() {
    assert!(matches!(
        ConfigMigration::migrate(r#"{"schema_version":"fidelity-v9"}"#),
        Err(MigrationError::UnknownSchema(_))
    ));
    assert!(matches!(
        ConfigMigration::migrate("{"),
        Err(MigrationError::ParseError(_))
    ));
}

#[test]
fn rollback_toggle_sets_have_expected_decisions() {
    let enabled = RollbackToggles::all_enabled();
    let disabled = RollbackToggles::all_disabled("test");
    for feature in [
        "inline_images",
        "inline_video",
        "terminal_title",
        "native_notifications",
        "modern_keyboard",
    ] {
        assert!(enabled.is_enabled(feature));
        assert!(!disabled.is_enabled(feature));
    }
    let risky = RollbackToggles::disable_risky("test");
    assert!(!risky.is_enabled("inline_video"));
    assert!(!risky.is_enabled("native_notifications"));
    assert!(risky.is_enabled("inline_images"));
    assert!(risky.is_enabled("terminal_title"));
}

#[test]
fn rollback_merge_disables_selected_fields() {
    let mut toggles = RollbackToggles::all_enabled();
    toggles.inline_images = RollbackDecision::ForceDisable {
        reason: "test".to_string(),
    };
    toggles.native_notifications = RollbackDecision::ForceDisable {
        reason: "test".to_string(),
    };
    toggles.modern_keyboard = RollbackDecision::ForceDisable {
        reason: "test".to_string(),
    };
    let merged = toggles.merge_with_config(&FidelityConfig::from_defaults());
    assert!(!merged.inline_images);
    assert_eq!(merged.notification, NotificationMode::Bell);
    assert_eq!(merged.input_mode, InputMode::Legacy);
    assert!(!merged.inline_video);
}

#[test]
fn rollback_uses_weakest_capability_profile() {
    let mut profiles = well_known_profiles();
    let dumb = CapabilityMatrix::new(profiles.remove(4).1);
    let dumb_toggles = RollbackToggles::from_capability_matrix(&dumb);
    for feature in [
        "inline_images",
        "inline_video",
        "native_notifications",
        "terminal_title",
        "modern_keyboard",
    ] {
        assert!(!dumb_toggles.is_enabled(feature));
    }
    let wezterm = CapabilityMatrix::new(profiles.remove(0).1);
    let modern = RollbackToggles::from_capability_matrix(&wezterm);
    for feature in [
        "inline_images",
        "inline_video",
        "native_notifications",
        "terminal_title",
        "modern_keyboard",
    ] {
        assert!(modern.is_enabled(feature));
    }
}
