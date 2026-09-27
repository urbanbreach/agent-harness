use super::*;

pub(super) fn publish(server: &Arc<Server>, result: &Value) -> Result<(), ToolError> {
    let tools = result["tools"]
        .as_array()
        .ok_or_else(|| failure("MCP tool list is malformed"))?;
    let mut names = BTreeMap::new();
    for tool in tools {
        let name = tool["name"]
            .as_str()
            .filter(|n| !n.is_empty() && n.len() <= 128)
            .ok_or_else(|| failure("MCP tool name is invalid"))?;
        if names.insert(name, tool).is_some() {
            return Err(failure("MCP tool list contains a duplicate name"));
        }
    }
    let mut catalog = BTreeMap::new();
    let mut mapping = BTreeMap::new();
    for (name, tool) in names {
        let id = tool_id(&server.name, name);
        mapping.insert(name.to_owned(), id.clone());
        let schema = tool["inputSchema"].clone();
        if !schema.is_object() || schema["type"] != "object" {
            return Err(failure("MCP tool input schema must describe an object"));
        }
        let tool = Arc::new(DiscoveredTool {
            id: id.clone(),
            name: name.into(),
            description: tool["description"]
                .as_str()
                .unwrap_or("MCP server tool")
                .into(),
            schema,
            server: Arc::clone(server),
        }) as Arc<dyn Tool>;
        if catalog.insert(id, tool).is_some() {
            return Err(failure("MCP tool identifiers collide"));
        }
    }
    let shared = server
        .catalog
        .upgrade()
        .ok_or_else(|| failure("MCP registry was closed"))?;
    *shared
        .write()
        .map_err(|_| failure("MCP catalog lock failed"))? = catalog;
    harness_core::config::update_registered_mcp_server_tools(&server.name, mapping);
    Ok(())
}
pub(super) fn tool_id(server: &str, name: &str) -> String {
    let mut value: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !value.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        value.insert_str(0, "t_");
    }
    if value != name {
        value.push('_');
        value.push_str(&blake3::hash(name.as_bytes()).to_hex()[..16]);
    }
    format!("mcp.{server}.{value}")
}
struct DiscoveredTool {
    id: String,
    name: String,
    description: String,
    schema: Value,
    server: Arc<Server>,
}
#[async_trait::async_trait]
impl Tool for DiscoveredTool {
    fn id(&self) -> &str {
        &self.id
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn capability(&self) -> ToolCapability {
        self.delegate().capability()
    }
    fn parameters_json_schema(&self) -> Value {
        self.schema.clone()
    }
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        vec![
            (self.id.clone(), args.to_string()),
            (
                format!("mcp.{}.tool.call", self.server.name),
                self.name.clone(),
            ),
        ]
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        self.delegate()
            .call(ctx, json!({"tool":self.name,"arguments":args}))
            .await
    }
}
impl DiscoveredTool {
    fn delegate(&self) -> McpTool {
        McpTool {
            id: self.id.clone(),
            method: "tools/call",
            description: "MCP server tool",
            server: Arc::clone(&self.server),
        }
    }
}
