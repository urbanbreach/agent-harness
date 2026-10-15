use super::*;

fn compaction_settings(
    compaction: serde_json::Value,
) -> Result<CompactionSettings, serde_json::Error> {
    serde_json::from_value(compaction)
}

#[test]
fn threshold_maps_follow_the_persisted_grammar() -> Result<(), Box<dyn std::error::Error>> {
    use serde_json::json;
    let schema = config_json_schema();
    let validator = jsonschema::validator_for(&schema)?;
    for (map, key, input, expected) in [
        (
            "modelThresholds",
            "local/model*",
            json!(75),
            json!({"base":75}),
        ),
        (
            "modelThresholds",
            "local/model*",
            json!("75%"),
            json!("75%"),
        ),
        (
            "modelThresholds",
            "local/model*",
            json!(" 80.5% "),
            json!("80.5%"),
        ),
        (
            "modelThresholds",
            "local/model*",
            json!("0.5%"),
            json!("0.5%"),
        ),
        (
            "modelThresholds",
            "local:model",
            json!("f32000"),
            json!({"tokens":32000}),
        ),
        (
            "modelThresholds",
            "local/model",
            json!(" F32000 "),
            json!({"tokens":32000}),
        ),
        (
            "modelThresholds",
            "local/model",
            json!({"base":64000}),
            json!({"base":64000}),
        ),
        (
            "modelThresholds",
            "local/model",
            json!({"tokens":9000}),
            json!({"tokens":9000}),
        ),
        (
            "modelThresholds",
            "local/model",
            json!(u64::MAX),
            json!({"base":u32::MAX}),
        ),
        (
            "agentThresholds",
            "default",
            json!(50000),
            json!({"tokens":50000}),
        ),
        ("agentThresholds", "default", json!("75%"), json!("75%")),
        (
            "agentThresholds",
            "default",
            json!({"tokens":9000}),
            json!({"tokens":9000}),
        ),
    ] {
        let snake = if map == "modelThresholds" {
            "model_thresholds"
        } else {
            "agent_thresholds"
        };
        let value = json!({"runtime":{"compaction":{map:{key:input}}}});
        assert!(validator.is_valid(&value), "{value}");
        let config = load_config_from_str(&value.to_string())?;
        let entries = serde_json::to_value(&config.runtime.compaction)?;
        assert_eq!(entries[snake][key], expected, "{value}");
    }
    for (map, key, input) in [
        ("model_thresholds", "local/model", json!("101%")),
        ("model_thresholds", "local/model", json!("0%")),
        ("model_thresholds", "local/model", json!("80")),
        ("model_thresholds", "local/model", json!("f0")),
        ("model_thresholds", "local/model", json!(0)),
        (
            "model_thresholds",
            "local/model",
            json!({"tokens":1,"base":2}),
        ),
        ("model_thresholds", "local/mo*del", json!(50)),
        ("model_thresholds", "local/mo del", json!(50)),
        ("model_thresholds", "local", json!(50)),
        ("agent_thresholds", "default", json!("f1000")),
        ("agent_thresholds", "default", json!({"base":64000})),
        ("agent_thresholds", "default", json!(0)),
    ] {
        let value = json!({"runtime":{"compaction":{map:{key:input}}}});
        assert!(!validator.is_valid(&value), "{value}");
        assert!(load_config_from_str(&value.to_string()).is_err(), "{value}");
    }
    for compaction in [
        json!({"split_oversized_turns":true}),
        json!({"structured_summary_contract":true}),
        json!({"suppress_auto_compaction":true}),
        json!({"archive":{"system_prompt":"some"}}),
        json!({"remote_models":{"local/model":{"unknown":true}}}),
        json!({"threshold_percent":101}),
    ] {
        let value = json!({"runtime":{"compaction":compaction}});
        assert!(!validator.is_valid(&value), "{value}");
        assert!(load_config_from_str(&value.to_string()).is_err());
    }
    Ok(())
}

#[test]
fn null_clears_maps_and_inherited_entries() -> Result<(), Box<dyn std::error::Error>> {
    use serde_json::json;
    let mut layered =
        json!({"model_thresholds":{"a/b":"70%","c/d":"50%"},"summary_models":{"a/b":"c/d"}});
    normalize::merge(
        &mut layered,
        json!({"model_thresholds":{"a/b":null},"summary_models":null}),
    );
    let config = compaction_settings(layered)?;
    assert_eq!(config.model_thresholds.keys().collect::<Vec<_>>(), ["c/d"]);
    assert!(config.summary_models.is_empty());
    let missing = compaction_settings(json!({}))?;
    assert!(missing.model_thresholds.is_empty() && missing.remote_models.is_empty());
    assert!(
        compaction_settings(json!({"agent_thresholds":null,"remote_models":null}))?
            .agent_thresholds
            .is_empty()
    );
    Ok(())
}

