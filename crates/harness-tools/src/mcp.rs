use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use async_trait::async_trait;
use harness_core::config::{
    set_registered_mcp_server_connection_states, set_registered_mcp_server_first_class_tool_ids,
    McpConfig, McpServerConfig, McpServerConnectionState,
};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use reqwest::StatusCode;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
const MCP_SESSION_ID_HEADER: &str = "mcp-session-id";
const MCP_PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";

pub(crate) fn register_mcp_tools(registry: &mut ToolRegistry, config: McpConfig) {
    if config.servers.is_empty() {
        set_registered_mcp_server_connection_states(BTreeMap::new());
        set_registered_mcp_server_first_class_tool_ids(BTreeMap::new());
        return;
    }

    let http_client = build_http_client();
    let mut connection_states = BTreeMap::new();
    let mut first_class_tool_ids = BTreeMap::new();
    for (server_id, server_config) in config.servers {
        if !server_config.enabled() {
            continue;
        }

        let executor = std::sync::Arc::new(McpServerExecutor::new(
            server_id,
            server_config,
            http_client.clone(),
        ));
        for kind in McpToolKind::all() {
            registry.register(std::sync::Arc::new(McpServerTool::new(
                executor.clone(),
                kind,
            )));
        }

        let (discovered_tools, discovered_tool_ids, connection_state) =
            discover_first_class_tools(executor.clone());
        connection_states.insert(executor.server_id.clone(), connection_state);
        first_class_tool_ids.insert(executor.server_id.clone(), discovered_tool_ids);
        for tool in discovered_tools {
            registry.register(std::sync::Arc::new(tool));
        }
    }

    set_registered_mcp_server_connection_states(connection_states);
    set_registered_mcp_server_first_class_tool_ids(first_class_tool_ids);
}

fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

fn discover_first_class_tools(
    executor: std::sync::Arc<McpServerExecutor>,
) -> (
    Vec<McpDiscoveredTool>,
    BTreeMap<String, String>,
    McpServerConnectionState,
) {
    let discovery_executor = executor.clone();
    let discovery = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| {
                ToolError::Execution(format!("failed to build MCP discovery runtime: {err}"))
            })?;
        runtime.block_on(async move { discovery_executor.discover_tools().await })
    })
    .join();

    match discovery {
        Ok(Ok(specs)) => {
            let discovered_tool_ids = specs
                .iter()
                .map(|spec| (spec.remote_tool_name.clone(), spec.tool_id.clone()))
                .collect::<BTreeMap<_, _>>();
            let tools = specs
                .into_iter()
                .map(|spec| McpDiscoveredTool::new(spec, executor.clone()))
                .collect::<Vec<_>>();
            (
                tools,
                discovered_tool_ids,
                McpServerConnectionState::Connected,
            )
        }
        Ok(Err(err)) => (
            Vec::new(),
            BTreeMap::new(),
            McpServerConnectionState::Failed(err.to_string()),
        ),
        Err(_) => (
            Vec::new(),
            BTreeMap::new(),
            McpServerConnectionState::Failed("MCP discovery task panicked".to_string()),
        ),
    }
}

fn normalize_provider_parameters_schema(schema: Option<Value>) -> Value {
    let Some(Value::Object(mut object)) = schema else {
        return serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": true,
        });
    };

    if object.get("type").and_then(Value::as_str) != Some("object") {
        return serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": true,
        });
    }

    for forbidden in ["oneOf", "anyOf", "allOf", "enum", "not"] {
        object.remove(forbidden);
    }
    object
        .entry("properties".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    object
        .entry("additionalProperties".to_string())
        .or_insert(Value::Bool(true));
    Value::Object(object)
}

fn sanitize_mcp_tool_segment(name: &str) -> String {
    let sanitized = harness_core::tool::sanitize_tool_function_name(name);
    sanitized.replace('-', "_")
}

fn reserved_mcp_tool_segments() -> BTreeSet<String> {
    McpToolKind::all()
        .map(|kind| sanitize_mcp_tool_segment(kind.suffix()))
        .collect()
}

#[derive(Debug, Clone, Copy)]
enum McpToolKind {
    ToolsList,
    ToolCall,
    ResourcesList,
    ResourceRead,
    PromptsList,
    PromptGet,
}

impl McpToolKind {
    const ALL: [Self; 6] = [
        Self::ToolsList,
        Self::ToolCall,
        Self::ResourcesList,
        Self::ResourceRead,
        Self::PromptsList,
        Self::PromptGet,
    ];

    fn all() -> impl Iterator<Item = Self> {
        Self::ALL.into_iter()
    }

    fn suffix(self) -> &'static str {
        match self {
            Self::ToolsList => "tools.list",
            Self::ToolCall => "tool.call",
            Self::ResourcesList => "resources.list",
            Self::ResourceRead => "resource.read",
            Self::PromptsList => "prompts.list",
            Self::PromptGet => "prompt.get",
        }
    }

    fn description(self, server_id: &str) -> String {
        match self {
            Self::ToolsList => {
                format!("Lists tools exposed by configured MCP server `{server_id}`.")
            }
            Self::ToolCall => format!(
                "Calls a named tool exposed by configured MCP server `{server_id}`. Use `mcp.{server_id}.tools.list` to discover tool names first."
            ),
            Self::ResourcesList => {
                format!("Lists resources exposed by configured MCP server `{server_id}`.")
            }
            Self::ResourceRead => format!(
                "Reads a resource URI exposed by configured MCP server `{server_id}`. Use `mcp.{server_id}.resources.list` to discover URIs first."
            ),
            Self::PromptsList => {
                format!("Lists prompts exposed by configured MCP server `{server_id}`.")
            }
            Self::PromptGet => format!(
                "Loads a prompt exposed by configured MCP server `{server_id}`. Use `mcp.{server_id}.prompts.list` to discover prompt names first."
            ),
        }
    }
    fn parameters_schema(self) -> Value {
        match self {
            Self::ToolsList | Self::ResourcesList | Self::PromptsList => {
                crate::json_schema_for::<EmptyArgs>()
            }
            Self::ToolCall => crate::json_schema_for::<McpToolCallArgs>(),
            Self::ResourceRead => crate::json_schema_for::<McpResourceReadArgs>(),
            Self::PromptGet => crate::json_schema_for::<McpPromptGetArgs>(),
        }
    }
}

