use harness_tools::UnwrapOrAbort;
use std::sync::{Arc, Mutex};

mod common;

use async_trait::async_trait;
use common::{
    expect_execution_error, expect_invalid_arguments, setup_workspace_fixture,
    test_context as common_test_context,
};
use harness_core::config::ShellAllowlist;
use harness_core::tool::ToolError;
use harness_tools::{
    coordinator_registry_with_remote_search_transport, RemoteSearchHttpRequest,
    RemoteSearchHttpResponse, RemoteSearchHttpTransport, RemoteSearchTestConfig,
};
use serde_json::json;

fn test_context(
    workspace_root: &std::path::Path,
    tool_call_id: &str,
) -> harness_core::tool::ToolContext {
    common_test_context(workspace_root, "run-native-web-search-tests", tool_call_id)
}

#[derive(Debug)]
struct ScriptedRemoteSearchTransport {
    requests: Mutex<Vec<RemoteSearchHttpRequest>>,
    responses: Mutex<Vec<RemoteSearchHttpResponse>>,
}

impl ScriptedRemoteSearchTransport {
    fn new(responses: Vec<RemoteSearchHttpResponse>) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses),
        })
    }

    fn requests(&self) -> Vec<RemoteSearchHttpRequest> {
        self.requests.lock().unwrap_or_abort().clone()
    }
}

#[async_trait]
impl RemoteSearchHttpTransport for ScriptedRemoteSearchTransport {
    async fn execute(
        &self,
        request: RemoteSearchHttpRequest,
    ) -> Result<RemoteSearchHttpResponse, ToolError> {
        self.requests.lock().unwrap_or_abort().push(request);
        Ok(self.responses.lock().unwrap_or_abort().remove(0))
    }
}

fn search_registry(
    transport: Arc<ScriptedRemoteSearchTransport>,
    config: RemoteSearchTestConfig,
) -> harness_core::tool::ToolRegistry {
    coordinator_registry_with_remote_search_transport(ShellAllowlist::default(), config, transport)
        .unwrap_or_abort()
}

#[tokio::test]
async fn native_web_search_rejects_non_baseline_control_values_before_network() {
    // arrange
    let workspace = setup_workspace_fixture();
    let transport = ScriptedRemoteSearchTransport::new(Vec::new());
    let registry = search_registry(
        Arc::clone(&transport),
        RemoteSearchTestConfig {
            auth_token: None,
            require_auth: false,
            max_retries: 0,
            retry_backoff_ms: 1,
            timeout_secs: 1,
            ..RemoteSearchTestConfig::default()
        },
    );
    let websearch = registry.get("websearch").unwrap_or_abort();

    let schema = websearch.parameters_json_schema().to_string();
    assert!(schema.contains("fallback"));
    assert!(schema.contains("preferred"));
    assert!(schema.contains("deep"));
    assert!(schema.contains("50000"));

    // act
    let invalid_livecrawl = websearch
        .call(
            test_context(workspace.workspace(), "invalid-livecrawl"),
            json!({
                "query": "tokio runtime",
                "livecrawl": "always"
            }),
        )
        .await
        .expect_err("invalid livecrawl should fail before network call");

    // assert
    expect_invalid_arguments(invalid_livecrawl, "fallback");

    let invalid_type = websearch
        .call(
            test_context(workspace.workspace(), "invalid-type"),
            json!({
                "query": "tokio runtime",
                "type": "exhaustive"
            }),
        )
        .await
        .expect_err("invalid search type should fail before network call");
    expect_invalid_arguments(invalid_type, "deep");

    let invalid_count = websearch
        .call(
            test_context(workspace.workspace(), "invalid-count"),
            json!({
                "query": "tokio runtime",
                "numResults": 0
            }),
        )
        .await
        .expect_err("invalid numResults should fail before network call");
    expect_invalid_arguments(invalid_count, "numResults");

    let invalid_context = websearch
        .call(
            test_context(workspace.workspace(), "invalid-context"),
            json!({
                "query": "tokio runtime",
                "contextMaxCharacters": 50_001
            }),
        )
        .await
        .expect_err("invalid contextMaxCharacters should fail before network call");
    expect_invalid_arguments(invalid_context, "contextMaxCharacters");

    assert!(transport.requests().is_empty());
}

