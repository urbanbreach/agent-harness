use std::sync::Mutex;

use async_trait::async_trait;
use harness_core::tool::ToolError;
use harness_tools::{
    ShellCommandInvocation, ShellCommandRunner, ShellProcessOutput, WebFetchHttpRequest,
    WebFetchHttpResponse, WebFetchHttpTransport,
};

#[derive(Debug)]
pub(crate) struct SingleSurfaceShellRunner {
    calls: Mutex<Vec<ShellCommandInvocation>>,
}

impl SingleSurfaceShellRunner {
    pub(crate) fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ShellCommandRunner for SingleSurfaceShellRunner {
    async fn run(
        &self,
        invocation: ShellCommandInvocation,
        _timeout_ms: u64,
    ) -> Result<ShellProcessOutput, ToolError> {
        self.calls
            .lock()
            .expect("lock shell calls")
            .push(invocation.clone());
        let command = invocation
            .args
            .get(1)
            .map(String::as_str)
            .unwrap_or_default();
        let stdout = if command.contains("{1..10000}") {
            "surface".repeat(10_000)
        } else if command.contains("cargo surface") {
            "cargo surface\n".to_string()
        } else {
            return Err(ToolError::Execution(format!(
                "unexpected scripted shell command: {command}"
            )));
        };
        Ok(ShellProcessOutput {
            stdout,
            stderr: String::new(),
            status: 0,
            success: true,
        })
    }
}

#[derive(Debug)]
pub(crate) struct SingleSurfaceWebFetchTransport;

#[async_trait]
impl WebFetchHttpTransport for SingleSurfaceWebFetchTransport {
    async fn execute(
        &self,
        request: WebFetchHttpRequest,
    ) -> Result<WebFetchHttpResponse, ToolError> {
        let url = reqwest::Url::parse(&request.url)
            .map_err(|err| ToolError::InvalidArguments(format!("invalid URL: {err}")))?;
        if url.path() != "/fetch" {
            return Ok(WebFetchHttpResponse::new(
                404,
                [("Content-Type", "text/plain")],
                b"missing".to_vec(),
            ));
        }
        Ok(WebFetchHttpResponse::new(
            200,
            [("Content-Type", "text/plain")],
            b"hello fetch\n".to_vec(),
        ))
    }
}
