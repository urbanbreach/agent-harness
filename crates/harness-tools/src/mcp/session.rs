use super::*;

pub(super) struct Client {
    name: String,
    catalog: Weak<RwLock<BTreeMap<String, Arc<dyn Tool>>>>,
}
impl rmcp::ClientHandler for Client {
    async fn on_tool_list_changed(&self, _: rmcp::service::NotificationContext<RoleClient>) {
        if let Some(catalog) = self.catalog.upgrade() {
            if let Ok(mut catalog) = catalog.write() {
                catalog.clear();
            }
        }
        harness_core::config::update_registered_mcp_server_tools(&self.name, BTreeMap::new());
    }
}
impl Server {
    pub(super) async fn connect(&self, ctx: &ToolContext) -> Result<Session, ToolError> {
        let handler = Client {
            name: self.name.clone(),
            catalog: Weak::clone(&self.catalog),
        };
        match &self.config {
            McpServerConfig::Stdio {
                command, env, cwd, ..
            } => {
                #[cfg(unix)]
                {
                    let (program, args) = command
                        .split_first()
                        .filter(|(s, _)| !s.is_empty())
                        .ok_or_else(|| {
                            ToolError::InvalidArguments("MCP command is empty".into())
                        })?;
                    let mut process = tokio::process::Command::new(program);
                    process
                        .args(args)
                        .envs(env)
                        .current_dir(cwd.as_ref().map_or_else(
                            || ctx.workspace_root.clone(),
                            |p| ctx.workspace_root.join(p),
                        ));
                    handler
                        .serve(stdio::Process::start(process)?)
                        .await
                        .map_err(|_| failure("MCP stdio initialization failed"))
                }
                #[cfg(not(unix))]
                {
                    let _ = (command, env, cwd, ctx);
                    Err(failure(
                        "MCP process-tree control is unavailable on this platform",
                    ))
                }
            }
            McpServerConfig::Http {
                endpoint, headers, ..
            } => {
                let url = reqwest::Url::parse(endpoint)
                    .map_err(|_| ToolError::InvalidArguments("invalid MCP endpoint".into()))?;
                if !matches!(url.scheme(), "http" | "https")
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.fragment().is_some()
                {
                    return Err(ToolError::InvalidArguments(
                        "MCP endpoint must be HTTP(S) without embedded credentials or fragment"
                            .into(),
                    ));
                }
                let mut map = reqwest::header::HeaderMap::new();
                for (name, value) in headers {
                    let key = reqwest::header::HeaderName::try_from(name).map_err(|_| {
                        ToolError::InvalidArguments("invalid MCP header name".into())
                    })?;
                    if matches!(
                        key.as_str(),
                        "host"
                            | "content-length"
                            | "content-type"
                            | "accept"
                            | "mcp-session-id"
                            | "mcp-protocol-version"
                            | "connection"
                    ) {
                        return Err(ToolError::InvalidArguments("reserved MCP header".into()));
                    }
                    map.insert(
                        key,
                        reqwest::header::HeaderValue::try_from(value).map_err(|_| {
                            ToolError::InvalidArguments("invalid MCP header value".into())
                        })?,
                    );
                }
                let client = reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .retry(reqwest::retry::never())
                    .default_headers(map)
                    .connect_timeout(Duration::from_secs(10))
                    .read_timeout(Duration::from_secs(30))
                    .build()
                    .map_err(|_| failure("MCP HTTP client setup failed"))?;
                let transport = StreamableHttpClientTransport::with_client(
                    http::Client(client),
                    StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
                        .max_concurrent_requests(1)
                        .reinit_on_expired_session(false)
                        .max_sse_event_size(LIMIT),
                );
                handler.serve(transport).await.map_err(|error| match error {
                    rmcp::service::ClientInitializeError::TransportError { error, .. } => {
                        http::tool_error(error.error.as_ref())
                    }
                    _ => failure("MCP HTTP initialization failed"),
                })
            }
        }
    }
}
pub(super) async fn request(
    session: &Session,
    method: &str,
    params: Value,
    ctx: &ToolContext,
    deadline: Instant,
) -> Result<Value, ToolError> {
    let request: ClientRequest =
        serde_json::from_value(json!({"method":method,"params":params}))
            .map_err(|_| ToolError::InvalidArguments("invalid MCP request".into()))?;
    let mut handle = session
        .send_cancellable_request(request, PeerRequestOptions::no_options())
        .await
        .map_err(request_error)?;
    let interrupted = tokio::select! {
        biased;
        () = ctx.cancellation.cancelled() => ToolError::Cancelled,
        () = tokio::time::sleep_until(deadline) => failure("MCP request timed out"),
        result = &mut handle.rx => return serde_json::to_value(result.map_err(|_| failure("MCP connection closed"))?.map_err(request_error)?).map_err(|_| failure("invalid MCP response")),
    };
    let _ = tokio::time::timeout(
        Duration::from_secs(1),
        handle.cancel(Some("request interrupted".into())),
    )
    .await;
    Err(interrupted)
}

fn request_error(error: rmcp::ServiceError) -> ToolError {
    match error {
        rmcp::ServiceError::TransportSend(error) => http::tool_error(error.error.as_ref()),
        _ => failure("MCP request failed"),
    }
}
