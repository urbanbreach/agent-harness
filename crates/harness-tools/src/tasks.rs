use harness_core::{
    config::SkillsConfig,
    coord::ChildTaskRequest,
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult},
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_stream::StreamExt;
mod history;

pub(crate) struct TaskTool(pub SkillsConfig);
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Task {
    prompt: String,
    #[serde(default, alias = "profile")]
    subagent_type: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(default, alias = "task_id")]
    session_id: Option<String>,
    run_in_background: bool,
    load_skills: Vec<String>,
    #[serde(default)]
    command: Option<String>,
}
#[async_trait::async_trait]
impl Tool for TaskTool {
    fn id(&self) -> &str {
        "task"
    }
    fn description(&self) -> &str {
        "Delegate a prompt to a child agent, or continue a returned session_id. Load only the skills needed for this task."
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object","properties":{"prompt":{"type":"string"},"description":{"type":"string"},"subagent_type":{"type":"string"},"session_id":{"type":"string"},"task_id":{"type":"string"},"run_in_background":{"type":"boolean"},"load_skills":{"type":"array","items":{"type":"string"}},"command":{"type":"string"}},"required":["prompt","run_in_background","load_skills"],"additionalProperties":false})
    }
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        let mut requests = vec![(
            "task".into(),
            args["subagent_type"]
                .as_str()
                .or(args["profile"].as_str())
                .unwrap_or("continue")
                .into(),
        )];
        for name in args["load_skills"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            requests.extend(crate::skills::load::permissions(name));
        }
        requests
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let mut args: Task =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
        if args.session_id.is_none() && args.subagent_type.as_deref().is_none_or(str::is_empty) {
            return Err(ToolError::InvalidArguments(
                "new tasks require subagent_type".into(),
            ));
        }
        args.prompt.push_str(&crate::skills::load::load(
            &ctx.workspace_root,
            &self.0,
            &args.load_skills,
        )?);
        if let Some(command) = args.command {
            args.prompt
                .push_str(&format!("\n\nParent command context:\n{command}"));
        }
        let mut result = ctx
            .coordinator
            .delegate_task(
                ctx.tool_call_id.to_string(),
                ChildTaskRequest {
                    session_id: args.session_id,
                    profile: args.subagent_type.unwrap_or_default(),
                    prompt: args.prompt,
                    description: args.description,
                    run_in_background: args.run_in_background,
                },
            )
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        if let Some(value) = result.structured_json.as_mut() {
            if let Some(id) = value["session_id"].as_str() {
                let info = ctx
                    .coordinator
                    .agent_runtime_info(id)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                value["profile"] = info.profile_name.into();
                value["model_ref"] = info.model_ref.into();
                value["toolset"] = json!(info.toolset);
            }
        }
        Ok(result)
    }
}