struct McpServerTool {
    tool_id: String,
    kind: McpToolKind,
    executor: std::sync::Arc<McpServerExecutor>,
}

struct McpDiscoveredTool {
    tool_id: String,
    remote_tool_name: String,
    description: String,
    parameters_schema: Value,
    executor: std::sync::Arc<McpServerExecutor>,
}

impl McpDiscoveredTool {
    fn new(spec: DiscoveredMcpToolSpec, executor: std::sync::Arc<McpServerExecutor>) -> Self {
        Self {
            tool_id: spec.tool_id,
            remote_tool_name: spec.remote_tool_name,
            description: spec.description,
            parameters_schema: spec.parameters_schema,
            executor,
        }
    }
}

impl McpServerTool {
    fn new(executor: std::sync::Arc<McpServerExecutor>, kind: McpToolKind) -> Self {
        Self {
            tool_id: format!("mcp.{}.{}", executor.server_id, kind.suffix()),
            kind,
            executor,
        }
    }
}

#[async_trait]
impl Tool for McpServerTool {
    fn id(&self) -> &str {
        &self.tool_id
    }

    fn description(&self) -> &str {
        self.executor.description_for(self.kind)
    }

    fn parameters_json_schema(&self) -> Value {
        self.kind.parameters_schema()
    }

    fn capability(&self) -> ToolCapability {
        self.executor.capability()
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        match self.kind {
            McpToolKind::ToolsList => {
                let _: EmptyArgs = serde_json::from_value(args_json)
                    .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
                self.executor.list_tools().await
            }
            McpToolKind::ToolCall => {
                let args: McpToolCallArgs = serde_json::from_value(args_json)
                    .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
                self.executor.call_tool(&ctx, args).await
            }
            McpToolKind::ResourcesList => {
                let _: EmptyArgs = serde_json::from_value(args_json)
                    .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
                self.executor.list_resources().await
            }
            McpToolKind::ResourceRead => {
                let args: McpResourceReadArgs = serde_json::from_value(args_json)
                    .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
                self.executor.read_resource(args).await
            }
            McpToolKind::PromptsList => {
                let _: EmptyArgs = serde_json::from_value(args_json)
                    .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
                self.executor.list_prompts().await
            }
            McpToolKind::PromptGet => {
                let args: McpPromptGetArgs = serde_json::from_value(args_json)
                    .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
                self.executor.get_prompt(args).await
            }
        }
    }
}