#[tokio::test]
async fn native_web_search_uses_shared_client_and_fixture_backend() {
    // arrange
    // act
    // assert
    let workspace = setup_workspace_fixture();
    let transport = ScriptedRemoteSearchTransport::new(vec![
        RemoteSearchHttpResponse::new(
            200,
            json!({
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": "Tokio runtime docs\nJoinSet guide"
                        }
                    ]
                }
            })
            .to_string(),
        ),
        RemoteSearchHttpResponse::new(
            200,
            json!({
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": "Tokio runtime docs\nJoinSet guide"
                        }
                    ]
                }
            })
            .to_string(),
        ),
    ]);

    let registry = search_registry(
        Arc::clone(&transport),
        RemoteSearchTestConfig {
            max_retries: 0,
            retry_backoff_ms: 1,
            timeout_secs: 1,
            ..RemoteSearchTestConfig::default()
        },
    );
    let websearch = registry.get("websearch").unwrap_or_abort();

    let first_result = websearch
        .call(
            test_context(workspace.workspace(), "websearch-first"),
            json!({
                "query": "tokio runtime",
                "numResults": 2,
                "livecrawl": "preferred",
                "type": "fast",
                "contextMaxCharacters": 4096,
            }),
        )
        .await
        .unwrap_or_abort();
    let second_result = websearch
        .call(
            test_context(workspace.workspace(), "websearch-second"),
            json!({
                "query": "tokio runtime",
                "numResults": 2,
                "livecrawl": "preferred",
                "type": "fast",
                "contextMaxCharacters": 4096,
            }),
        )
        .await
        .unwrap_or_abort();

    assert_eq!(
        first_result.display_text,
        "Tokio runtime docs\nJoinSet guide"
    );
    assert_eq!(first_result.display_text, second_result.display_text);
    assert_eq!(first_result.structured_json, second_result.structured_json);
    let result_json = first_result.structured_json.unwrap_or_abort();
    assert_eq!(result_json["query"], json!("tokio runtime"));
    assert_eq!(result_json["numResults"], json!(2));
    assert_eq!(result_json["livecrawl"], json!("preferred"));
    assert_eq!(result_json["type"], json!("fast"));
    assert_eq!(result_json["contextMaxCharacters"], json!(4096));
    assert_eq!(result_json["empty"], json!(false));

    let requests = transport.requests();
    assert_eq!(
        requests.len(),
        2,
        "websearch should hit the shared backend path for each request"
    );
    for request in requests.iter() {
        assert_eq!(request.auth_token.as_deref(), Some("fixture-token"));
        assert_eq!(request.tool_name, "web_search_exa");
        assert_eq!(request.arguments["query"], json!("tokio runtime"));
        assert_eq!(request.arguments["numResults"], json!(2));
        assert_eq!(request.arguments["livecrawl"], json!("preferred"));
        assert_eq!(request.arguments["type"], json!("fast"));
        assert_eq!(request.arguments["contextMaxCharacters"], json!(4096));
    }
}

#[tokio::test]
async fn native_web_search_handles_missing_auth_rate_limit_and_empty_results() {
    // arrange
    // act
    // assert
    let workspace = setup_workspace_fixture();

    let missing_auth_registry = search_registry(
        ScriptedRemoteSearchTransport::new(Vec::new()),
        RemoteSearchTestConfig {
            auth_token: None,
            require_auth: true,
            max_retries: 0,
            retry_backoff_ms: 1,
            timeout_secs: 1,
            ..RemoteSearchTestConfig::default()
        },
    );
    let missing_auth = missing_auth_registry
        .get("websearch")
        .unwrap_or_abort()
        .call(
            test_context(workspace.workspace(), "missing-auth"),
            json!({
                "query": "tokio runtime"
            }),
        )
        .await
        .expect_err("missing auth should fail before network call");
    expect_execution_error(missing_auth, "authentication is not configured");

    let rate_limit_transport = ScriptedRemoteSearchTransport::new(vec![
        RemoteSearchHttpResponse::new(429, "rate limited").with_retry_after_secs(1),
        RemoteSearchHttpResponse::new(429, "rate limited").with_retry_after_secs(1),
    ]);
    let rate_limit_registry = search_registry(
        Arc::clone(&rate_limit_transport),
        RemoteSearchTestConfig {
            auth_token: Some("fixture-token".to_string()),
            require_auth: true,
            max_retries: 1,
            retry_backoff_ms: 1,
            timeout_secs: 1,
            ..RemoteSearchTestConfig::default()
        },
    );
    let rate_limit_error = rate_limit_registry
        .get("websearch")
        .unwrap_or_abort()
        .call(
            test_context(workspace.workspace(), "rate-limit"),
            json!({
                "query": "tokio runtime",
                "numResults": 1
            }),
        )
        .await
        .expect_err("rate limit should fail deterministically");
    expect_execution_error(
        rate_limit_error,
        "rate limit exceeded after 2 attempts; retry after 1s",
    );
    assert_eq!(rate_limit_transport.requests().len(), 2);

    let empty_registry = search_registry(
        ScriptedRemoteSearchTransport::new(vec![RemoteSearchHttpResponse::new(
            200,
            "data: {\"result\":{\"content\":[]}}\n\n",
        )]),
        RemoteSearchTestConfig {
            auth_token: None,
            require_auth: false,
            max_retries: 0,
            retry_backoff_ms: 1,
            timeout_secs: 5,
            ..RemoteSearchTestConfig::default()
        },
    );
    let empty_result = empty_registry
        .get("websearch")
        .unwrap_or_abort()
        .call(
            test_context(workspace.workspace(), "empty-results"),
            json!({
                "query": "no matches fixture"
            }),
        )
        .await
        .unwrap_or_abort();
    assert_eq!(empty_result.display_text, "No search results found");
    let empty_json = empty_result.structured_json.unwrap_or_abort();
    assert_eq!(empty_json["numResults"], json!(8));
    assert_eq!(empty_json["livecrawl"], json!("fallback"));
    assert_eq!(empty_json["type"], json!("auto"));
    assert_eq!(empty_json["empty"], json!(true));
}