pub(crate) enum BackgroundTool {
    Output,
    Cancel,
}
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Background {
    session_id: Option<String>,
    task_id: Option<String>,
    request_id: Option<String>,
    request_ids: Vec<String>,
    wait_mode: Option<String>,
    block: bool,
    #[serde(alias = "timeout")]
    timeout_ms: Option<u64>,
    cancel: bool,
    reason: Option<String>,
    all: bool,
    full_session: bool,
    include_thinking: bool,
    message_limit: Option<usize>,
    since_message_id: Option<String>,
    include_tool_results: bool,
    thinking_max_chars: Option<usize>,
    from_end: bool,
}
#[async_trait::async_trait]
impl Tool for BackgroundTool {
    fn id(&self) -> &str {
        match self {
            Self::Output => "background_output",
            Self::Cancel => "background_cancel",
        }
    }
    fn description(&self) -> &str {
        "Inspect, wait for, or cancel direct child tasks. Waiting times out without cancelling the task."
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object","properties":{"session_id":{"type":"string"},"task_id":{"type":"string"},"request_id":{"type":"string"},"request_ids":{"type":"array","items":{"type":"string"}},"wait_mode":{"enum":["any","all"]},"block":{"type":"boolean"},"timeout_ms":{"type":"integer","minimum":0,"maximum":300000},"timeout":{"type":"integer","minimum":0,"maximum":300000},"cancel":{"type":"boolean"},"reason":{"type":"string"},"all":{"type":"boolean"},"full_session":{"type":"boolean"},"include_thinking":{"type":"boolean","description":"Reasoning is not retained; returns an unavailable result."},"message_limit":{"type":"integer","minimum":0,"maximum":200},"since_message_id":{"type":"string"},"include_tool_results":{"type":"boolean"},"thinking_max_chars":{"type":"integer","minimum":0},"from_end":{"type":"boolean"}},"additionalProperties":false})
    }
    fn permission_requests(&self, _: &Value) -> Vec<(String, String)> {
        vec![(self.id().into(), "*".into())]
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let mut args: Background =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
        if let Some(mode) = &mut args.wait_mode {
            *mode = mode.trim().to_ascii_lowercase();
        }
        if args
            .wait_mode
            .as_deref()
            .is_some_and(|mode| !matches!(mode, "any" | "all"))
        {
            return Err(ToolError::InvalidArguments(
                "wait_mode must be any or all".into(),
            ));
        }
        let tool = ctx.tool_call_id.to_string();
        let selectors = if args.all {
            ctx.coordinator
                .owned_child_tasks(&tool)
                .await
                .map_err(|e| ToolError::Execution(e.to_string()))?
        } else {
            let mut ids = args.request_ids.clone();
            ids.extend(args.session_id.clone());
            ids.extend(args.task_id.clone());
            ids.extend(args.request_id.clone());
            ids.sort();
            ids.dedup();
            ids
        };
        if selectors.is_empty() || selectors.len() > 64 {
            return Err(ToolError::InvalidArguments(
                "select 1 to 64 direct child tasks".into(),
            ));
        }
        let cancel = matches!(self, Self::Cancel) || args.cancel;
        if selectors.len() > 1
            && (args.full_session || args.include_thinking || (!cancel && args.wait_mode.is_none()))
        {
            return Err(ToolError::InvalidArguments(
                "multiple children require wait_mode; history options require one child".into(),
            ));
        }
        let mut events = ctx
            .coordinator
            .subscribe_new_events()
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let deadline = tokio::time::Instant::now()
            + Duration::from_millis(args.timeout_ms.unwrap_or(60_000).min(300_000));
        loop {
            let mut results = Vec::new();
            for id in &selectors {
                results.push(
                    if cancel {
                        ctx.coordinator
                            .cancel_child_task(
                                &tool,
                                id,
                                args.reason.as_deref().unwrap_or("cancelled by parent"),
                            )
                            .await
                    } else {
                        ctx.coordinator.child_task_output(&tool, id).await
                    }
                    .map_err(|e| ToolError::Execution(e.to_string()))?,
                );
            }
            let done = |r: &ToolResult| {
                r.structured_json
                    .as_ref()
                    .is_some_and(|v| v["status"] != "running")
            };
            let complete = if args.wait_mode.as_deref() == Some("any") {
                results.iter().any(done)
            } else {
                results.iter().all(done)
            };
            if cancel || !args.block || complete || tokio::time::Instant::now() >= deadline {
                let timed_out = args.block && !cancel && !complete;
                return if results.len() == 1 {
                    let mut result = if args.full_session || args.include_thinking {
                        let (mut output, events) = ctx
                            .coordinator
                            .child_task_output_with_history(&tool, &selectors[0])
                            .await
                            .map_err(|e| ToolError::Execution(e.to_string()))?;
                        history::attach(&ctx, &mut output, &events, &args)?;
                        output
                    } else {
                        results
                            .pop()
                            .ok_or_else(|| ToolError::Execution("missing child result".into()))?
                    };
                    if let Some(value) = result.structured_json.as_mut() {
                        value["timed_out"] = timed_out.into();
                    }
                    Ok(result)
                } else {
                    Ok(ToolResult::structured(
                        results
                            .iter()
                            .map(|r| r.display_text.as_str())
                            .collect::<Vec<_>>()
                            .join("\n\n"),
                        json!({"tasks":results.iter().map(|r|&r.structured_json).collect::<Vec<_>>(),"timed_out":timed_out}),
                    ))
                };
            }
            tokio::select! {
                ()=ctx.cancellation.cancelled()=>return Err(ToolError::Cancelled),
                ()=tokio::time::sleep_until(deadline)=>{},
                event=events.next()=>{if event.is_none() {return Err(ToolError::Execution("session ended while waiting".into()));}},
            }
        }
    }
}