#[async_trait]
impl Tool for McpDiscoveredTool {
    fn id(&self) -> &str {
        &self.tool_id
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_json_schema(&self) -> Value {
        self.parameters_schema.clone()
    }

    fn capability(&self) -> ToolCapability {
        self.executor.capability()
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        self.executor
            .call_named_tool(&ctx, &self.remote_tool_name, args_json)
            .await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct McpToolCallArgs {
    tool: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct McpResourceReadArgs {
    uri: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct McpPromptGetArgs {
    name: String,
    #[serde(default)]
    arguments: Value,
}

struct McpServerExecutor {
    server_id: String,
    server_config: McpServerConfig,
    http_client: reqwest::Client,
    descriptions: BTreeMap<&'static str, String>,
    stdio_session: Option<std::sync::Arc<tokio::sync::Mutex<Option<StdioMcpSession>>>>,
}

#[derive(Debug, Clone)]
struct DiscoveredMcpToolSpec {
    tool_id: String,
    remote_tool_name: String,
    description: String,
    parameters_schema: Value,
}

#[derive(Debug, Clone)]
struct PendingDiscoveredMcpToolSpec {
    base_segment: String,
    remote_tool_name: String,
    description: String,
    parameters_schema: Value,
}

impl McpServerExecutor {
    fn new(
        server_id: String,
        server_config: McpServerConfig,
        http_client: reqwest::Client,
    ) -> Self {
        let stdio_session = matches!(&server_config, McpServerConfig::Stdio { .. })
            .then(|| std::sync::Arc::new(tokio::sync::Mutex::new(None)));
        let descriptions = McpToolKind::all()
            .map(|kind| (kind.suffix(), kind.description(&server_id)))
            .collect();
        Self {
            server_id,
            server_config,
            http_client,
            descriptions,
            stdio_session,
        }
    }

    fn description_for(&self, kind: McpToolKind) -> &str {
        self.descriptions
            .get(kind.suffix())
            .map(String::as_str)
            .unwrap_or("MCP tool")
    }

    fn capability(&self) -> ToolCapability {
        match &self.server_config {
            McpServerConfig::Stdio { .. } => ToolCapability::Shell,
            McpServerConfig::Http { .. } => ToolCapability::Network,
        }
    }

    fn transport_name(&self) -> &'static str {
        match &self.server_config {
            McpServerConfig::Stdio { .. } => "stdio",
            McpServerConfig::Http { .. } => "http",
        }
    }

    async fn list_tools(&self) -> Result<ToolResult, ToolError> {
        let list = self.list_items("tools/list", "tools").await?;
        let display_text = render_list_output(
            &format!("MCP tools from {}", self.server_id),
            &list.items,
            |item| {
                let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
                let description = item
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("No description");
                format!("{name} — {description}")
            },
        );
        Ok(ToolResult {
            display_text,
            structured_json: Some(self.wrap_result(json!({ "tools": list.items }), &list.metadata)),
            artifacts: Vec::new(),
        })
    }

    async fn call_tool(
        &self,
        _ctx: &ToolContext,
        args: McpToolCallArgs,
    ) -> Result<ToolResult, ToolError> {
        self.call_named_tool(_ctx, &args.tool, args.arguments).await
    }

    async fn call_named_tool(
        &self,
        _ctx: &ToolContext,
        tool_name: &str,
        arguments: Value,
    ) -> Result<ToolResult, ToolError> {
        let tool_name = tool_name.to_string();
        let arguments = normalize_object_value(arguments);
        let (result, metadata) = if let Some(cache) = &self.stdio_session {
            self.request_via_stdio(
                cache,
                "tools/call",
                json!({
                    "name": tool_name.clone(),
                    "arguments": arguments.clone(),
                }),
            )
            .await?
        } else {
            let mut session = self.start_session().await?;
            let payload = session
                .request(
                    "tools/call",
                    json!({
                        "name": tool_name.clone(),
                        "arguments": arguments.clone(),
                    }),
                )
                .await;
            let metadata = session.metadata().clone();
            let close_result = session.close().await;
            let result = payload?;
            close_result?;
            (result, metadata)
        };

        let rendered = render_content_entries(result.get("content").and_then(Value::as_array));
        let display_text = if rendered.is_empty() {
            "MCP tool returned no content".to_string()
        } else {
            rendered.join("\n")
        };
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(ToolError::Execution(display_text));
        }

        Ok(ToolResult {
            display_text,
            structured_json: Some(self.wrap_result(
                json!({
                    "tool": tool_name,
                    "arguments": arguments,
                    "result": result,
                }),
                &metadata,
            )),
            artifacts: Vec::new(),
        })
    }

    async fn discover_tools(&self) -> Result<Vec<DiscoveredMcpToolSpec>, ToolError> {
        let list = self.list_items_ephemeral("tools/list", "tools").await?;
        let mut pending_specs = Vec::new();

        for item in list.items {
            let Some(remote_tool_name) = item
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
            else {
                continue;
            };

            let description = item
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| {
                    format!(
                        "Calls MCP tool `{remote_tool_name}` exposed by server `{}`.",
                        self.server_id
                    )
                });
            let parameters_schema =
                normalize_provider_parameters_schema(item.get("inputSchema").cloned());

            pending_specs.push(PendingDiscoveredMcpToolSpec {
                base_segment: sanitize_mcp_tool_segment(remote_tool_name),
                remote_tool_name: remote_tool_name.to_string(),
                description,
                parameters_schema,
            });
        }

        let mut used_ids = reserved_mcp_tool_segments();
        let mut assigned_segments = vec![String::new(); pending_specs.len()];
        let mut allocation_order = pending_specs
            .iter()
            .enumerate()
            .map(|(index, spec)| {
                (
                    index,
                    spec.base_segment.as_str(),
                    spec.remote_tool_name.as_str(),
                )
            })
            .collect::<Vec<_>>();
        allocation_order
            .sort_by(|left, right| left.1.cmp(right.1).then_with(|| left.2.cmp(right.2)));
        for (index, base_segment, _) in allocation_order {
            let mut tool_segment = base_segment.to_string();
            let mut suffix = 2usize;
            while !used_ids.insert(tool_segment.clone()) {
                tool_segment = format!("{base_segment}_{suffix}");
                suffix += 1;
            }
            assigned_segments[index] = tool_segment;
        }

        let specs = pending_specs
            .into_iter()
            .zip(assigned_segments)
            .map(|(spec, tool_segment)| DiscoveredMcpToolSpec {
                tool_id: format!("mcp.{}.{}", self.server_id, tool_segment),
                remote_tool_name: spec.remote_tool_name,
                description: spec.description,
                parameters_schema: spec.parameters_schema,
            })
            .collect();

        Ok(specs)
    }

    async fn list_resources(&self) -> Result<ToolResult, ToolError> {
        let list = self.list_items("resources/list", "resources").await?;
        let display_text = render_list_output(
            &format!("MCP resources from {}", self.server_id),
            &list.items,
            |item| {
                let uri = item
                    .get("uri")
                    .and_then(Value::as_str)
                    .unwrap_or("resource");
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty());
                match name {
                    Some(name) => format!("{uri} — {name}"),
                    None => uri.to_string(),
                }
            },
        );
        Ok(ToolResult {
            display_text,
            structured_json: Some(
                self.wrap_result(json!({ "resources": list.items }), &list.metadata),
            ),
            artifacts: Vec::new(),
        })
    }

    async fn read_resource(&self, args: McpResourceReadArgs) -> Result<ToolResult, ToolError> {
        let uri = args.uri;
        let (result, metadata) = if let Some(cache) = &self.stdio_session {
            self.request_via_stdio(cache, "resources/read", json!({ "uri": uri.clone() }))
                .await?
        } else {
            let mut session = self.start_session().await?;
            let payload = session
                .request("resources/read", json!({ "uri": uri.clone() }))
                .await;
            let metadata = session.metadata().clone();
            let close_result = session.close().await;
            let result = payload?;
            close_result?;
            (result, metadata)
        };

        let contents = result
            .get("contents")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let display_text = render_resource_contents(&contents);
        Ok(ToolResult {
            display_text,
            structured_json: Some(self.wrap_result(
                json!({
                    "uri": uri,
                    "contents": contents,
                }),
                &metadata,
            )),
            artifacts: Vec::new(),
        })
    }

    async fn list_prompts(&self) -> Result<ToolResult, ToolError> {
        let list = self.list_items("prompts/list", "prompts").await?;
        let display_text = render_list_output(
            &format!("MCP prompts from {}", self.server_id),
            &list.items,
            |item| {
                let name = item.get("name").and_then(Value::as_str).unwrap_or("prompt");
                let description = item
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("No description");
                format!("{name} — {description}")
            },
        );
        Ok(ToolResult {
            display_text,
            structured_json: Some(
                self.wrap_result(json!({ "prompts": list.items }), &list.metadata),
            ),
            artifacts: Vec::new(),
        })
    }

    async fn get_prompt(&self, args: McpPromptGetArgs) -> Result<ToolResult, ToolError> {
        let prompt_name = args.name;
        let arguments = normalize_object_value(args.arguments);
        let (result, metadata) = if let Some(cache) = &self.stdio_session {
            self.request_via_stdio(
                cache,
                "prompts/get",
                json!({
                    "name": prompt_name.clone(),
                    "arguments": arguments.clone(),
                }),
            )
            .await?
        } else {
            let mut session = self.start_session().await?;
            let payload = session
                .request(
                    "prompts/get",
                    json!({
                        "name": prompt_name.clone(),
                        "arguments": arguments.clone(),
                    }),
                )
                .await;
            let metadata = session.metadata().clone();
            let close_result = session.close().await;
            let result = payload?;
            close_result?;
            (result, metadata)
        };

        let messages = result
            .get("messages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let display_text = render_prompt_messages(&messages);
        Ok(ToolResult {
            display_text,
            structured_json: Some(self.wrap_result(
                json!({
                    "name": prompt_name,
                    "arguments": arguments,
                    "messages": messages,
                }),
                &metadata,
            )),
            artifacts: Vec::new(),
        })
    }

    async fn list_items(&self, method: &str, key: &str) -> Result<McpListResult, ToolError> {
        if let Some(cache) = &self.stdio_session {
            let mut cursor = None::<String>;
            let mut items = Vec::new();

            loop {
                let mut params = serde_json::Map::new();
                if let Some(value) = cursor.as_deref() {
                    params.insert("cursor".to_string(), Value::String(value.to_string()));
                }
                let (page, page_metadata) = self
                    .request_via_stdio(cache, method, Value::Object(params))
                    .await?;
                if let Some(page_items) = page.get(key).and_then(Value::as_array) {
                    items.extend(page_items.iter().cloned());
                }
                cursor = page
                    .get("nextCursor")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                if cursor.is_none() {
                    return Ok(McpListResult {
                        items,
                        metadata: page_metadata,
                    });
                }
            }
        }

        self.list_items_ephemeral(method, key).await
    }

    async fn list_items_ephemeral(
        &self,
        method: &str,
        key: &str,
    ) -> Result<McpListResult, ToolError> {
        let mut session = self.start_session().await?;
        let mut cursor = None::<String>;
        let mut items = Vec::new();

        loop {
            let mut params = serde_json::Map::new();
            if let Some(value) = cursor.as_deref() {
                params.insert("cursor".to_string(), Value::String(value.to_string()));
            }
            let page = match session.request(method, Value::Object(params)).await {
                Ok(page) => page,
                Err(err) => {
                    let _ = session.close().await;
                    return Err(err);
                }
            };
            if let Some(page_items) = page.get(key).and_then(Value::as_array) {
                items.extend(page_items.iter().cloned());
            }
            cursor = page
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            if cursor.is_none() {
                let metadata = session.metadata().clone();
                session.close().await?;
                return Ok(McpListResult { items, metadata });
            }
        }
    }

    async fn request_via_stdio(
        &self,
        cache: &std::sync::Arc<tokio::sync::Mutex<Option<StdioMcpSession>>>,
        method: &str,
        params: Value,
    ) -> Result<(Value, McpSessionMetadata), ToolError> {
        let mut guard = cache.lock().await;
        if guard.is_none() {
            *guard = Some(self.start_stdio_session().await?);
        }
        let session = guard
            .as_mut()
            .expect("stdio MCP session should be initialized");
        let response = session.request(method, params).await;
        let metadata = session.metadata.clone();
        match response {
            Ok(payload) => Ok((payload, metadata)),
            Err(err) => {
                let stale = guard.take();
                drop(guard);
                if let Some(session) = stale {
                    let _ = session.close().await;
                }
                Err(err)
            }
        }
    }

    async fn start_stdio_session(&self) -> Result<StdioMcpSession, ToolError> {
        match &self.server_config {
            McpServerConfig::Stdio {
                command,
                env,
                cwd,
                timeout_secs,
                enabled: _,
            } => {
                StdioMcpSession::start(&self.server_id, command, env, cwd.as_ref(), *timeout_secs)
                    .await
            }
            McpServerConfig::Http { .. } => Err(ToolError::Execution(format!(
                "MCP server `{}` is not configured for stdio sessions",
                self.server_id
            ))),
        }
    }

    async fn start_session(&self) -> Result<McpSession, ToolError> {
        McpSession::start(
            &self.server_id,
            &self.server_config,
            self.http_client.clone(),
        )
        .await
    }

    fn wrap_result(&self, payload: Value, metadata: &McpSessionMetadata) -> Value {
        json!({
            "server": {
                "id": self.server_id,
                "transport": self.transport_name(),
            },
            "protocolVersion": metadata.protocol_version,
            "serverInfo": metadata.server_info,
            "payload": payload,
        })
    }
}

#[derive(Debug, Clone)]
struct McpListResult {
    items: Vec<Value>,
    metadata: McpSessionMetadata,
}

#[derive(Debug, Clone, Default)]
struct McpSessionMetadata {
    protocol_version: Option<String>,
    server_info: Option<Value>,
}

enum McpSession {
    Stdio(StdioMcpSession),
    Http(HttpMcpSession),
}

impl McpSession {
    async fn start(
        server_id: &str,
        config: &McpServerConfig,
        http_client: reqwest::Client,
    ) -> Result<Self, ToolError> {
        match config {
            McpServerConfig::Stdio {
                command,
                env,
                cwd,
                timeout_secs,
                ..
            } => Ok(Self::Stdio(
                StdioMcpSession::start(server_id, command, env, cwd.as_ref(), *timeout_secs)
                    .await?,
            )),
            McpServerConfig::Http {
                endpoint,
                headers,
                timeout_secs,
                ..
            } => Ok(Self::Http(
                HttpMcpSession::start(server_id, endpoint, headers, *timeout_secs, http_client)
                    .await?,
            )),
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, ToolError> {
        match self {
            Self::Stdio(session) => session.request(method, params).await,
            Self::Http(session) => session.request(method, params).await,
        }
    }

    fn metadata(&self) -> &McpSessionMetadata {
        match self {
            Self::Stdio(session) => &session.metadata,
            Self::Http(session) => &session.metadata,
        }
    }

    async fn close(self) -> Result<(), ToolError> {
        match self {
            Self::Stdio(session) => session.close().await,
            Self::Http(session) => session.close().await,
        }
    }
}

struct StdioMcpSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    timeout: Duration,
    metadata: McpSessionMetadata,
}

impl StdioMcpSession {
    async fn start(
        server_id: &str,
        command: &[String],
        env: &BTreeMap<String, String>,
        cwd: Option<&std::path::PathBuf>,
        timeout_secs: u64,
    ) -> Result<Self, ToolError> {
        if command.is_empty() {
            return Err(ToolError::Execution(format!(
                "MCP server `{server_id}` has empty stdio command"
            )));
        }

        let mut process = Command::new(&command[0]);
        process
            .args(command.iter().skip(1))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        if let Some(cwd) = cwd {
            process.current_dir(cwd);
        }
        if !env.is_empty() {
            process.envs(env.iter());
        }

        let mut child = process.spawn().map_err(|err| {
            ToolError::Execution(format!(
                "failed to start MCP stdio server `{server_id}`: {err}"
            ))
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            ToolError::Execution(format!("MCP stdio server `{server_id}` stdin unavailable"))
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            ToolError::Execution(format!("MCP stdio server `{server_id}` stdout unavailable"))
        })?;
        let timeout = Duration::from_secs(timeout_secs.max(1));

        let mut session = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
            timeout,
            metadata: McpSessionMetadata::default(),
        };
        let initialize = session
            .request(
                "initialize",
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {
                        "name": "agent-harness",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                }),
            )
            .await?;
        session.metadata = parse_session_metadata(&initialize);
        session
            .notify("notifications/initialized", json!({}))
            .await?;
        Ok(session)
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, ToolError> {
        let request_id = self.next_request_id();
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        }))
        .await?;

