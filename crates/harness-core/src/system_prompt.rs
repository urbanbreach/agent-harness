//! Shared, capability-aware prompts. Rendering never changes tool authority.
pub(crate) mod models;
mod templates;

use crate::{config::SubagentLimitBehavior, model_resolution::DelegationBias};
use harness_providers::ToolDef;
use minijinja::{syntax::SyntaxConfig, Environment};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Default)]
pub struct PromptSource {
    /// An explicit operator prompt replaces the shared base, without interpreting templates.
    pub configured: Option<String>,
    /// User overrides, supplied by the CLI's environment rather than process-global discovery.
    pub user_prompt_dir: Option<PathBuf>,
    /// Native children inherit the root agent's project prompts, including in worktrees.
    pub project_prompt_root: Option<PathBuf>,
    pub suffix: String,
    pub subagent: bool,
    pub agent_instructions: String,
    pub role_instructions: String,
    pub persona_instructions: String,
    pub isolated: bool,
    pub tool_kinds: Vec<(String, String)>,
}

pub struct PromptContext<'a> {
    pub workspace: &'a Path,
    pub model: &'a str,
    pub prompt_preset: &'a str,
    pub delegation_bias: DelegationBias,
    /// All reachable tools, including those routed through eval.
    pub tools: &'a [ToolDef],
    pub direct_tools: &'a [ToolDef],
    pub current_date: &'a str,
    pub max_concurrent: usize,
    pub limit_behavior: SubagentLimitBehavior,
}

pub struct RenderedPrompt {
    pub system: String,
    pub eval_guidance: String,
}

impl PromptSource {
    pub fn render(&self, context: &PromptContext<'_>) -> Result<RenderedPrompt, minijinja::Error> {
        let mut tools = BTreeMap::new();
        let mut inventory = Vec::new();
        for tool in context.tools {
            let reference = if context
                .direct_tools
                .iter()
                .any(|t| t.tool_id == tool.tool_id)
            {
                tool.function_name.clone()
            } else {
                format!("eval: tool.{}", tool.function_name)
            };
            inventory.push(reference.clone());
            tools.insert(tool.tool_id.clone(), Value::String(reference));
        }
        // Custom agent templates use the same kind names as tool discovery.
        let mut by_kind = BTreeMap::new();
        let mut params = BTreeMap::new();
        for (id, kind) in &self.tool_kinds {
            let Some(reference) = tools.get(id) else {
                continue;
            };
            if by_kind.contains_key(kind.as_str()) {
                continue;
            }
            let Some(tool) = context.tools.iter().find(|tool| tool.tool_id == *id) else {
                continue;
            };
            by_kind.insert(kind.as_str(), reference.clone());
            let mut names = BTreeMap::new();
            if let Some(properties) = tool.parameters["properties"].as_object() {
                names.extend(properties.keys().map(|name| (name.clone(), name.clone())));
                if kind == "execute" && properties.contains_key("run_in_background") {
                    names.insert("is_background".into(), "run_in_background".into());
                }
            }
            params.insert(kind, names);
        }
        tools.insert("by_kind".into(), json!(by_kind));
        let mut data = json!({
            "tools": tools, "inventory": inventory,
            "model": context.model, "delegation_bias": context.delegation_bias,
            "working_directory": context.workspace.to_string_lossy(),
            "os_name": std::env::consts::OS,
            "shell_path": std::env::var("SHELL").unwrap_or_default(),
            "current_date": context.current_date.split('T').next().unwrap_or_default(),
            "subagent": self.subagent, "isolated": self.isolated,
            "max_concurrent": context.max_concurrent,
            "limit_behavior": match context.limit_behavior {
                crate::config::SubagentLimitBehavior::Queue => "queue",
                crate::config::SubagentLimitBehavior::Fail => "are rejected at the concurrency limit",
            },
            "role_instructions": self.role_instructions,
            "persona_instructions": self.persona_instructions,
            "params": params,
            "is_windows": cfg!(windows), "has_unix_utilities": cfg!(unix),
            "system_reminders_enabled": true,
            "memory_enabled": by_kind.contains_key("memory_search") && by_kind.contains_key("memory_get"),
        });
        let syntax = SyntaxConfig::builder()
            .block_delimiters("${%", "%}")
            .variable_delimiters("${{", "}}")
            .comment_delimiters("${#", "#}")
            .build()?;
        let mut environment = Environment::new();
        environment.set_syntax(syntax);
        templates::configure(&mut environment, self, context.workspace);
        let preset = if context.prompt_preset.is_empty() {
            models::resolve(
                &crate::agent::AgentModelRef::parse(context.model).model_id,
                None,
            )
        } else {
            context.prompt_preset
        };
        let eval_guidance = if context.tools.iter().any(|tool| tool.tool_id == "eval") {
            environment
                .get_template(&format!("eval/{}.md", models::eval_dialect(preset)))?
                .render(&data)?
        } else {
            String::new()
        };
        if let Some(configured) = self.configured.as_deref().filter(|s| !s.trim().is_empty()) {
            return Ok(RenderedPrompt {
                system: format!("{configured}{}", self.suffix),
                eval_guidance,
            });
        }
        data["eval_guidance"] = eval_guidance.clone().into();
        data["prompt_preset"] = preset.into();
        data["personality"] = environment
            .get_template("personality.md")?
            .render(&data)?
            .into();
        let mut prompt = environment
            .get_template(&format!("models/{preset}.md"))?
            .render(&data)?;
        if self.subagent {
            data["agent"] = environment
                .render_str(&self.agent_instructions, &data)
                .unwrap_or_else(|_| self.agent_instructions.clone())
                .into();
            prompt.push_str("\n\n");
            prompt.push_str(&environment.get_template("subagent.md")?.render(&data)?);
        }
        prompt.push_str(&self.suffix);
        Ok(RenderedPrompt {
            system: prompt,
            eval_guidance,
        })
    }
}
