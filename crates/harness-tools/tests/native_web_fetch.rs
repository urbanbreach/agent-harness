use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::config::ShellAllowlist;
use harness_core::tool::ToolContext;
use harness_tools::coordinator_registry;
use serde_json::json;

mod common;

use common::{
    expect_execution_error, expect_invalid_arguments, setup_workspace_fixture,
    spawn_binary_http_server as spawn_http_server, test_context as common_test_context,
    TestBinaryResponse as TestResponse, TestRequest,
};

const MARKDOWN_ACCEPT: &str =
    "text/markdown;q=1.0, text/x-markdown;q=0.9, text/plain;q=0.8, text/html;q=0.7, */*;q=0.1";
const ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";
const PNG_BYTES: &[u8] = &[137, 80, 78, 71, 13, 10, 26, 10];
const PDF_BYTES: &[u8] = b"%PDF-1.7\n%test pdf\n";

fn test_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
    common_test_context(workspace_root, "run-native-web-fetch-tests", tool_call_id)
}

fn artifact_bytes(context: &ToolContext, artifact_path: &str) -> Vec<u8> {
    let relative = artifact_path
        .strip_prefix("artifacts/")
        .expect("artifact path prefix");
    fs::read(context.artifacts_dir.join(relative)).expect("artifact bytes")
}

#[tokio::test]
async fn native_web_fetch_supports_text_markdown_html_and_binary_artifacts() {
    let workspace = setup_workspace_fixture();
    let workspace_root = workspace.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let web_fetch = registry.get("webfetch").expect("webfetch tool");

    let requests = Arc::new(Mutex::new(Vec::<TestRequest>::new()));
    let counts = Arc::new(Mutex::new(BTreeMap::<String, usize>::new()));
    let request_log = Arc::clone(&requests);
    let request_counts = Arc::clone(&counts);
    let base_url = spawn_http_server(Arc::new(move |request| {
        request_log
            .lock()
            .expect("request log")
            .push(request.clone());
        let hit = {
            let mut guard = request_counts.lock().expect("request counts");
            let entry = guard.entry(request.path.clone()).or_insert(0);
            *entry += 1;
            *entry
        };

        match request.path.as_str() {
            "/plain" => TestResponse {
                status: "200 OK",
                headers: vec![(
                    "Content-Type".to_string(),
                    "text/plain; charset=utf-8".to_string(),
                )],
                body: b"hello text\n".to_vec(),
                delay: Duration::ZERO,
            },
            "/markdown" => TestResponse {
                status: "200 OK",
                headers: vec![(
                    "Content-Type".to_string(),
                    "text/markdown; charset=utf-8".to_string(),
                )],
                body: b"# Hello markdown\n\nBody\n".to_vec(),
                delay: Duration::ZERO,
            },
            "/html" => TestResponse {
                status: "200 OK",
                headers: vec![(
                    "Content-Type".to_string(),
                    "text/html; charset=utf-8".to_string(),
                )],
                body: b"<html><body><h1>Hello HTML</h1><p>Body text</p></body></html>".to_vec(),
                delay: Duration::ZERO,
            },
            "/cf" if hit == 1 => TestResponse {
                status: "403 Forbidden",
                headers: vec![
                    ("Content-Type".to_string(), "text/plain".to_string()),
                    ("cf-mitigated".to_string(), "challenge".to_string()),
                ],
                body: b"challenge".to_vec(),
                delay: Duration::ZERO,
            },
            "/cf" => TestResponse {
                status: "200 OK",
                headers: vec![(
                    "Content-Type".to_string(),
                    "text/html; charset=utf-8".to_string(),
                )],
                body: b"<html><body><h1>Retry Title</h1><p>Retry body</p></body></html>".to_vec(),
                delay: Duration::ZERO,
            },
            "/image" => TestResponse {
                status: "200 OK",
                headers: vec![(
                    "Content-Type".to_string(),
                    "image/png; charset=binary".to_string(),
                )],
                body: PNG_BYTES.to_vec(),
                delay: Duration::ZERO,
            },
            "/pdf" => TestResponse {
                status: "200 OK",
                headers: vec![("Content-Type".to_string(), "application/pdf".to_string())],
                body: PDF_BYTES.to_vec(),
                delay: Duration::ZERO,
            },
            _ => TestResponse {
                status: "404 Not Found",
                headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
                body: b"missing".to_vec(),
                delay: Duration::ZERO,
            },
        }
    }));

    let plain = web_fetch
        .call(
            test_context(workspace_root, "native-plain"),
            json!({
                "url": format!("{base_url}/plain"),
                "format": "text",
            }),
        )
        .await
        .expect("plain fetch");
    assert_eq!(plain.display_text, "hello text\n");
    assert!(plain.artifacts.is_empty());
    assert_eq!(
        plain.structured_json.expect("plain json")["response_kind"],
        json!("text")
    );

    let markdown = web_fetch
        .call(
            test_context(workspace_root, "native-markdown"),
            json!({
                "url": format!("{base_url}/markdown"),
                "format": "markdown",
            }),
        )
        .await
        .expect("markdown fetch");
    let repeated_markdown = web_fetch
        .call(
            test_context(workspace_root, "repeat-markdown"),
            json!({
                "url": format!("{base_url}/markdown"),
                "format": "markdown",
            }),
        )
        .await
        .expect("repeat markdown fetch");
    assert_eq!(markdown.display_text, "# Hello markdown\n\nBody\n");
    assert_eq!(markdown.display_text, repeated_markdown.display_text);
    assert_eq!(markdown.structured_json, repeated_markdown.structured_json);

    let html = web_fetch
        .call(
            test_context(workspace_root, "native-html"),
            json!({
                "url": format!("{base_url}/html"),
                "format": "html",
            }),
        )
        .await
        .expect("html fetch");
    assert_eq!(
        html.display_text,
        "<html><body><h1>Hello HTML</h1><p>Body text</p></body></html>"
    );
    assert!(html.artifacts.is_empty());

    let retry = web_fetch
        .call(
            test_context(workspace_root, "native-retry"),
            json!({
                "url": format!("{base_url}/cf"),
                "format": "markdown",
            }),
        )
        .await
        .expect("retry fetch");
    assert!(retry.display_text.contains("# Retry Title"));
    assert!(retry.display_text.contains("Retry body"));

    let image_ctx = test_context(workspace_root, "native-image");
    let image = web_fetch
        .call(
            image_ctx.clone(),
            json!({
                "url": format!("{base_url}/image"),
                "format": "markdown",
            }),
        )
        .await
        .expect("image fetch");
    assert!(image.display_text.contains("Fetched image artifact"));
    assert_eq!(image.artifacts.len(), 1);
    assert_eq!(
        artifact_bytes(&image_ctx, &image.artifacts[0].path),
        PNG_BYTES
    );
    let image_json = image.structured_json.expect("image json");
    assert_eq!(image_json["response_kind"], json!("artifact"));
    assert_eq!(image_json["artifact_kind"], json!("image"));
    assert_eq!(
        image_json["artifact"]["path"],
        json!(image.artifacts[0].path.clone())
    );

    let pdf_ctx = test_context(workspace_root, "native-pdf");
    let pdf = web_fetch
        .call(
            pdf_ctx.clone(),
            json!({
                "url": format!("{base_url}/pdf"),
                "format": "text",
            }),
        )
        .await
        .expect("pdf fetch");
    assert!(pdf.display_text.contains("Fetched pdf artifact"));
    assert_eq!(pdf.artifacts.len(), 1);
    assert_eq!(artifact_bytes(&pdf_ctx, &pdf.artifacts[0].path), PDF_BYTES);
    let pdf_json = pdf.structured_json.expect("pdf json");
    assert_eq!(pdf_json["response_kind"], json!("artifact"));
    assert_eq!(pdf_json["artifact_kind"], json!("pdf"));

    let cf_requests: Vec<_> = requests
        .lock()
        .expect("request log")
        .iter()
        .filter(|request| request.path == "/cf")
        .cloned()
        .collect();
    assert_eq!(cf_requests.len(), 2, "expected cf mitigation retry");
    assert_eq!(
        cf_requests[0].headers.get("accept").map(String::as_str),
        Some(MARKDOWN_ACCEPT)
    );
    assert_eq!(
        cf_requests[0]
            .headers
            .get("accept-language")
            .map(String::as_str),
        Some(ACCEPT_LANGUAGE)
    );
    assert!(cf_requests[0]
        .headers
        .get("user-agent")
        .expect("browser user-agent")
        .starts_with("Mozilla/5.0"));
    assert_eq!(
        cf_requests[1].headers.get("user-agent").map(String::as_str),
        Some("agent-harness")
    );
}