        loop {
            let message = self.read_message().await?;
            if let Some(server_method) = message.get("method").and_then(Value::as_str) {
                if let Some(message_id) = message.get("id").cloned() {
                    self.respond_method_not_found(message_id, server_method)
                        .await?;
                }
                continue;
            }

            if message.get("id") != Some(&Value::String(request_id.clone())) {
                continue;
            }

            return extract_jsonrpc_result(message, method);
        }
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), ToolError> {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .await
    }

    async fn respond_method_not_found(
        &mut self,
        request_id: Value,
        method: &str,
    ) -> Result<(), ToolError> {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": -32601,
                "message": format!("agent-harness MCP client does not implement `{method}`"),
            },
        }))
        .await
    }

    async fn write_message(&mut self, message: &Value) -> Result<(), ToolError> {
        let body = serde_json::to_vec(message)
            .map_err(|err| ToolError::Execution(format!("failed to encode MCP message: {err}")))?;
        timeout(self.timeout, async {
            self.stdin.write_all(&body).await?;
            self.stdin.write_all(b"\n").await?;
            self.stdin.flush().await
        })
        .await
        .map_err(|_| ToolError::Execution("MCP stdio write timed out".to_string()))
        .map_err(|err| ToolError::Execution(format!("failed to write MCP message: {err}")))?
        .map_err(|err| ToolError::Execution(format!("failed to flush MCP message: {err}")))
    }

    async fn read_message(&mut self) -> Result<Value, ToolError> {
        loop {
            let mut line = String::new();
            let read = timeout(self.timeout, self.stdout.read_line(&mut line))
                .await
                .map_err(|_| ToolError::Execution("MCP stdio read timed out".to_string()))?
                .map_err(|err| ToolError::Execution(format!("failed to read MCP output: {err}")))?;
            if read == 0 {
                return Err(ToolError::Execution(
                    "MCP stdio server closed the connection".to_string(),
                ));
            }

            if line.trim().is_empty() {
                continue;
            }

            if line.to_ascii_lowercase().starts_with("content-length:") {
                let length = parse_content_length(&line)?;
                loop {
                    let mut header_line = String::new();
                    let header_read =
                        timeout(self.timeout, self.stdout.read_line(&mut header_line))
                            .await
                            .map_err(|_| {
                                ToolError::Execution("MCP stdio read timed out".to_string())
                            })?
                            .map_err(|err| {
                                ToolError::Execution(format!("failed to read MCP header: {err}"))
                            })?;
                    if header_read == 0 {
                        return Err(ToolError::Execution(
                            "MCP stdio server closed before message body".to_string(),
                        ));
                    }
                    if header_line == "\n" || header_line == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0_u8; length];
                timeout(self.timeout, self.stdout.read_exact(&mut body))
                    .await
                    .map_err(|_| ToolError::Execution("MCP stdio read timed out".to_string()))?
                    .map_err(|err| {
                        ToolError::Execution(format!("failed to read MCP message body: {err}"))
                    })?;
                return serde_json::from_slice(&body).map_err(|err| {
                    ToolError::Execution(format!("failed to parse MCP message body: {err}"))
                });
            }

            return serde_json::from_str(line.trim()).map_err(|err| {
                ToolError::Execution(format!("failed to parse MCP stdio message: {err}"))
            });
        }
    }

    fn next_request_id(&mut self) -> String {
        let id = format!("{}", self.next_id);
        self.next_id += 1;
        id
    }

    async fn close(mut self) -> Result<(), ToolError> {
        drop(self.stdin);
        match timeout(self.timeout, self.child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(err)) => Err(ToolError::Execution(format!(
                "failed to wait for MCP stdio server shutdown: {err}"
            ))),
            Err(_) => {
                let _ = self.child.kill().await;
                let _ = self.child.wait().await;
                Ok(())
            }
        }
    }
}

