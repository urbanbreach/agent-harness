use harness_core::config::ShellAllowlist;
use harness_tools::coordinator_registry;
use harness_tools::UnwrapOrAbort;

#[test]
fn high_risk_provider_visible_tool_fields_have_model_guidance() {
    // arrange
    let registry = coordinator_registry(ShellAllowlist::default());

    // act
    for (tool_id, fields) in [
        // assert
        ("read", &["path", "offset", "limit"] as &[&str]),
        ("list", &["path", "ignore"] as &[&str]),
        ("glob", &["pattern", "path", "limit"]),
        ("grep", &["pattern", "path", "include", "limit", "literal", "output_mode", "head_limit"]),
        ("edit", &["path", "oldString", "newString", "replaceAll"]),
        ("bash", &["command", "timeout", "workdir"]),
        (
            "task",
            &[
                "description",
                "prompt",
                "subagent_type",
                "category",
                "task_id",
                "session_id",
                "run_in_background",
                "load_skills",
                "command",
            ],
        ),
        (
            "background_output",
            &[
                "request_id",
                "task_id",
                "session_id",
                "block",
                "timeout",
                "cancel",
                "reason",
                "full_session",
                "include_thinking",
                "message_limit",
                "since_message_id",
                "include_tool_results",
                "thinking_max_chars",
                "from_end",
            ],
        ),
        ("background_cancel", &["request_id", "all", "reason"]),
        ("batch", &["tool_calls"]),
        ("skill", &["name"]),
        ("webfetch", &["url", "format", "timeout"]),
        (
            "websearch",
            &[
                "query",
                "numResults",
                "livecrawl",
                "type",
                "contextMaxCharacters",
            ],
        ),
        ("codesearch", &["query", "tokensNum"]),
    ] {
        let schema = registry
            .get(tool_id)
            .unwrap_or_else(|| panic!("missing tool {tool_id}"))
            .parameters_json_schema();
        assert_described_fields(tool_id, &schema, fields);
    }
}

#[test]
fn tool_descriptions_route_models_away_from_known_smoke_test_confusions() {
    // arrange
    let registry = coordinator_registry(ShellAllowlist::default());

    // act
    let codesearch = registry.get("codesearch").unwrap_or_abort();
    let read = registry.get("read").unwrap_or_abort();
    let edit = registry.get("edit").unwrap_or_abort();
    let task = registry.get("task").unwrap_or_abort();

    // assert
    assert!(!read.description().contains("hashlineAnchors"));
    assert!(!read.description().contains("LINE#HASH"));
    assert!(edit.description().contains("oldString"));
    assert!(edit.description().contains("newString"));
    assert!(edit.description().contains("replaceAll"));
    assert!(!edit.description().contains("LINE#HASH"));
    assert!(!edit.description().contains("delete=true"));
    assert!(
        codesearch.description().contains("remote/public"),
        "codesearch should not look like local workspace symbol search"
    );
    assert!(
        codesearch
            .description()
            .contains("Use grep, ast_grep_search, or lsp for local workspace"),
        "codesearch should point models at local-first tools"
    );
    assert!(
        task.description()
            .contains("Exactly one of `category` or `subagent_type` is required"),
        "task should spell out the routing requirement that the parser enforces"
    );

    let codesearch_schema = codesearch.parameters_json_schema();
    let query_description = codesearch_schema
        .pointer("/properties/query/description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_abort();
    assert!(query_description.contains("Remote/public"));
    assert!(query_description.contains("not local workspace symbol lookup"));

    let task_schema = task.parameters_json_schema();
    let category_description = task_schema
        .pointer("/properties/category/description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_abort();
    let subagent_description = task_schema
        .pointer("/properties/subagent_type/description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_abort();
    assert!(category_description.contains("Required when subagent_type is omitted"));
    assert!(subagent_description.contains("Required when category is omitted"));
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn assert_described_fields(tool_id: &str, schema: &serde_json::Value, fields: &[&str]) {
    let properties = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .unwrap_or_else(|| panic!("abort"));
    for field in fields {
        let description = properties
            .get(*field)
            .and_then(|field_schema| field_schema.get("description"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("abort"));
        assert!(
            description.trim().len() >= 16,
            "{tool_id}.{field} description is too sparse: {description}"
        );
    }
}