#[tokio::test]
async fn native_web_fetch_rejects_invalid_scheme_large_response_and_timeout() {
    let workspace = setup_workspace_fixture();
    let workspace_root = workspace.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let web_fetch = registry.get("webfetch").expect("webfetch tool");

    let base_url = spawn_http_server(Arc::new(|request| match request.path.as_str() {
        "/large" => TestResponse {
            status: "200 OK",
            headers: vec![
                ("Content-Type".to_string(), "application/pdf".to_string()),
                (
                    "Content-Length".to_string(),
                    (5 * 1024 * 1024 + 1).to_string(),
                ),
            ],
            body: Vec::new(),
            delay: Duration::ZERO,
        },
        "/slow" => TestResponse {
            status: "200 OK",
            headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
            body: b"too slow".to_vec(),
            delay: Duration::from_millis(1100),
        },
        _ => TestResponse {
            status: "404 Not Found",
            headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
            body: b"missing".to_vec(),
            delay: Duration::ZERO,
        },
    }));

    let invalid_scheme = web_fetch
        .call(
            test_context(workspace_root, "invalid-scheme"),
            json!({
                "url": "file:///tmp/secret.txt",
                "format": "text",
            }),
        )
        .await
        .expect_err("invalid scheme should fail");
    expect_invalid_arguments(invalid_scheme, "http:// or https://");

    let oversized = web_fetch
        .call(
            test_context(workspace_root, "oversized"),
            json!({
                "url": format!("{base_url}/large"),
                "format": "markdown",
            }),
        )
        .await
        .expect_err("oversized response should fail");
    expect_execution_error(oversized, "response too large");

    let timed_out = web_fetch
        .call(
            test_context(workspace_root, "timed-out"),
            json!({
                "url": format!("{base_url}/slow"),
                "format": "text",
                "timeout": 0,
            }),
        )
        .await
        .expect_err("slow response should time out");
    expect_execution_error(timed_out, "timed out after 1s");
}