struct HttpMcpSession {
    client: reqwest::Client,
    endpoint: String,
    headers: BTreeMap<String, String>,
    timeout: Duration,
    next_id: u64,
    session_id: Option<String>,
    metadata: McpSessionMetadata,
}

impl HttpMcpSession {
    async fn start(
        server_id: &str,
        endpoint: &str,
        headers: &BTreeMap<String, String>,
        timeout_secs: u64,
        client: reqwest::Client,
    ) -> Result<Self, ToolError> {
        let mut session = Self {
            client,
            endpoint: endpoint.to_string(),
            headers: headers.clone(),
            timeout: Duration::from_secs(timeout_secs.max(1)),
            next_id: 1,
            session_id: None,
            metadata: McpSessionMetadata::default(),
        };

        let initialize = session
            .request(
                "initialize",
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {
                        "name": "agent-harness",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                }),
            )
            .await
            .map_err(|err| {
                ToolError::Execution(format!(
                    "failed to initialize MCP HTTP server `{server_id}`: {err}"
                ))
            })?;
        session.metadata = parse_session_metadata(&initialize);
        session
            .notify("notifications/initialized", json!({}))
            .await?;
        Ok(session)
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, ToolError> {
        let id = self.next_request_id();
        let message = self
            .post_jsonrpc(
                Some(id.clone()),
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": method,
                    "params": params,
                }),
            )
            .await?;
        extract_jsonrpc_result(message, method)
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), ToolError> {
        let _ = self
            .post_jsonrpc(
                None,
                json!({
                    "jsonrpc": "2.0",
                    "method": method,
                    "params": params,
                }),
            )
            .await?;
        Ok(())
    }

    async fn post_jsonrpc(
        &mut self,
        request_id: Option<String>,
        payload: Value,
    ) -> Result<Value, ToolError> {
        let mut request = self
            .client
            .post(&self.endpoint)
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(MCP_PROTOCOL_VERSION_HEADER, MCP_PROTOCOL_VERSION)
            .timeout(self.timeout)
            .json(&payload);
        if let Some(session_id) = &self.session_id {
            request = request.header(MCP_SESSION_ID_HEADER, session_id);
        }
        for (name, value) in &self.headers {
            request = request.header(name, value);
        }

        let response = request.send().await.map_err(|err| {
            if err.is_timeout() {
                ToolError::Execution("MCP HTTP request timed out".to_string())
            } else {
                ToolError::Execution(format!("MCP HTTP request failed: {err}"))
            }
        })?;
        if let Some(header_value) = response
            .headers()
            .get(MCP_SESSION_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
        {
            self.session_id = Some(header_value);
        }

        let status = response.status();
        if !status.is_success() {
            let headers = response.headers().clone();
            let body = response.text().await.unwrap_or_default();
            return Err(ToolError::Execution(render_mcp_http_status_error(
                status, &headers, &body,
            )));
        }

        if request_id.is_none() {
            return Ok(Value::Null);
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if content_type.starts_with("text/event-stream") {
            return read_sse_response(response, request_id.as_deref()).await;
        }

        let body = response.text().await.map_err(|err| {
            ToolError::Execution(format!("failed to read MCP HTTP response body: {err}"))
        })?;
        if body.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&body)
            .map_err(|err| ToolError::Execution(render_mcp_http_parse_error(&body, &err)))
    }

    fn next_request_id(&mut self) -> String {
        let id = format!("{}", self.next_id);
        self.next_id += 1;
        id
    }

    async fn close(self) -> Result<(), ToolError> {
        if let Some(session_id) = self.session_id {
            let mut request = self
                .client
                .delete(&self.endpoint)
                .header(MCP_SESSION_ID_HEADER, session_id)
                .header(MCP_PROTOCOL_VERSION_HEADER, MCP_PROTOCOL_VERSION)
                .timeout(self.timeout);
            for (name, value) in &self.headers {
                request = request.header(name, value);
            }
            let _ = request.send().await;
        }
        Ok(())
    }
}

