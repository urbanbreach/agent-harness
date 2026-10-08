use harness::{run, CliDeps, CliIo};
use serde_json::{json, Value};
use std::{fs, io::Cursor, net::TcpListener};

#[test]
fn inspection_is_offline_redacted_and_reports_unknown_model_limits(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let endpoint = TcpListener::bind("127.0.0.1:0")?;
    endpoint.set_nonblocking(true)?;
    let config = root.path().join("fixture.json");
    fs::write(&config, json!({
        "provider":{"local":{"type":"openai_compatible","baseURL":format!("http://{}/v1",endpoint.local_addr()?),
            "apiKey":"${INSPECTION_KEY}", "models":{"known":{"name":"Private opaque-inspection-token","limit":{"context":4000,"output":500}},"unknown":{}}}},
        "model":"local/known","agent":{"default":{"tools":[]}}
    }).to_string())?;
    let deps = CliDeps::real()
        .with_current_dir(root.path().into())
        .with_env(
            "HARNESS_DATA_HOME",
            root.path().join("data").to_string_lossy(),
        )
        .with_env("INSPECTION_KEY", "opaque-inspection-token");
    for args in [
        vec!["doctor", "--json"],
        vec!["models", "list", "--json"],
        vec!["providers", "protocols"],
    ] {
        let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            ["harness", "--config", "fixture.json"]
                .into_iter()
                .chain(args.clone()),
            &mut CliIo::new(&mut input, &mut output, &mut errors),
            deps.clone(),
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&errors));
        let report: Value = serde_json::from_slice(&output)?;
        assert!(!String::from_utf8(output)?.contains("opaque-inspection-token"));
        match args[0] {
            "doctor" => {
                assert_eq!(report["no_network_probes"], true);
                assert_eq!(report["provider_execution_proof"], false);
                assert!(report["checks"]
                    .as_array()
                    .ok_or("checks missing")?
                    .iter()
                    .any(|check| check["status"] == "warn" && check["name"] == "model_limits"));
            }
            "models" => {
                let rows = report.as_array().ok_or("models missing")?;
                let unknown = rows
                    .iter()
                    .find(|r| r["model"] == "unknown")
                    .ok_or("unknown model missing")?;
                assert!(unknown["limits"]["context_window"]["tokens"].is_null());
                let known = rows
                    .iter()
                    .find(|r| r["model"] == "known")
                    .ok_or("known model missing")?;
                assert_eq!(known["limits"]["context_window"]["tokens"], 4000);
            }
            _ => assert!(report
                .as_array()
                .ok_or("protocols missing")?
                .iter()
                .any(
                    |row| row["protocol"] == "anthropic_messages" && row["support"] == "supported"
                )),
        }
    }
    assert_eq!(
        fs::read_dir(root.path())?.count(),
        1,
        "inspection wrote workspace files"
    );
    assert!(matches!(endpoint.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    fs::write(&config, json!({"provider":{"custom":{"type":"openai_compatible","authProvider":"codex","apiKeyEnv":[],"models":{"gpt-6-astra":{}}}},"model":"custom/gpt-6-astra"}).to_string())?;
    let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
    let result = run(
        ["harness", "--config", "fixture.json", "doctor", "--json"],
        &mut CliIo::new(&mut input, &mut output, &mut errors),
        deps.with_env(
            "HARNESS_DATA_HOME",
            root.path().join("empty-data").to_str().ok_or("data path")?,
        ),
    );
    assert_ne!(
        result.code, 0,
        "a subscription without credentials is not anonymous access"
    );
    assert!(!root.path().join("empty-data").exists());
    Ok(())
}

#[test]
fn catalog_generation_filters_models_and_preserves_output_after_invalid_input(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let output = root.path().join("catalog.json");
    let raw = json!({"openai":{"name":"OpenAI","env":["OPENAI_API_KEY"],"models":{
        "reasoning":{"tool_call":true,"reasoning":true,"family":"claude","modalities":{"input":["text","image"],"output":["text"]},"limit":{"context":10000,"output":2000}},
        "retired":{"tool_call":true,"status":"deprecated","limit":{"context":10000,"output":2000}},
        "text":{"tool_call":false,"limit":{"context":10000,"output":2000}},
        "unknown":{"tool_call":true}
    }}}).to_string();
    for valid in [true, false] {
        let (mut input, mut stdout, mut stderr) = (
            Cursor::new(if valid { raw.as_bytes() } else { b"invalid" }),
            Vec::new(),
            Vec::new(),
        );
        let result = run(
            [
                "harness",
                "models",
                "generate",
                "--stdin",
                "--provider",
                "openai",
                "--output",
                "catalog.json",
            ],
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            CliDeps::real()
                .with_current_dir(root.path().into())
                .with_env(
                    "HARNESS_DATA_HOME",
                    root.path().join("data").to_string_lossy(),
                ),
        );
        assert_eq!(
            result.code == 0,
            valid,
            "{}",
            String::from_utf8_lossy(&stderr)
        );
        let catalog = harness_core::provider_catalog::ProviderCatalog::from_path(&output)?;
        let provider = catalog.provider("openai").ok_or("provider missing")?;
        assert_eq!(provider.base_url, "https://api.openai.com/v1");
        let models = &provider.models;
        assert_eq!(models.len(), 1);
        let reasoning = &models["reasoning"].definition;
        assert!(reasoning.modalities.input.contains(&"image".into()));
        assert_eq!(
            reasoning.variants["high"].metadata.reasoning_effort,
            Some(harness_core::config::ModelVariantReasoningEffort::High)
        );
    }
    Ok(())
}

#[test]
fn config_commands_explain_layer_precedence_without_writes_or_secret_output(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let global = root.path().join("xdg/harness");
    let project = root.path().join("project");
    fs::create_dir_all(&global)?;
    fs::create_dir(&project)?;
    fs::write(
        global.join("harness.jsonc"),
        "{runtime:{compaction:{enabled:false,fallbackInputTokens:4096}}}",
    )?;
    fs::write(
        project.join("harness.jsonc"),
        r#"{provider:{local:{type:'openai_compatible',apiKey:'${INSPECTION_KEY}',models:{fixture:{}}}},model:'local/fixture',runtime:{compaction:{fallback_input_tokens:8192}}}"#,
    )?;
    fs::write(
        project.join("tui.jsonc"),
        "{keybinds:{copy_selection:'ctrl+y'}}",
    )?;
    let deps = CliDeps::real()
        .with_current_dir(project.clone())
        .with_env(
            "HARNESS_DATA_HOME",
            root.path().join("data").to_string_lossy(),
        )
        .with_env(
            "XDG_CONFIG_HOME",
            root.path().join("xdg").to_str().ok_or("xdg path")?,
        )
        .with_env(
            "HARNESS_CONFIG_CONTENT",
            "{runtime:{compaction:{fallbackInputTokens:16384}}}",
        )
        .with_env("INSPECTION_KEY", "opaque-configuration-token");
    for args in [
        vec!["validate"],
        vec!["show", "--effective"],
        vec!["sources"],
        vec!["explain", "runtime.compaction.fallbackInputTokens"],
        vec!["explain", "provider.local.apiKey"],
        vec!["settings"],
    ] {
        let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            ["harness", "config"].into_iter().chain(args.clone()),
            &mut CliIo::new(&mut input, &mut output, &mut errors),
            deps.clone(),
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&errors));
        let text = String::from_utf8(output)?;
        assert!(!text.contains("opaque-configuration-token"));
        if args[0] == "validate" {
            assert!(text.contains("tui.jsonc"));
            continue;
        }
        let report: Value = serde_json::from_str(&text)?;
        match args.as_slice() {
            ["show", _] => {
                assert_eq!(
                    report["effective"]["runtime"]["compaction"]["fallback_input_tokens"],
                    16384
                );
                assert_eq!(
                    report["effective"]["runtime"]["compaction"]["enabled"],
                    false
                );
                assert_eq!(
                    report["effective"]["providers"]["local"]["apiKey"],
                    "[REDACTED]"
                );
            }
            ["explain", "provider.local.apiKey"] => assert_eq!(report["effective"], "[REDACTED]"),
            ["explain", _] => {
                assert_eq!(report["effective"], 16384);
                assert_eq!(report["source_path"], "HARNESS_CONFIG_CONTENT");
            }
            ["sources"] => assert_eq!(report["layer_count"], 4),
            _ => {}
        }
    }
    assert_eq!(fs::read_dir(&project)?.count(), 2);
    fs::write(project.join("harness.jsonc"), "{runtme:{}}")?;
    let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
    let result = run(
        ["harness", "config", "validate"],
        &mut CliIo::new(&mut input, &mut output, &mut errors),
        deps,
    );
    assert_ne!(result.code, 0);
    assert!(output.is_empty());
    assert_eq!(fs::read_dir(&project)?.count(), 2);
    Ok(())
}
