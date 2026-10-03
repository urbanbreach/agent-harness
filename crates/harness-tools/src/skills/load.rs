use super::*;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolResult};
use serde_json::{json, Value};

pub(crate) struct SkillTool(pub SkillsConfig);
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[async_trait::async_trait]
impl Tool for SkillTool {
    fn id(&self) -> &str {
        "skill"
    }
    fn description(&self) -> &str {
        "Load a skill's instructions. Its tool hints do not grant permissions."
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object","properties":{"name":{"type":"string"},"arguments":{"type":"string"}},"required":["name"],"additionalProperties":false})
    }
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        permissions(args["name"].as_str().unwrap_or_default())
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let args: Args =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
        if args.arguments.len() > 16_384 {
            return Err(ToolError::InvalidArguments(
                "skill arguments exceed 16 KiB".into(),
            ));
        }
        let text = match &ctx.skill_startup {
            Some(snapshot) => load_catalog(&snapshot.catalog, &[args.name])?,
            None => load(&ctx.workspace_root, &self.0, &[args.name])?,
        };
        let expansion = text
            .matches("$ARGUMENTS")
            .count()
            .saturating_mul(args.arguments.len().saturating_sub("$ARGUMENTS".len()));
        if text.len().saturating_add(expansion) > 1_048_576 {
            return Err(ToolError::InvalidArguments(
                "expanded skill instructions exceed 1 MiB".into(),
            ));
        }
        Ok(ToolResult::text(
            text.replace("$ARGUMENTS", &args.arguments),
        ))
    }
}
pub(crate) fn permissions(name: &str) -> Vec<(String, String)> {
    let mut selectors = vec![("skill".into(), name.into())];
    if let Some((_, name)) = name.rsplit_once(':') {
        selectors.push(("skill".into(), name.into()));
    }
    selectors
}
pub(crate) fn load(
    workspace: &Path,
    config: &SkillsConfig,
    names: &[String],
) -> Result<String, ToolError> {
    if names.is_empty() {
        return Ok(String::new());
    }
    let catalog = discover_skill_catalog_with_config(workspace, config)?;
    load_catalog(&catalog, names)
}

fn load_catalog(catalog: &SkillCatalog, names: &[String]) -> Result<String, ToolError> {
    if names.len() > 32 || names.iter().any(|name| name.is_empty() || name.len() > 160) {
        return Err(ToolError::InvalidArguments(
            "load at most 32 named skills".into(),
        ));
    }
    if names.is_empty() {
        return Ok(String::new());
    }
    let mut loaded = BTreeSet::new();
    let mut result = String::new();
    for name in names {
        let entry = catalog
            .entries
            .iter()
            .find(|entry| entry.name == *name || entry.stable_id == *name)
            .ok_or_else(|| ToolError::InvalidArguments(format!("unknown skill: {name}")))?;
        if !entry.loadable {
            return Err(ToolError::InvalidArguments(format!(
                "skill {name} is {}",
                entry.status.as_str()
            )));
        }
        if !loaded.insert(&entry.stable_id) {
            continue;
        }
        let mut text = String::new();
        harness_core::store::open_private_file(&entry.location)?
            .take(1_048_577)
            .read_to_string(&mut text)?;
        if text.len() > 1_048_576 {
            return Err(ToolError::InvalidArguments("skill exceeds 1 MiB".into()));
        }
        let mut lines = text.split_inclusive('\n');
        let mut offset = lines
            .next()
            .filter(|line| line.trim() == "---")
            .ok_or_else(|| ToolError::InvalidArguments("skill header changed".into()))?
            .len();
        let start = offset;
        let mut end = None;
        for line in lines {
            if line.trim() == "---" {
                end = Some(offset);
                offset += line.len();
                break;
            }
            offset += line.len();
        }
        let end =
            end.ok_or_else(|| ToolError::InvalidArguments("skill header is incomplete".into()))?;
        let parsed: Header = serde_yaml_ng::from_str(&text[start..end])
            .map_err(|_| ToolError::InvalidArguments("invalid skill header".into()))?;
        if parsed.name != entry.name {
            return Err(ToolError::InvalidArguments(
                "skill identity changed while loading".into(),
            ));
        }
        result.push_str(&format!(
            "\n\nSkill {}:\n{}",
            entry.name,
            text[offset..].trim()
        ));
        if result.len() > 1_048_576 {
            return Err(ToolError::InvalidArguments(
                "combined skill instructions exceed 1 MiB".into(),
            ));
        }
    }
    Ok(result)
}