#[test]
fn scalar_settings_accept_sentinels_decimals_and_wide_numbers(
) -> Result<(), Box<dyn std::error::Error>> {
    use serde_json::json;
    let unset = compaction_settings(
        json!({"threshold_percent":-1,"threshold_tokens":-1,"reserve_tokens":-1,"base_window_tokens":-1}),
    )?;
    assert!(
        unset.threshold_percent.is_none()
            && unset.threshold_tokens.is_none()
            && unset.reserve_tokens.is_none()
            && unset.base_window_tokens.is_none()
    );
    for (input, expected) in [(json!(0.5), 0.5), (json!("80.5%"), 80.5), (json!(75), 75.0)] {
        let percent = compaction_settings(json!({"threshold_percent":input}))?.threshold_percent;
        assert_eq!(percent.map(CompactionThresholdPercent::get), Some(expected));
    }
    assert_eq!(
        serde_json::to_value(compaction_settings(json!({"threshold_percent":75}))?)?
            ["threshold_percent"],
        json!(75)
    );
    for invalid in [
        json!({"threshold_percent":0}),
        json!({"threshold_percent":100.5}),
        json!({"threshold_percent":"80"}),
        json!({"threshold_tokens":0}),
        json!({"reserve_tokens":-2}),
        json!({"keep_recent_tokens":-1}),
        json!({"fallback_input_tokens":1.5}),
    ] {
        assert!(compaction_settings(invalid.clone()).is_err(), "{invalid}");
    }
    let wide = compaction_settings(json!({
        "keep_recent_tokens":u64::MAX,
        "v2_retained_message_budget":-5,
        "idle_threshold_tokens":12.9,
        "idle_timeout_seconds":-1,
        "remote_endpoint":"  ",
        "method_order":["soft","bogus",7,"archive","soft"],
    }))?;
    assert_eq!(
        (
            wide.keep_recent_tokens,
            wide.v2_retained_message_budget,
            wide.idle_threshold_tokens,
            wide.idle_timeout_seconds,
            wide.remote_endpoint,
            wide.method_order,
        ),
        (
            u32::MAX,
            0,
            12,
            0,
            None,
            vec![CompactionMethod::Soft, CompactionMethod::Archive]
        )
    );
    assert_eq!(
        compaction_settings(json!({"method_order":"soft"}))?.method_order,
        CompactionMethod::DEFAULT_ORDER
    );
    Ok(())
}

#[test]
fn new_compaction_settings_have_defaults_aliases_and_schema_entries(
) -> Result<(), Box<dyn std::error::Error>> {
    use serde_json::json;
    let defaults = load_config_from_str("{}")?.runtime.compaction;
    assert!(defaults.remote_enabled && !defaults.notes_windows);
    assert_eq!(
        (
            defaults.retry.enabled,
            defaults.retry.max_retries,
            defaults.retry.base_delay_ms
        ),
        (true, 10, 500)
    );
    assert_eq!(
        (
            defaults.archive.shape.as_str(),
            defaults.archive.system_prompt,
            defaults.archive.tool_results
        ),
        ("auto", ArchiveSystemPrompt::None, false)
    );
    let value = json!({"runtime":{"compaction":{
        "remoteEnabled":false,"baseWindowTokens":100000,"notesWindows":true,
        "remoteModels":{"openai/gpt-*":{"enabled":true,"v2StreamingEnabled":false,"v2Endpoint":"https://example.test/v2"}},
        "summaryModels":{"openai/gpt-*":"openai/gpt-mini"},
        "retry":{"enabled":false,"maxRetries":3,"baseDelayMs":50},
        "archive":{"systemPrompt":"agents_md","toolResults":true},
    }}});
    let validator = jsonschema::validator_for(&config_json_schema())?;
    assert!(validator.is_valid(&value), "{value}");
    let config = load_config_from_str(&value.to_string())?.runtime.compaction;
    assert!(!config.remote_enabled && config.notes_windows);
    assert_eq!(config.base_window_tokens, Some(100_000));
    assert_eq!(
        config.remote_models["openai/gpt-*"].v2_streaming_enabled,
        Some(false)
    );
    assert_eq!(config.summary_models["openai/gpt-*"], "openai/gpt-mini");
    assert_eq!(
        (
            config.retry.enabled,
            config.retry.max_retries,
            config.retry.base_delay_ms
        ),
        (false, 3, 50)
    );
    assert_eq!(
        (config.archive.system_prompt, config.archive.tool_results),
        (ArchiveSystemPrompt::AgentsMd, true)
    );
    Ok(())
}

#[test]
fn settings_writes_migrate_numeric_percentages_to_strings() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("harness.json");
    std::fs::write(
        &path,
        r#"{"runtime":{"compaction":{"modelThresholds":{"a/b":50,"c/d":64000},"agent_thresholds":{"worker":80,"lead":9000}}}}"#,
    )?;
    write_project_setting_value(&path, "runtime.compaction.fallback_input_tokens", "4096")?;
    let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    let compaction = &saved["runtime"]["compaction"];
    assert_eq!(
        compaction["model_thresholds"],
        serde_json::json!({"a/b":"50%","c/d":64000})
    );
    assert_eq!(
        compaction["agent_thresholds"],
        serde_json::json!({"worker":"80%","lead":9000})
    );
    Ok(())
}

#[test]
fn collapse_compacted_history_defaults_on_and_persists_in_tui_json(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let runtime = temp.path().join("harness.jsonc");
    let tui = temp.path().join("tui.json");
    let context = ConfigDiscoveryContext {
        current_dir: temp.path().into(),
        harness_home: None,
        home: None,
        data_dir: Some(temp.path().join("data/harness")),
        runtime_config_path: None,
        tui_config_path: None,
    };
    assert!(collapse_compacted_history_settings(Some(&runtime), &context)?.0);
    write_collapse_compacted_history(&tui, false)?;
    assert_eq!(
        collapse_compacted_history_settings(Some(&runtime), &context)?,
        (false, tui)
    );
    Ok(())
}
