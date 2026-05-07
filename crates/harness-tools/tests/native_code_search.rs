use std::sync::{Arc, Mutex};
use std::time::Duration;

mod common;

use common::{
    env_test_lock, expect_execution_error, remote_search_env, setup_workspace_fixture,
    spawn_http_server, test_context as common_test_context, EnvGuard, TestRequest, TestResponse,
};
use harness_core::config::ShellAllowlist;
use harness_tools::coordinator_registry;
use serde_json::{json, Value};

const EMPTY_CODE_SEARCH_MESSAGE: &str = "No code snippets or documentation found. Please try a different query, be more specific about the library or programming concept, or check the spelling of framework names.";

fn test_context(
    workspace_root: &std::path::Path,
    tool_call_id: &str,
) -> harness_core::tool::ToolContext {
    common_test_context(workspace_root, "run-native-code-search-tests", tool_call_id)
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide search env mutation across awaits"
)]
async fn native_code_search_uses_shared_client_and_respects_tokens_contract() {
    let _env_guard = env_test_lock();
    let workspace = setup_workspace_fixture();
    let requests = Arc::new(Mutex::new(Vec::<TestRequest>::new()));
    let request_log = Arc::clone(&requests);
    let base_url = spawn_http_server(Arc::new(move |request| {
        request_log.lock().expect("request log").push(request);
        TestResponse {
            status: "200 OK",
            headers: vec![(
                "Content-Type".to_string(),
                "text/event-stream; charset=utf-8".to_string(),
            )],
            body: "data: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"Tokio JoinSet examples\\nspawn multiple tasks\"}]}}\n\n".to_string(),
            delay: Duration::ZERO,
        }
    }));
    let _search_env = EnvGuard::set(&[
        (remote_search_env::ENDPOINT, Some(base_url.as_str())),
        (remote_search_env::AUTH_TOKEN, Some("fixture-token")),
        (remote_search_env::REQUIRE_AUTH, Some("1")),
        (remote_search_env::TIMEOUT_SECS, Some("5")),
        (remote_search_env::MAX_RETRIES, Some("0")),
        (remote_search_env::RETRY_BACKOFF_MS, Some("1")),
    ]);

    let registry = coordinator_registry(ShellAllowlist::default());
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

    let requests = requests.lock().expect("request log");
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
        assert_eq!(request.path, "/");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer fixture-token")
        );
        assert_eq!(
            request.headers.get("accept").map(String::as_str),
            Some("application/json, text/event-stream")
        );
        let payload: Value = serde_json::from_str(&request.body).expect("jsonrpc payload");
        assert_eq!(payload["method"], json!("tools/call"));
        assert_eq!(payload["params"]["name"], json!("web_search_exa"));
        assert_eq!(payload["params"]["arguments"]["query"], json!(query));
        assert_eq!(
            payload["params"]["arguments"]["tokensNum"],
            json!(tokens_num)
        );
    }
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide search env mutation across awaits"
)]
async fn native_code_search_handles_timeout_and_empty_context_cleanly() {
    let _env_guard = env_test_lock();
    let workspace = setup_workspace_fixture();

    let timeout_url = spawn_http_server(Arc::new(move |_request| TestResponse {
        status: "200 OK",
        headers: vec![(
            "Content-Type".to_string(),
            "text/event-stream; charset=utf-8".to_string(),
        )],
        body: "data: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"slow result\"}]}}\n\n"
            .to_string(),
        delay: Duration::from_secs(2),
    }));
    let _timeout_env = EnvGuard::set(&[
        (remote_search_env::ENDPOINT, Some(timeout_url.as_str())),
        (remote_search_env::AUTH_TOKEN, Some("fixture-token")),
        (remote_search_env::REQUIRE_AUTH, Some("1")),
        (remote_search_env::TIMEOUT_SECS, Some("1")),
        (remote_search_env::MAX_RETRIES, Some("0")),
        (remote_search_env::RETRY_BACKOFF_MS, Some("1")),
    ]);
    let timeout_registry = coordinator_registry(ShellAllowlist::default());
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
    drop(_timeout_env);

    let empty_url = spawn_http_server(Arc::new(move |_request| TestResponse {
        status: "200 OK",
        headers: vec![(
            "Content-Type".to_string(),
            "text/event-stream; charset=utf-8".to_string(),
        )],
        body: "data: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"   \"}]}}\n\n"
            .to_string(),
        delay: Duration::ZERO,
    }));
    let _empty_env = EnvGuard::set(&[
        (remote_search_env::ENDPOINT, Some(empty_url.as_str())),
        (remote_search_env::AUTH_TOKEN, None),
        (remote_search_env::REQUIRE_AUTH, Some("0")),
        (remote_search_env::TIMEOUT_SECS, Some("5")),
        (remote_search_env::MAX_RETRIES, Some("0")),
        (remote_search_env::RETRY_BACKOFF_MS, Some("1")),
    ]);
    let empty_registry = coordinator_registry(ShellAllowlist::default());
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