async fn read_sse_response(
    mut response: reqwest::Response,
    request_id: Option<&str>,
) -> Result<Value, ToolError> {
    let mut buffer = String::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| ToolError::Execution(format!("failed to read MCP SSE chunk: {err}")))?
    {
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = find_sse_event_boundary(&buffer) {
            let event = buffer[..index].to_string();
            let remainder = buffer[index..].trim_start_matches(['\r', '\n']).to_string();
            buffer = remainder;
            if let Some(message) = parse_sse_event(&event)? {
                if request_id.is_none() {
                    return Ok(message);
                }
                let response_id = message.get("id").and_then(Value::as_str);
                if response_id == request_id {
                    return Ok(message);
                }
            }
        }
    }
    Err(ToolError::Execution(
        "MCP SSE stream ended before the request response arrived".to_string(),
    ))
}

fn find_sse_event_boundary(buffer: &str) -> Option<usize> {
    buffer
        .find("\n\n")
        .or_else(|| buffer.find("\r\n\r\n"))
        .map(|index| {
            if buffer[index..].starts_with("\r\n\r\n") {
                index + 4
            } else {
                index + 2
            }
        })
}

fn parse_sse_event(event: &str) -> Result<Option<Value>, ToolError> {
    let mut data_lines = Vec::new();
    for line in event.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            let trimmed = data.trim();
            if trimmed == "[DONE]" {
                return Ok(None);
            }
            data_lines.push(trimmed);
        }
    }
    if data_lines.is_empty() {
        return Ok(None);
    }
    let joined = data_lines.join("\n");
    serde_json::from_str(&joined).map(Some).map_err(|err| {
        ToolError::Execution(format!(
            "failed to parse MCP SSE data: {}",
            describe_upstream_non_json_response(&joined).unwrap_or_else(|| err.to_string())
        ))
    })
}

