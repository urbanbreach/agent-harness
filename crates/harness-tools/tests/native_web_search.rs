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

fn test_context(
    workspace_root: &std::path::Path,
    tool_call_id: &str,
) -> harness_core::tool::ToolContext {
    common_test_context(workspace_root, "run-native-web-search-tests", tool_call_id)
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide search env mutation across awaits"
)]
async fn native_web_search_uses_shared_client_and_fixture_backend() {
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
                "application/json; charset=utf-8".to_string(),
            )],
            body: json!({
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
            delay: Duration::ZERO,
        }
    }));
    let _search_env = EnvGuard::set(&[
        (remote_search_env::ENDPOINT, Some(base_url.as_str())),
        (remote_search_env::AUTH_TOKEN, Some("fixture-token")),
        (remote_search_env::REQUIRE_AUTH, Some("1")),
        (remote_search_env::TIMEOUT_SECS, Some("1")),
        (remote_search_env::MAX_RETRIES, Some("0")),
        (remote_search_env::RETRY_BACKOFF_MS, Some("1")),
    ]);

    let registry = coordinator_registry(ShellAllowlist::default());
    let websearch = registry.get("websearch").expect("websearch tool");

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
        .expect("websearch first");
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
        .expect("websearch second");

    assert_eq!(
        first_result.display_text,
        "Tokio runtime docs\nJoinSet guide"
    );
    assert_eq!(first_result.display_text, second_result.display_text);
    assert_eq!(first_result.structured_json, second_result.structured_json);
    let result_json = first_result.structured_json.expect("structured json");
    assert_eq!(result_json["query"], json!("tokio runtime"));
    assert_eq!(result_json["numResults"], json!(2));
    assert_eq!(result_json["livecrawl"], json!("preferred"));
    assert_eq!(result_json["type"], json!("fast"));
    assert_eq!(result_json["contextMaxCharacters"], json!(4096));
    assert_eq!(result_json["empty"], json!(false));

    let requests = requests.lock().expect("request log");
    assert_eq!(
        requests.len(),
        2,
        "websearch should hit the shared backend path for each request"
    );
    for request in requests.iter() {
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
        assert_eq!(
            payload["params"]["arguments"]["query"],
            json!("tokio runtime")
        );
        assert_eq!(payload["params"]["arguments"]["numResults"], json!(2));
        assert_eq!(
            payload["params"]["arguments"]["livecrawl"],
            json!("preferred")
        );
        assert_eq!(payload["params"]["arguments"]["type"], json!("fast"));
        assert_eq!(
            payload["params"]["arguments"]["contextMaxCharacters"],
            json!(4096)
        );
    }
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide search env mutation across awaits"
)]
async fn native_web_search_handles_missing_auth_rate_limit_and_empty_results() {
    let _env_guard = env_test_lock();
    let workspace = setup_workspace_fixture();

    let _missing_auth_env = EnvGuard::set(&[
        (remote_search_env::ENDPOINT, Some("http://127.0.0.1:9")),
        (remote_search_env::AUTH_TOKEN, None),
        (remote_search_env::REQUIRE_AUTH, Some("1")),
        (remote_search_env::TIMEOUT_SECS, Some("1")),
        (remote_search_env::MAX_RETRIES, Some("0")),
        (remote_search_env::RETRY_BACKOFF_MS, Some("1")),
    ]);
    let missing_auth_registry = coordinator_registry(ShellAllowlist::default());
    let missing_auth = missing_auth_registry
        .get("websearch")
        .expect("websearch tool")
        .call(
            test_context(workspace.workspace(), "missing-auth"),
            json!({
                "query": "tokio runtime"
            }),
        )
        .await
        .expect_err("missing auth should fail before network call");
    expect_execution_error(missing_auth, "authentication is not configured");
    drop(_missing_auth_env);

    let rate_limit_hits = Arc::new(Mutex::new(0_usize));
    let hit_counter = Arc::clone(&rate_limit_hits);
    let rate_limit_url = spawn_http_server(Arc::new(move |_request| {
        *hit_counter.lock().expect("rate limit hits") += 1;
        TestResponse {
            status: "429 Too Many Requests",
            headers: vec![
                ("Content-Type".to_string(), "text/plain".to_string()),
                ("Retry-After".to_string(), "1".to_string()),
            ],
            body: "rate limited".to_string(),
            delay: Duration::ZERO,
        }
    }));
    let _rate_limit_env = EnvGuard::set(&[
        (remote_search_env::ENDPOINT, Some(rate_limit_url.as_str())),
        (remote_search_env::AUTH_TOKEN, Some("fixture-token")),
        (remote_search_env::REQUIRE_AUTH, Some("1")),
        (remote_search_env::TIMEOUT_SECS, Some("1")),
        (remote_search_env::MAX_RETRIES, Some("1")),
        (remote_search_env::RETRY_BACKOFF_MS, Some("1")),
    ]);
    let rate_limit_registry = coordinator_registry(ShellAllowlist::default());
    let rate_limit_error = rate_limit_registry
        .get("websearch")
        .expect("websearch tool")
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
    assert_eq!(*rate_limit_hits.lock().expect("rate limit hits"), 2);
    drop(_rate_limit_env);

    let empty_url = spawn_http_server(Arc::new(move |_request| TestResponse {
        status: "200 OK",
        headers: vec![(
            "Content-Type".to_string(),
            "text/event-stream; charset=utf-8".to_string(),
        )],
        body: "data: {\"result\":{\"content\":[]}}\n\n".to_string(),
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
        .get("websearch")
        .expect("websearch tool")
        .call(
            test_context(workspace.workspace(), "empty-results"),
            json!({
                "query": "no matches fixture"
            }),
        )
        .await
        .expect("empty results should be handled");
    assert_eq!(empty_result.display_text, "No search results found");
    let empty_json = empty_result.structured_json.expect("empty structured json");
    assert_eq!(empty_json["numResults"], json!(8));
    assert_eq!(empty_json["livecrawl"], json!("fallback"));
    assert_eq!(empty_json["type"], json!("auto"));
    assert_eq!(empty_json["empty"], json!(true));
}
