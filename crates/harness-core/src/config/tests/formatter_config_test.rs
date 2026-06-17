use crate::config::public::translate_public_formatter_config;
use crate::config::{FormatterConfig, FormatterOverride};

#[test]
fn formatter_scalar_false_disables_formatting() {
    let translated =
        translate_public_formatter_config(Some(&serde_json::Value::Bool(false))).unwrap();

    assert!(!translated.enabled);
    assert!(!translated.experimental_oxfmt);
    assert!(translated.overrides.is_empty());
}

#[test]
fn formatter_scalar_true_yields_defaults() {
    let translated =
        translate_public_formatter_config(Some(&serde_json::Value::Bool(true))).unwrap();

    assert!(translated.enabled);
    assert!(!translated.experimental_oxfmt);
    assert!(translated.overrides.is_empty());
}

#[test]
fn formatter_none_yields_defaults() {
    let translated = translate_public_formatter_config(None).unwrap();

    assert!(translated.enabled);
    assert!(!translated.experimental_oxfmt);
    assert!(translated.overrides.is_empty());
}

#[test]
fn formatter_object_parses_enabled_and_experimental_oxfmt() {
    let value = serde_json::json!({
        "enabled": false,
        "experimentalOxfmt": true,
    });
    let translated = translate_public_formatter_config(Some(&value)).unwrap();

    assert!(!translated.enabled);
    assert!(translated.experimental_oxfmt);
    assert!(translated.overrides.is_empty());
}

#[test]
fn formatter_object_parses_named_override() {
    let value = serde_json::json!({
        "rustfmt": {
            "command": ["rustfmt", "--edition", "2021"],
            "extensions": [".rs"],
        },
    });
    let translated = translate_public_formatter_config(Some(&value)).unwrap();

    assert!(translated.enabled);
    let rustfmt = translated
        .overrides
        .get("rustfmt")
        .expect("rustfmt override");
    assert_eq!(
        rustfmt.command,
        Some(vec![
            "rustfmt".to_string(),
            "--edition".to_string(),
            "2021".to_string()
        ])
    );
    assert_eq!(rustfmt.extensions, Some(vec![".rs".to_string()]));
    assert!(!rustfmt.disabled);
    assert!(rustfmt.environment.is_none());
}

#[test]
fn formatter_disable_by_name_sets_disabled_flag() {
    let value = serde_json::json!({
        "prettier": {
            "disabled": true,
            "command": ["prettier", "--write"],
            "extensions": [".js", ".ts"],
        },
    });
    let translated = translate_public_formatter_config(Some(&value)).unwrap();

    let prettier = translated
        .overrides
        .get("prettier")
        .expect("prettier override");
    assert!(prettier.disabled);
    assert_eq!(
        prettier.command,
        Some(vec!["prettier".to_string(), "--write".to_string()])
    );
}

#[test]
fn formatter_environment_is_preserved_at_config_level() {
    let value = serde_json::json!({
        "black": {
            "command": ["black"],
            "extensions": [".py"],
            "environment": {
                "PYTHONPATH": "/opt/python",
                "BLACK_CACHE_DIR": "/tmp/black",
            },
        },
    });
    let translated = translate_public_formatter_config(Some(&value)).unwrap();

    let black = translated.overrides.get("black").expect("black override");
    let environment = black.environment.as_ref().expect("environment map");
    assert_eq!(
        environment.get("PYTHONPATH"),
        Some(&"/opt/python".to_string())
    );
    assert_eq!(
        environment.get("BLACK_CACHE_DIR"),
        Some(&"/tmp/black".to_string())
    );
}

#[test]
fn formatter_languages_backward_compat_converts_to_synthetic_overrides() {
    let value = serde_json::json!({
        "enabled": true,
        "languages": {
            "rs": { "command": ["rustfmt"] },
            "py": { "command": ["black"] },
        },
    });
    let translated = translate_public_formatter_config(Some(&value)).unwrap();

    assert!(translated.enabled);

    let rust = translated
        .overrides
        .get("_lang_rs")
        .expect("synthetic rust override");
    assert_eq!(rust.command, Some(vec!["rustfmt".to_string()]));
    assert_eq!(rust.extensions, Some(vec![".rs".to_string()]));

    let python = translated
        .overrides
        .get("_lang_py")
        .expect("synthetic python override");
    assert_eq!(python.command, Some(vec!["black".to_string()]));
    assert_eq!(python.extensions, Some(vec![".py".to_string()]));
}

#[test]
fn formatter_default_values_are_sensible() {
    let default = FormatterConfig::default();

    assert!(default.enabled);
    assert!(!default.experimental_oxfmt);
    assert!(default.overrides.is_empty());
}

#[test]
fn formatter_override_default_values_are_sensible() {
    let override_value: FormatterOverride = serde_json::from_value(serde_json::json!({})).unwrap();

    assert!(!override_value.disabled);
    assert!(override_value.command.is_none());
    assert!(override_value.environment.is_none());
    assert!(override_value.extensions.is_none());
}