fn extract_jsonrpc_result(message: Value, method: &str) -> Result<Value, ToolError> {
    if let Some(error) = message.get("error") {
        return Err(ToolError::Execution(format!(
            "MCP `{method}` failed: {}",
            jsonrpc_error_message(error)
        )));
    }
    Ok(message.get("result").cloned().unwrap_or(Value::Null))
}

fn parse_session_metadata(result: &Value) -> McpSessionMetadata {
    McpSessionMetadata {
        protocol_version: result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        server_info: result.get("serverInfo").cloned(),
    }
}

fn render_list_output<F>(title: &str, items: &[Value], render: F) -> String
where
    F: Fn(&Value) -> String,
{
    if items.is_empty() {
        format!("{title}: none")
    } else {
        let lines = items.iter().map(render).collect::<Vec<_>>().join("\n");
        format!("{title}\n{lines}")
    }
}

fn render_content_entries(content: Option<&Vec<Value>>) -> Vec<String> {
    content
        .into_iter()
        .flat_map(|entries| entries.iter())
        .filter_map(render_content_entry)
        .collect()
}

fn render_content_entry(entry: &Value) -> Option<String> {
    match entry.get("type").and_then(Value::as_str) {
        Some("text") => entry
            .get("text")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        Some("image") => Some(format!(
            "[image {}]",
            entry
                .get("mimeType")
                .and_then(Value::as_str)
                .unwrap_or("unknown mime")
        )),
        Some("resource") => entry
            .get("resource")
            .and_then(render_resource_entry)
            .or_else(|| Some(compact_json(entry))),
        _ => Some(compact_json(entry)),
    }
}

