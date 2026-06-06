use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use harness_core::config::{
    set_registered_mcp_server_connection_states, set_registered_mcp_server_first_class_tool_ids,
    McpConfig, McpServerConfig, McpServerConnectionState,
};
use harness_core::tool::{
    sanitize_mcp_tool_segment, Tool, ToolCapability, ToolContext, ToolError, ToolRegistry,
    ToolResult,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http_client;
use crate::mcp_render::{
    normalize_object_value, render_content_entries, render_list_output, render_prompt_messages,
    render_resource_contents,
};
use crate::mcp_session::{McpSession, McpSessionMetadata, StdioMcpSession};
use crate::parse_tool_args;
use crate::text::{has_trimmed_content, trimmed_non_empty};

pub(crate) fn register_mcp_tools(registry: &mut ToolRegistry, config: McpConfig) {
    if config.servers.is_empty() {
        set_registered_mcp_server_connection_states(BTreeMap::new());
        set_registered_mcp_server_first_class_tool_ids(BTreeMap::new());
        return;
    }

    let http_client = http_client::default_client_or_fallback();
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
                let _: EmptyArgs = parse_tool_args(args_json)?;
                self.executor.list_tools().await
            }
            McpToolKind::ToolCall => {
                let args: McpToolCallArgs = parse_tool_args(args_json)?;
                self.executor.call_tool(&ctx, args).await
            }
            McpToolKind::ResourcesList => {
                let _: EmptyArgs = parse_tool_args(args_json)?;
                self.executor.list_resources().await
            }
            McpToolKind::ResourceRead => {
                let args: McpResourceReadArgs = parse_tool_args(args_json)?;
                self.executor.read_resource(args).await
            }
            McpToolKind::PromptsList => {
                let _: EmptyArgs = parse_tool_args(args_json)?;
                self.executor.list_prompts().await
            }
            McpToolKind::PromptGet => {
                let args: McpPromptGetArgs = parse_tool_args(args_json)?;
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
                    .and_then(trimmed_non_empty)
                    .unwrap_or("No description");
                format!("{name} — {description}")
            },
        );
        Ok(self.tool_result(display_text, json!({ "tools": list.items }), &list.metadata))
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

        Ok(self.tool_result(
            display_text,
            json!({
                "tool": tool_name,
                "arguments": arguments,
                "result": result,
            }),
            &metadata,
        ))
    }

    async fn discover_tools(&self) -> Result<Vec<DiscoveredMcpToolSpec>, ToolError> {
        let list = self.list_items_ephemeral("tools/list", "tools").await?;
        let mut pending_specs = Vec::new();

        for item in list.items {
            let Some(remote_tool_name) = item
                .get("name")
                .and_then(Value::as_str)
                .and_then(trimmed_non_empty)
            else {
                continue;
            };

            let description = item
                .get("description")
                .and_then(Value::as_str)
                .and_then(trimmed_non_empty)
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
                    .filter(|value| has_trimmed_content(value));
                match name {
                    Some(name) => format!("{uri} — {name}"),
                    None => uri.to_string(),
                }
            },
        );
        Ok(self.tool_result(
            display_text,
            json!({ "resources": list.items }),
            &list.metadata,
        ))
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
        Ok(self.tool_result(
            display_text,
            json!({
                "uri": uri,
                "contents": contents,
            }),
            &metadata,
        ))
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
                    .and_then(trimmed_non_empty)
                    .unwrap_or("No description");
                format!("{name} — {description}")
            },
        );
        Ok(self.tool_result(
            display_text,
            json!({ "prompts": list.items }),
            &list.metadata,
        ))
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
        Ok(self.tool_result(
            display_text,
            json!({
                "name": prompt_name,
                "arguments": arguments,
                "messages": messages,
            }),
            &metadata,
        ))
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
        let metadata = session.metadata().clone();
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

    fn tool_result(
        &self,
        display_text: String,
        payload: Value,
        metadata: &McpSessionMetadata,
    ) -> ToolResult {
        crate::text_json_tool_result(display_text, self.wrap_result(payload, metadata))
    }
}

#[derive(Debug, Clone)]
struct McpListResult {
    items: Vec<Value>,
    metadata: McpSessionMetadata,
}
