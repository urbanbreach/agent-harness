use std::sync::{Arc, Mutex};

mod common;

use async_trait::async_trait;
use common::{
    expect_execution_error, setup_workspace_fixture, test_context as common_test_context,
};
use harness_core::config::ShellAllowlist;
use harness_core::tool::ToolError;
use harness_tools::{
    coordinator_registry_with_remote_search_transport, RemoteSearchHttpRequest,
    RemoteSearchHttpResponse, RemoteSearchHttpTransport, RemoteSearchTestConfig,
};
use serde_json::json;

const EMPTY_CODE_SEARCH_MESSAGE: &str = "No code snippets or documentation found. Please try a different query, be more specific about the library or programming concept, or check the spelling of framework names.";

fn test_context(
    workspace_root: &std::path::Path,
    tool_call_id: &str,
) -> harness_core::tool::ToolContext {
    common_test_context(workspace_root, "run-native-code-search-tests", tool_call_id)
}

#[derive(Debug)]
struct ScriptedRemoteSearchTransport {
    requests: Mutex<Vec<RemoteSearchHttpRequest>>,
    responses: Mutex<Vec<Result<RemoteSearchHttpResponse, ToolError>>>,
}

impl ScriptedRemoteSearchTransport {
    fn new(responses: Vec<Result<RemoteSearchHttpResponse, ToolError>>) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses),
        })
    }

    fn ok(responses: Vec<RemoteSearchHttpResponse>) -> Arc<Self> {
        Self::new(responses.into_iter().map(Ok).collect())
    }

    fn requests(&self) -> Vec<RemoteSearchHttpRequest> {
        self.requests.lock().expect("request log").clone()
    }
}

#[async_trait]
impl RemoteSearchHttpTransport for ScriptedRemoteSearchTransport {
    async fn execute(
        &self,
        request: RemoteSearchHttpRequest,
    ) -> Result<RemoteSearchHttpResponse, ToolError> {
        self.requests.lock().expect("request log").push(request);
        self.responses.lock().expect("response script").remove(0)
    }
}

fn search_registry(
    transport: Arc<ScriptedRemoteSearchTransport>,
    config: RemoteSearchTestConfig,
) -> harness_core::tool::ToolRegistry {
    coordinator_registry_with_remote_search_transport(ShellAllowlist::default(), config, transport)
        .expect("remote search test registry")
}

#[tokio::test]
async fn native_code_search_uses_shared_client_and_respects_tokens_contract() {
    let workspace = setup_workspace_fixture();
    let response_body = "data: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"Tokio JoinSet examples\\nspawn multiple tasks\"}]}}\n\n";
    let transport = ScriptedRemoteSearchTransport::ok(vec![
        RemoteSearchHttpResponse::new(200, response_body),
        RemoteSearchHttpResponse::new(200, response_body),
        RemoteSearchHttpResponse::new(200, response_body),
    ]);

    let registry = search_registry(
        Arc::clone(&transport),
        RemoteSearchTestConfig {
            auth_token: Some("fixture-token".to_string()),
            require_auth: true,
            timeout_secs: 5,
            max_retries: 0,
            retry_backoff_ms: 1,
            ..RemoteSearchTestConfig::default()
        },
    );
    let codesearch = registry.get("codesearch").expect("codesearch tool");

    let min_result = codesearch
        .call(
            test_context(workspace.workspace(), "codesearch-min"),
            json!({
                "query": "Tokio JoinSet rust example",
                "tokensNum": 25,
            }),
        )
        .await
        .expect("codesearch min clamp");
    let default_result = codesearch
        .call(
            test_context(workspace.workspace(), "codesearch-default"),
            json!({
                "query": "Tokio JoinSet rust example default"
            }),
        )
        .await
        .expect("codesearch default");
    let max_result = codesearch
        .call(
            test_context(workspace.workspace(), "codesearch-max"),
            json!({
                "query": "Tokio JoinSet rust example max",
                "tokensNum": 90_000,
            }),
        )
        .await
        .expect("codesearch max clamp");

    assert_eq!(
        min_result.display_text,
        "Tokio JoinSet examples\nspawn multiple tasks"
    );
    assert_eq!(min_result.display_text, default_result.display_text);
    assert_eq!(min_result.display_text, max_result.display_text);

    let min_json = min_result.structured_json.expect("min structured json");
    let default_json = default_result
        .structured_json
        .expect("default structured json");
    let max_json = max_result.structured_json.expect("max structured json");
    assert_eq!(min_json["tokensNum"], json!(1000));
    assert_eq!(default_json["tokensNum"], json!(5000));
    assert_eq!(max_json["tokensNum"], json!(50000));
    assert_eq!(min_json["empty"], json!(false));
    assert_eq!(default_json["empty"], json!(false));
    assert_eq!(max_json["empty"], json!(false));

    let requests = transport.requests();
    assert_eq!(
        requests.len(),
        3,
        "codesearch should hit the shared backend path for each request"
    );
    for (request, (query, tokens_num)) in requests.iter().zip([
        ("Tokio JoinSet rust example", 1000),
        ("Tokio JoinSet rust example default", 5000),
        ("Tokio JoinSet rust example max", 50000),
    ]) {
        assert_eq!(request.auth_token.as_deref(), Some("fixture-token"));
        assert_eq!(request.tool_name, "web_search_exa");
        assert_eq!(request.arguments["query"], json!(query));
        assert_eq!(request.arguments["tokensNum"], json!(tokens_num));
    }
}

#[tokio::test]
async fn native_code_search_handles_timeout_and_empty_context_cleanly() {
    let workspace = setup_workspace_fixture();

    let timeout_registry = search_registry(
        ScriptedRemoteSearchTransport::new(vec![Err(ToolError::Execution(
            "search.code request timed out after 1s".to_string(),
        ))]),
        RemoteSearchTestConfig {
            auth_token: Some("fixture-token".to_string()),
            require_auth: true,
            timeout_secs: 1,
            max_retries: 0,
            retry_backoff_ms: 1,
            ..RemoteSearchTestConfig::default()
        },
    );
    let timeout_error = timeout_registry
        .get("codesearch")
        .expect("codesearch tool")
        .call(
            test_context(workspace.workspace(), "timeout-code-search"),
            json!({
                "query": "Tokio JoinSet timeout"
            }),
        )
        .await
        .expect_err("timeout should fail deterministically");
    expect_execution_error(timeout_error, "Code search request timed out");

    let empty_registry = search_registry(
        ScriptedRemoteSearchTransport::ok(vec![RemoteSearchHttpResponse::new(
            200,
            "data: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"   \"}]}}\n\n",
        )]),
        RemoteSearchTestConfig {
            auth_token: None,
            require_auth: false,
            timeout_secs: 5,
            max_retries: 0,
            retry_backoff_ms: 1,
            ..RemoteSearchTestConfig::default()
        },
    );
    let empty_result = empty_registry
        .get("codesearch")
        .expect("codesearch tool")
        .call(
            test_context(workspace.workspace(), "empty-code-search"),
            json!({
                "query": "no matches fixture"
            }),
        )
        .await
        .expect("empty context should be handled");
    assert_eq!(empty_result.display_text, EMPTY_CODE_SEARCH_MESSAGE);
    let empty_json = empty_result.structured_json.expect("empty structured json");
    assert_eq!(empty_json["query"], json!("no matches fixture"));
    assert_eq!(empty_json["tokensNum"], json!(5000));
    assert_eq!(empty_json["empty"], json!(true));
}