fn render_resource_contents(contents: &[Value]) -> String {
    if contents.is_empty() {
        return "MCP resource returned no contents".to_string();
    }

    contents
        .iter()
        .map(|entry| render_resource_entry(entry).unwrap_or_else(|| compact_json(entry)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_resource_entry(entry: &Value) -> Option<String> {
    if let Some(text) = entry.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(blob) = entry.get("blob").and_then(Value::as_str) {
        let uri = entry
            .get("uri")
            .and_then(Value::as_str)
            .unwrap_or("resource");
        let mime = entry
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream");
        return Some(format!(
            "[binary resource {uri} ({mime}, {} base64 chars)]",
            blob.len()
        ));
    }
    entry
        .get("uri")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn render_prompt_messages(messages: &[Value]) -> String {
    if messages.is_empty() {
        return "MCP prompt returned no messages".to_string();
    }

    messages
        .iter()
        .map(|message| {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("message");
            let content = message
                .get("content")
                .and_then(Value::as_array)
                .map(|entries| render_content_entries(Some(entries)).join("\n"))
                .filter(|text| !text.trim().is_empty())
                .unwrap_or_else(|| compact_json(message));
            format!("{role}: {content}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_object_value(value: Value) -> Value {
    match value {
        Value::Null => Value::Object(serde_json::Map::new()),
        Value::Object(_) => value,
        other => json!({ "value": other }),
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn jsonrpc_error_message(error: &Value) -> String {
    let code = error
        .get("code")
        .and_then(Value::as_i64)
        .map(|value| value.to_string());
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .map(normalize_mcp_error_message)
        .unwrap_or_else(|| compact_json(error));
    match code {
        Some(code) => format!("{message} (code {code})"),
        None => message,
    }
}

fn render_mcp_http_parse_error(body: &str, err: &serde_json::Error) -> String {
    match describe_upstream_non_json_response(body) {
        Some(message) => format!("failed to parse MCP HTTP response: {message}"),
        None => format!("failed to parse MCP HTTP response: {err}"),
    }
}

fn render_mcp_http_status_error(
    status: StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> String {
    let status_prefix = format!("MCP HTTP request failed with status {status}");

    if status == StatusCode::TOO_MANY_REQUESTS {
        let retry_after = retry_after_hint(headers)
            .map(|value| format!("; retry-after {value}"))
            .unwrap_or_default();
        let detail = extract_upstream_error_detail(body)
            .or_else(|| describe_upstream_non_json_response(body))
            .unwrap_or_else(|| "upstream service rate limited the request".to_string());
        return format!("{status_prefix}: {detail}{retry_after}");
    }

    if let Some(detail) =
        extract_upstream_error_detail(body).or_else(|| describe_upstream_non_json_response(body))
    {
        return format!("{status_prefix}: {detail}");
    }

    status_prefix
}

fn normalize_mcp_error_message(message: &str) -> String {
    describe_upstream_non_json_response(message).unwrap_or_else(|| collapse_whitespace(message))
}

fn extract_upstream_error_detail(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }

    let value: Value = serde_json::from_str(trimmed).ok()?;
    if let Some(error) = value.get("error") {
        return Some(jsonrpc_error_message(error));
    }
    for field in ["message", "detail", "error_description", "error"] {
        if let Some(message) = value.get(field).and_then(Value::as_str) {
            let collapsed = collapse_whitespace(message);
            if !collapsed.is_empty() {
                return Some(collapsed);
            }
        }
    }
    None
}

fn describe_upstream_non_json_response(body: &str) -> Option<String> {
    let collapsed = collapse_whitespace(body);
    if collapsed.is_empty() {
        return None;
    }

    let snippet = truncated_snippet(&collapsed, 160);
    let lower = collapsed.to_ascii_lowercase();
    let looks_like_non_json = lower.contains("unexpected token")
        || lower.contains("not valid json")
        || lower.contains("failed to parse");
    let looks_like_too_many_requests = lower.contains("too many requests")
        || lower.contains("too many request")
        || lower.contains("too many r");
    let looks_like_rate_limit = looks_like_too_many_requests || lower.contains("rate limit");
    let looks_like_html =
        lower.contains("<html") || lower.contains("<!doctype html") || lower.contains("<body");

    if looks_like_too_many_requests {
        return Some(
            "upstream service returned a non-JSON rate-limit response (Too Many Requests)"
                .to_string(),
        );
    }
    if looks_like_rate_limit {
        return Some(format!(
            "upstream service rate limited the request: {snippet}"
        ));
    }
    if looks_like_html {
        return Some(format!(
            "upstream service returned HTML instead of JSON: {snippet}"
        ));
    }
    if looks_like_non_json {
        return Some(format!(
            "upstream service returned non-JSON content: {snippet}"
        ));
    }
    None
}

fn retry_after_hint(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn truncated_snippet(value: &str, max_chars: usize) -> String {
    let truncated = value.chars().take(max_chars).collect::<String>();
    if truncated.chars().count() == value.chars().count() {
        truncated
    } else {
        format!("{truncated}…")
    }
}

fn parse_content_length(line: &str) -> Result<usize, ToolError> {
    line.split_once(':')
        .map(|(_, value)| value.trim())
        .ok_or_else(|| ToolError::Execution("invalid MCP content-length header".to_string()))?
        .parse::<usize>()
        .map_err(|err| ToolError::Execution(format!("invalid MCP content length: {err}")))
}

#[cfg(test)]
mod tests {
    use super::{
        describe_upstream_non_json_response, normalize_mcp_error_message,
        render_mcp_http_parse_error, render_mcp_http_status_error,
    };
    use reqwest::{header::HeaderMap, StatusCode};
    use serde_json::Value;

    #[test]
    fn mcp_error_normalization_marks_rate_limited_non_json_errors() {
        let message = normalize_mcp_error_message(
            "Unexpected token 'T', \"Too Many R\"... is not valid JSON",
        );
        assert_eq!(
            message,
            "upstream service returned a non-JSON rate-limit response (Too Many Requests)"
        );
    }

    #[test]
    fn mcp_http_parse_error_uses_body_context_for_non_json_responses() {
        let err = serde_json::from_str::<Value>("Too Many Requests")
            .expect_err("plain text should not parse as json");
        let message = render_mcp_http_parse_error("Too Many Requests", &err);
        assert!(message.contains("non-JSON"));
        assert!(message.contains("Too Many Requests"));
    }

    #[test]
    fn mcp_non_json_description_ignores_normal_text() {
        assert!(describe_upstream_non_json_response("transient upstream issue").is_none());
    }

    #[test]
    fn mcp_http_status_error_extracts_jsonrpc_body_message() {
        let message = render_mcp_http_status_error(
            StatusCode::BAD_GATEWAY,
            &HeaderMap::new(),
            r#"{"error":{"code":-32000,"message":"backend unavailable"}}"#,
        );
        assert!(message.contains("502 Bad Gateway"));
        assert!(message.contains("backend unavailable"));
        assert!(message.contains("code -32000"));
    }

    #[test]
    fn mcp_http_status_error_marks_rate_limits_and_retry_after() {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::RETRY_AFTER,
            "12".parse().expect("retry-after"),
        );

        let message = render_mcp_http_status_error(
            StatusCode::TOO_MANY_REQUESTS,
            &headers,
            "<html><body>Too Many Requests</body></html>",
        );
        assert!(message.contains("429 Too Many Requests"));
        assert!(message.contains("rate-limit response"));
        assert!(message.contains("retry-after 12"));
        assert!(!message.contains("<html>"));
    }
}
