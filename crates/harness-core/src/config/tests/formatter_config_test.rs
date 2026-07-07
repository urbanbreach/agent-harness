use crate::config::public::translate_public_formatter_config;
use crate::config::{FormatterConfig, FormatterOverride};
use crate::UnwrapOrAbort;

#[test]
fn formatter_scalar_false_disables_formatting() {
    // arrange
    // act
    let translated =
        translate_public_formatter_config(Some(&serde_json::Value::Bool(false))).unwrap();

    // assert
    assert!(!translated.enabled);
    assert!(!translated.experimental_oxfmt);
    assert!(translated.overrides.is_empty());
}

#[test]
fn formatter_scalar_true_yields_defaults() {
    // arrange
    // act
    let translated =
        translate_public_formatter_config(Some(&serde_json::Value::Bool(true))).unwrap();

    // assert
    assert!(translated.enabled);
    assert!(!translated.experimental_oxfmt);
    assert!(translated.overrides.is_empty());
}

#[test]
fn formatter_none_yields_defaults() {
    // arrange
    // act
    let translated = translate_public_formatter_config(None).unwrap();

    // assert
    assert!(translated.enabled);
    assert!(!translated.experimental_oxfmt);
    assert!(translated.overrides.is_empty());
}

#[test]
fn formatter_object_parses_enabled_and_experimental_oxfmt() {
    // arrange
    let value = serde_json::json!({
        "enabled": false,
        "experimentalOxfmt": true,
    });
    // act
    let translated = translate_public_formatter_config(Some(&value)).unwrap();

    // assert
    assert!(!translated.enabled);
    assert!(translated.experimental_oxfmt);
    assert!(translated.overrides.is_empty());
}

#[test]
fn formatter_object_parses_named_override() {
    // arrange
    let value = serde_json::json!({
        "rustfmt": {
            "command": ["rustfmt", "--edition", "2021"],
            "extensions": [".rs"],
        },
    });
    // act
    let translated = translate_public_formatter_config(Some(&value)).unwrap();

    // assert
    assert!(translated.enabled);
    let rustfmt = translated.overrides.get("rustfmt").unwrap_or_abort();
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
    // arrange
    let value = serde_json::json!({
        "prettier": {
            "disabled": true,
            "command": ["prettier", "--write"],
            "extensions": [".js", ".ts"],
        },
    });
    // act
    let translated = translate_public_formatter_config(Some(&value)).unwrap();

    // assert
    let prettier = translated.overrides.get("prettier").unwrap_or_abort();
    assert!(prettier.disabled);
    assert_eq!(
        prettier.command,
        Some(vec!["prettier".to_string(), "--write".to_string()])
    );
}

#[test]
fn formatter_environment_is_preserved_at_config_level() {
    // arrange
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
    // act
    let translated = translate_public_formatter_config(Some(&value)).unwrap();

    // assert
    let black = translated.overrides.get("black").unwrap_or_abort();
    let environment = black.environment.as_ref().unwrap_or_abort();
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
fn formatter_uvformat_alias_translates_to_uv_key() {
    // arrange
    let value = serde_json::json!({
        "uvformat": { "disabled": true },
    });
    // act
    let translated = translate_public_formatter_config(Some(&value)).unwrap();

    // assert
    assert!(translated.enabled);
    assert!(
        !translated.overrides.contains_key("uvformat"),
        "legacy uvformat key should not remain in overrides"
    );
    let uv = translated.overrides.get("uv").unwrap_or_abort();
    assert!(uv.disabled);
}

#[test]
fn formatter_uv_canonical_key_takes_precedence_over_uvformat_alias() {
    // arrange
    let value = serde_json::json!({
        "uv": { "disabled": true },
        "uvformat": { "disabled": false },
    });
    // act
    let translated = translate_public_formatter_config(Some(&value)).unwrap();

    // assert
    let uv = translated.overrides.get("uv").unwrap_or_abort();
    assert!(
        uv.disabled,
        "canonical uv key should win when both are present"
    );
    assert!(!translated.overrides.contains_key("uvformat"));
}

#[test]
fn formatter_languages_backward_compat_converts_to_synthetic_overrides() {
    // arrange
    let value = serde_json::json!({
        "enabled": true,
        "languages": {
            "rs": { "command": ["rustfmt"] },
            "py": { "command": ["black"] },
        },
    });
    // act
    let translated = translate_public_formatter_config(Some(&value)).unwrap();

    // assert
    assert!(translated.enabled);

    let rust = translated.overrides.get("_lang_rs").unwrap_or_abort();
    assert_eq!(rust.command, Some(vec!["rustfmt".to_string()]));
    assert_eq!(rust.extensions, Some(vec![".rs".to_string()]));

    let python = translated.overrides.get("_lang_py").unwrap_or_abort();
    assert_eq!(python.command, Some(vec!["black".to_string()]));
    assert_eq!(python.extensions, Some(vec![".py".to_string()]));
}

#[test]
fn formatter_default_values_are_sensible() {
    // arrange
    // act
    let default = FormatterConfig::default();

    // assert
    assert!(default.enabled);
    assert!(!default.experimental_oxfmt);
    assert!(default.overrides.is_empty());
}

#[test]
fn formatter_override_default_values_are_sensible() {
    // arrange
    // act
    let override_value: FormatterOverride = serde_json::from_value(serde_json::json!({})).unwrap();

    // assert
    assert!(!override_value.disabled);
    assert!(override_value.command.is_none());
    assert!(override_value.environment.is_none());
    assert!(override_value.extensions.is_none());
}
