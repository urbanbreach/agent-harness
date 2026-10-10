use harness::{run, CliDeps, CliIo};
use serde_json::{json, Value};
use std::{fs, io::Cursor, path::Path};

#[test]
fn public_schema_accepts_loader_syntax_and_rejects_misspelled_settings(
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
    let result = run(
        ["harness", "schema"],
        &mut CliIo::new(&mut input, &mut output, &mut errors),
        CliDeps::real(),
    );
    assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&errors));
    let schema: Value = serde_json::from_slice(&output)?;
    let validator = jsonschema::validator_for(&schema)?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let example: Value = json5::from_str(&fs::read_to_string(
        root.join("configs/harness.example.jsonc"),
    )?)?;
    for config in [
        example,
        json!({"formatter":true}),
        json!({"eval":{"languages":["js","py"],"parallel_pool_width":8},"permission":{"eval":"ask"}}),
        json!({"formatter":{"languages":{"demo":{"command":["formatter","$FILE"]}},"uvformat":{"disabled":true}}}),
        json!({"provider":{"local":{"type":"openai_compatible","baseURL":"http://localhost:1234","models":{"fixture":{"cost":{"input":3,"output":15,"cacheRead":0.3}}}}},"model":"local/fixture","instructions":["Follow the rules"],"permission":{"bash":{"git *":"allow","*":"deny"}},"agent":{"default":{"tools":{"read":true,"bash":false},"permission":"deny"}},"formatter":false,"lsp":false}),
        json!({"providers":{"local":{"type":"openai_compatible","options":{"base_url":"http://localhost:1234"},"models":{"fixture":{}}}},"model":"local/fixture","runtime":{"yolo":true,"compaction":{"thresholdPercent":80,"keep_recent_tokens":2000}}}),
        json!({"mcp":{"remote":{"transport":"streamable_http","url":"http://127.0.0.1:1/rpc","timeout":5,"enabled":false}}}),
    ] {
        harness_core::config::load_config_from_str(&config.to_string())?;
        let failures: Vec<_> = validator
            .iter_errors(&config)
            .map(|e| e.to_string())
            .collect();
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
    for config in [
        json!({"runtme":{}}),
        json!({"eval":{"languages":["javascript"]}}),
        json!({"eval":{"languages":["js","js"]}}),
        json!({"eval":{"run_budget_seconds":0}}),
        json!({"eval":{"max_detached_cells":257}}),
        json!({"runtime":{"compaction":{"threshold_percent":101}}}),
        json!({"permission":{"bash":"always"}}),
        json!({"agent":{"default":{"toos":[]}}}),
        json!({"agent":{"default":{"permission":{"shell":null}}}}),
        json!({"provider":{"local":{"type":"openai_compatible","baseURL":"http://localhost:1234","models":{"fixture":{"cost":{"input":-1,"output":15}}}}}}),
    ] {
        assert!(!validator.is_valid(&config), "{config}");
        assert!(harness_core::config::load_config_from_str(&config.to_string()).is_err());
    }
    let checked_in: Value =
        serde_json::from_str(&fs::read_to_string(root.join("configs/config.json"))?)?;
    assert_eq!(checked_in, schema);
    output.clear();
    let result = run(
        ["harness", "schema", "--tui"],
        &mut CliIo::new(&mut input, &mut output, &mut errors),
        CliDeps::real(),
    );
    assert_eq!(result.code, 0);
    assert_eq!(
        serde_json::from_slice::<Value>(&output)?,
        serde_json::from_str::<Value>(&fs::read_to_string(root.join("configs/tui.json"))?)?
    );
    Ok(())
}
