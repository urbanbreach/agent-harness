mod http;
mod session;
use session::request;
mod catalog;
mod media;
mod remote_search;
pub use remote_search::register_remote_search_tools;
#[cfg(unix)]
mod stdio;
use harness_core::{
    config::{McpConfig, McpServerConfig},
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult},
};
use rmcp::{
    model::ClientRequest,
    service::{PeerRequestOptions, RunningService},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    RoleClient, ServiceExt,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, RwLock, Weak},
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};
const LIMIT: usize = 1_048_576;
type Session = RunningService<RoleClient, session::Client>;
const METHODS: [(&str, &str, &str); 6] = [
    (
        "tools.list",
        "tools/list",
        "List the server's tools and argument schemas.",
    ),
    (
        "tool.call",
        "tools/call",
        "Call a server tool by name using its discovered argument schema.",
    ),
    (
        "resources.list",
        "resources/list",
        "List the server's resources.",
    ),
    (
        "resource.read",
        "resources/read",
        "Read a resource URI from the server.",
    ),
    (
        "prompts.list",
        "prompts/list",
        "List the server's prompt templates.",
    ),
    (
        "prompt.get",
        "prompts/get",
        "Load a server prompt template with arguments.",
    ),
];

pub fn register_mcp_tools(registry: &mut ToolRegistry, config: McpConfig) -> Result<(), ToolError> {
    for (name, config) in config.servers {
        if !config.enabled() {
            continue;
        }
        if name.is_empty()
            || name.len() > 80
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(ToolError::InvalidArguments(
                "MCP server IDs must contain 1–80 ASCII letters, numbers, underscores or hyphens"
                    .into(),
            ));
        }
        let catalog = Arc::new(RwLock::new(BTreeMap::new()));
        harness_core::config::update_registered_mcp_server_connection(&name, None);
        registry.register_catalog(format!("mcp.{name}.tool.call"), Arc::clone(&catalog));
        let server = Arc::new(Server {
            name: name.clone(),
            config,
            sessions: Mutex::new(BTreeMap::new()),
            catalog: Arc::downgrade(&catalog),
        });
        for (suffix, method, description) in METHODS {
            registry.register(Arc::new(McpTool {
                id: format!("mcp.{name}.{suffix}"),
                method,
                description,
                server: Arc::clone(&server),
            }));
        }
    }
    Ok(())
}
struct Server {
    name: String,
    config: McpServerConfig,
    catalog: Weak<RwLock<BTreeMap<String, Arc<dyn Tool>>>>,
    // ponytail: one in-flight request per configured server; multiplex only if measured throughput needs it.
    sessions: Mutex<BTreeMap<String, Session>>,
}
struct McpTool {
    id: String,
    method: &'static str,
    description: &'static str,
    server: Arc<Server>,
}
#[async_trait::async_trait]
impl Tool for McpTool {
    fn id(&self) -> &str {
        &self.id
    }
    fn description(&self) -> &str {
        self.description
    }
    fn capability(&self) -> ToolCapability {
        match self.server.config {
            McpServerConfig::Stdio { .. } => ToolCapability::Shell,
            McpServerConfig::Http { .. } => ToolCapability::Network,
        }
    }
    fn secret_values(&self) -> Vec<String> {
        let values = match &self.server.config {
            McpServerConfig::Stdio { env, .. } => env,
            McpServerConfig::Http { headers, .. } => headers,
        };
        values.values().cloned().collect()
    }
    fn parameters_json_schema(&self) -> Value {
        let (properties, required) = match self.method {
            "tools/call" => (
                json!({"tool":{"type":"string"},"arguments":{"type":"object"}}),
                vec!["tool"],
            ),
            "resources/read" => (json!({"uri":{"type":"string"}}), vec!["uri"]),
            "prompts/get" => (
                json!({"name":{"type":"string"},"arguments":{"type":"object","additionalProperties":{"type":"string"}}}),
                vec!["name"],
            ),
            _ => (json!({}), vec![]),
        };
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
    }
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        let mut requests = vec![(
            self.id.clone(),
            args.get("tool")
                .or_else(|| args.get("uri"))
                .or_else(|| args.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("*")
                .into(),
        )];
        if self.method == "tools/call"
            && let Some(name) = args["tool"].as_str()
        {
            requests.push((
                catalog::tool_id(&self.server.name, name),
                args.get("arguments").unwrap_or(&Value::Null).to_string(),
            ));
        }
        requests
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let mut params = self.params(args)?;
        let timeout = match self.server.config {
            McpServerConfig::Stdio { timeout_secs, .. }
            | McpServerConfig::Http { timeout_secs, .. } => timeout_secs,
        };
        if !(1..=300).contains(&timeout) {
            return Err(ToolError::InvalidArguments(
                "MCP timeout must be 1–300 seconds".into(),
            ));
        }
        let deadline = Instant::now() + Duration::from_secs(timeout);
        let mut sessions = tokio::select! {
            () = ctx.cancellation.cancelled() => return Err(ToolError::Cancelled),
            result = tokio::time::timeout_at(deadline, self.server.sessions.lock()) => result.map_err(|_| failure("MCP server is busy"))?,
        };
        if sessions
            .get(&ctx.run_id)
            .is_some_and(|s| s.is_closed() || s.is_transport_closed())
        {
            sessions.remove(&ctx.run_id);
        }
        if !sessions.contains_key(&ctx.run_id) {
            let session = tokio::select! {
                () = ctx.cancellation.cancelled() => return Err(ToolError::Cancelled),
                result = tokio::time::timeout_at(deadline, self.server.connect(&ctx)) => result.map_err(|_| failure("MCP initialization timed out")).and_then(std::convert::identity),
            };
            let state = match &session {
                Ok(_) => harness_core::config::McpServerConnectionState::Connected,
                Err(error) => {
                    harness_core::config::McpServerConnectionState::Failed(error.to_string())
                }
            };
            harness_core::config::update_registered_mcp_server_connection(
                &self.server.name,
                Some(state),
            );
            sessions.insert(ctx.run_id.clone(), session?);
        }
        let session = sessions
            .get(&ctx.run_id)
            .ok_or_else(|| failure("MCP session is unavailable"))?;
        let list = self.method.strip_suffix("/list");
        let mut items = Vec::new();
        let mut cursors = BTreeSet::new();
        let mut bytes = 0;
        let mut result = loop {
            let mut result = request(session, self.method, params.clone(), &ctx, deadline).await?;
            bytes += result.to_string().len();
            if bytes > LIMIT {
                return Err(failure("MCP result exceeds 1 MiB"));
            }
            let Some(key) = list else {
                break result;
            };
            let page = result[key]
                .as_array_mut()
                .ok_or_else(|| failure("MCP list is malformed"))?;
            items.append(page);
            if items.len() > 1024 {
                return Err(failure("MCP list exceeds 1024 entries"));
            }
            let cursor = result.get("nextCursor").and_then(Value::as_str);
            match cursor {
                None => break json!({key:items}),
                Some(cursor) if cursors.len() < 32 && cursors.insert(cursor.to_owned()) => {
                    params["cursor"] = cursor.into();
                }
                Some(_) => return Err(failure("MCP pagination repeated or exceeded 32 pages")),
            }
        };
        let attachments = media::extract(&mut result, ctx.tool_call_id.as_ref())?;
        omit_media(&mut result);
        if self.method == "tools/list" {
            catalog::publish(&self.server, &result)?;
        }
        let is_error = result["isError"].as_bool().unwrap_or(false);
        let display = render(&result);
        Ok(
            ToolResult::structured(display, json!({"result":result,"is_error":is_error}))
                .with_attachments(attachments),
        )
    }
    async fn close_run(&self, run: &str) -> Result<(), ToolError> {
        let mut sessions = self.server.sessions.lock().await;
        if let Some(mut session) = sessions.remove(run) {
            if sessions.is_empty() {
                harness_core::config::update_registered_mcp_server_connection(
                    &self.server.name,
                    None,
                );
            }
            if session
                .close_with_timeout(Duration::from_secs(2))
                .await
                .map_err(|_| failure("MCP shutdown failed"))?
                .is_none()
            {
                return Err(failure("MCP shutdown timed out"));
            }
        }
        Ok(())
    }
}
impl McpTool {
    fn params(&self, args: Value) -> Result<Value, ToolError> {
        let mut args = args
            .as_object()
            .cloned()
            .ok_or_else(|| ToolError::InvalidArguments("MCP arguments must be an object".into()))?;
        let allowed: &[&str] = match self.method {
            "tools/call" => &["tool", "arguments"],
            "resources/read" => &["uri"],
            "prompts/get" => &["name", "arguments"],
            _ => &[],
        };
        if args.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err(ToolError::InvalidArguments("unknown MCP argument".into()));
        }
        if let Some(key) = allowed.first() {
            let text = args
                .get(*key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty() && s.len() <= 4096);
            if text.is_none() {
                return Err(ToolError::InvalidArguments(format!(
                    "MCP {key} must be nonempty text of at most 4096 bytes"
                )));
            }
        }
        if self.method == "tools/call"
            && let Some(tool) = args.remove("tool")
        {
            args.insert("name".into(), tool);
        }
        if let Some(arguments) = args.get("arguments") {
            let object = arguments.as_object().ok_or_else(|| {
                ToolError::InvalidArguments("MCP arguments must be an object".into())
            })?;
            if self.method == "prompts/get" && object.values().any(|v| !v.is_string()) {
                return Err(ToolError::InvalidArguments(
                    "MCP prompt arguments must be strings".into(),
                ));
            }
        }
        Ok(Value::Object(args))
    }
}
fn failure(text: &str) -> ToolError {
    ToolError::Execution(text.into())
}
fn omit_media(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(omit_media),
        Value::Object(object) => {
            if matches!(
                object.get("type").and_then(Value::as_str),
                Some("image" | "audio")
            ) {
                object.remove("data");
                object.insert("omitted".into(), true.into());
            }
            if object.contains_key("blob") {
                object.remove("blob");
                object.insert("omitted".into(), true.into());
            }
            object.values_mut().for_each(omit_media);
        }
        _ => {}
    }
}
fn render(value: &Value) -> String {
    match value {
        Value::Array(values) => values
            .iter()
            .map(render)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(object) => {
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                return text.into();
            }
            if let Some(name) = object.get("name").and_then(Value::as_str) {
                let mut text = format!(
                    "{name}: {}",
                    object
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                );
                for key in ["uri", "inputSchema", "arguments"] {
                    if let Some(value) = object.get(key) {
                        text.push_str(&format!("\n{key}: {}", render(value)));
                    }
                }
                return text;
            }
            for key in [
                "content",
                "contents",
                "messages",
                "tools",
                "resources",
                "prompts",
            ] {
                if let Some(value) = object.get(key) {
                    let text = render(value);
                    if !text.is_empty() {
                        return object
                            .get("role")
                            .and_then(Value::as_str)
                            .map_or_else(|| text.clone(), |role| format!("{role}: {text}"));
                    }
                }
            }
            if let Some(structured) = object.get("structuredContent") {
                return structured.to_string();
            }
            value.to_string()
        }
        _ => value.to_string(),
    }
}
