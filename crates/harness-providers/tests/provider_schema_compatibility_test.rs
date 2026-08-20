use harness_providers::schema_compat::{prepare_tools_for_family, ProviderSchemaFamily};
use harness_providers::UnwrapOrAbort;
use harness_providers::{Provider, ProviderErrorCategory, ProviderStreamEvent, ToolDef};
use provider_schema_compatibility_support::{completion_request, provider, real_tools};
use serde_json::json;
use tokio_stream::StreamExt;

#[path = "support/provider_schema_compatibility_support.rs"]
mod provider_schema_compatibility_support;

#[test]
fn strict_openai_compatible_accepts_real_default_and_explore_tools() {
    // arrange
    let tools = [real_tools("default"), real_tools("explore")].concat();

    // act
    let prepared =
        prepare_tools_for_family(ProviderSchemaFamily::OpenAiCompatible, tools).unwrap_or_abort();

    // assert
    assert!(prepared.iter().any(|tool| tool.tool_id == "shell.run"));
    assert!(prepared
        .iter()
        .any(|tool| tool.function_name == "shell_run"));
    assert!(prepared
        .iter()
        .all(|tool| has_object_root(&tool.parameters)));
    assert!(prepared
        .iter()
        .all(|tool| has_no_top_level_combinator(&tool.parameters)));
}

#[test]
fn openai_compatible_canonicalizes_nested_native_schema_extensions() {
    // arrange
    let tools = real_tools("explore");

    // act
    let prepared =
        prepare_tools_for_family(ProviderSchemaFamily::OpenAiCompatible, tools).unwrap_or_abort();
    let question = prepared
        .iter()
        .find(|tool| tool.tool_id == "question")
        .unwrap_or_abort();

    // assert
    assert!(has_no_unsupported_openai_keywords(&question.parameters));
    assert_eq!(
        question.parameters["properties"]["questions"]["items"]["type"],
        json!("object")
    );
}

#[test]
fn openai_compatible_rejects_unresolved_refs_instead_of_widening_schema() {
    // arrange
    let mut tools = real_tools("explore");
    tools[0].parameters["properties"]["broken"] = json!({"$ref": "#/$defs/Missing"});

    // act
    let result = prepare_tools_for_family(ProviderSchemaFamily::OpenAiCompatible, tools);

    // assert
    assert!(result.is_err());
}

#[test]
fn openai_compatible_rejects_heterogeneous_tuple_items() {
    // arrange
    let mut tools = real_tools("explore");
    tools[0].parameters["properties"]["tuple"] = json!({
        "type": "array",
        "items": [{"type": "string"}, {"type": "number"}],
    });

    // act
    let result = prepare_tools_for_family(ProviderSchemaFamily::OpenAiCompatible, tools);

    // assert
    assert!(result.is_err());
}

#[test]
fn gemini_like_normalizes_real_mcp_schema_extensions_and_rejects_top_level_combinators() {
    // arrange
    let mut tools = real_tools("mcp");
    let tool = tools
        .iter_mut()
        .find(|tool| tool.tool_id == "mcp.docs.rs.tool.call")
        .unwrap_or_abort();
    let properties = tool.parameters["properties"]
        .as_object_mut()
        .unwrap_or_abort();
    properties.insert("optional".to_string(), json!({"type": ["string", "null"]}));
    properties.insert(
        "mode".to_string(),
        json!({"type": "integer", "enum": [1, 2]}),
    );
    tool.parameters["required"] = json!(["name", "missing"]);

    // act
    let prepared = prepare_tools_for_family(ProviderSchemaFamily::Gemini, tools).unwrap_or_abort();
    let schema = &prepared[0].parameters;

    // assert
    assert_eq!(schema["properties"]["mode"]["enum"], json!(["1", "2"]));
    assert_eq!(schema["properties"]["optional"]["nullable"], json!(true));
    assert!(!schema["required"]
        .as_array()
        .is_some_and(|items| items.contains(&json!("missing"))));

    let err = prepare_tools_for_family(ProviderSchemaFamily::Gemini, vec![bad_top_level_tool()])
        .expect_err("top-level combinators must fail before provider request serialization");
    assert!(err.to_string().contains("top-level"));
    assert!(err.to_string().contains("mcp.docs.rs.tool.call"));
}

#[test]
fn kimi_like_normalizes_ref_siblings_and_tuple_items_on_real_tool_schema() {
    // arrange
    let mut tools = real_tools("default");
    tools[0].parameters["properties"]["refArg"] = json!({
        "$ref": "#/$defs/Arg",
        "description": "Moonshot rejects siblings next to $ref"
    });
    tools[0].parameters["properties"]["tupleArg"] = json!({
        "type": "array",
        "items": [{"type": "string"}, {"type": "number"}]
    });

    // act
    let prepared = prepare_tools_for_family(ProviderSchemaFamily::Kimi, tools).unwrap_or_abort();
    let schema = &prepared[0].parameters;

    // assert
    assert_eq!(
        schema["properties"]["refArg"],
        json!({"$ref": "#/$defs/Arg"})
    );
    assert_eq!(
        schema["properties"]["tupleArg"]["items"],
        json!({"type": "string"})
    );
}

#[tokio::test]
async fn provider_rejects_incompatible_schema_before_http_request_leaves_adapter() {
    // arrange
    let (provider, http) = provider();

    // act
    let events = provider
        .stream_completion(completion_request(
            "google",
            "gemini-2.5-pro",
            vec![bad_top_level_tool()],
        ))
        .await
        .collect::<Vec<_>>()
        .await;

    // assert
    assert_eq!(http.calls().len(), 0);
    assert!(matches!(
        events.as_slice(),
        [ProviderStreamEvent::Error { category: Some(ProviderErrorCategory::UnsupportedToolCall), message, .. }]
            if message.contains("mcp.docs.rs.tool.call") && message.contains("top-level")
    ));
}

fn bad_top_level_tool() -> ToolDef {
    let mut tool = real_tools("mcp").remove(0);
    tool.parameters = json!({"anyOf": [{"type": "object", "properties": {}}]});
    tool
}

fn has_object_root(schema: &serde_json::Value) -> bool {
    schema["type"].as_str() == Some("object")
}

fn has_no_top_level_combinator(schema: &serde_json::Value) -> bool {
    ["oneOf", "anyOf", "allOf", "enum", "not"]
        .iter()
        .all(|key| schema.get(key).is_none())
}

fn has_no_unsupported_openai_keywords(schema: &serde_json::Value) -> bool {
    match schema {
        serde_json::Value::Array(items) => items.iter().all(has_no_unsupported_openai_keywords),
        serde_json::Value::Object(object) => {
            let unsupported = [
                "$defs",
                "$ref",
                "$schema",
                "allOf",
                "anyOf",
                "definitions",
                "not",
                "oneOf",
            ];
            unsupported.iter().all(|key| !object.contains_key(*key))
                && object.values().all(has_no_unsupported_openai_keywords)
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => true,
    }
}
