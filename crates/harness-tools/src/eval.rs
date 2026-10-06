mod host;
mod images;
mod kernel_tools;
mod result;

use harness_core::{
    config::EvalConfig,
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult},
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub fn register(registry: &mut ToolRegistry, config: EvalConfig) {
    registry.route_through_eval(config.route_tools.clone());
    registry.register(Arc::new(EvalTool {
        config,
        hosts: Mutex::new(BTreeMap::new()),
    }));
}

type HostSlot = Arc<Mutex<Option<Arc<host::Host>>>>;

struct EvalTool {
    config: EvalConfig,
    hosts: Mutex<BTreeMap<(String, Option<String>), HostSlot>>,
}

#[async_trait::async_trait]
impl Tool for EvalTool {
    fn id(&self) -> &str {
        "eval"
    }
    fn description(&self) -> &str {
        "Execute code in persistent JavaScript, Python, Ruby or Julia kernels. Run requires language, code and a one-line summary. State survives until reset or session end. Top-level await and a final expression work. Compose independent tool calls with known arguments using parallel(thunks) or JavaScript's Promise.allSettled, inspecting every result. Direct calls suit isolated operations. Inspect dependent results before choosing later calls; never guess arguments to fill a batch. Follow the model-specific eval routing guidance when supplied. Use specialized tools through tool.<name>(args) inside batches, rather than raw file or process APIs, to retain normal tool permissions and behavior. For example, await tool.read({path: '...'}). tool_schema() lists callable tools, including tools routed only through eval; tool_schema(name) returns their schema. Results have text, details, images and hasError. Check hasError before using a tool result. Filter large results in code and display the relevant evidence. display(value) emits JSON, markdown or images; images are delivered only when displayed. print, log, phase, read, write, env, agent, completion, output, wait, workpool, parallel(thunks) and pipeline(items, ...stages) are available. JS tool(function...) and Python @tool define child-callable functions. JS/Python %load executes a local script in the current kernel; %npm install and %pip install use managed session environments. workpool creates bounded native subagents; close seals input and delivers one aggregate notice. agent(handle:true) returns a record with control.status/output/send/cancel/wait methods in JS/Python. wait supports all, any-success, and settled outcomes. isolate:true uses a fresh QuickJS context when eval.sandbox.enabled; only tool and output helpers are present. parallel preserves order with bounded concurrency. Promise.allSettled retains independent failures. Long interactive cells detach and notify once at completion; do not rerun them. action peek/stop needs cell_id; list shows live and recent cells. reset clears only the selected language and refuses a busy kernel. timeout is the cell's execution budget in seconds, paused during host tool calls. on_timeout:error waits for settlement. Eval runs local code with the user's permissions; it is not an OS sandbox. Nested tools still require their own permissions, and recursive eval is rejected."
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        vec![(
            "eval".into(),
            args.get("summary")
                .or_else(|| args.get("title"))
                .and_then(Value::as_str)
                .unwrap_or("persistent local code execution")
                .into(),
        )]
    }
    fn parameters_json_schema(&self) -> Value {
        let languages = available_languages(&self.config);
        json!({"type":"object","properties":{
            "action":{"type":"string","enum":["run","peek","stop","list"],"description":"Defaults to run; peek and stop require cell_id."},
            "language":{"type":"string","enum":languages,"description":"Required for run. Each language retains separate state."},
            "code":{"type":"string","description":"Required for run. Cell body; top-level await works."},
            "summary":{"type":"string","description":"Required for run. Explain what you are doing and why in one line."},
            "timeout":{"type":"number","minimum":1,"maximum":86400,"description":format!("Own execution budget in seconds; default {}. Host tool waits are excluded. Does not move the {}s detach point. Wall-clock safety limit is {}s, raised by a larger timeout.",self.config.run_budget_seconds,self.config.cell_timeout_seconds.min(self.config.foreground_window_seconds),self.config.hard_limit_seconds)},
            "on_timeout":{"type":"string","enum":["detach","error"]},
            "reset":{"type":"boolean","description":"Reset only this language. Refused while it has live cells."},
            "isolate":{"type":"boolean","description":"Run JavaScript in a fresh QuickJS context with only explicit host tools and output helpers. Requires eval.sandbox.enabled. No ambient process, files, network, modules or persistent variables."},
            "cell_id":{"type":"string","minLength":1}
        },"additionalProperties":false})
    }
    async fn call(&self, ctx: ToolContext, mut input: Value) -> Result<ToolResult, ToolError> {
        self.config
            .validate()
            .map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
        input = harness_eval::normalize_request(input, &available_languages(&self.config))
            .map_err(|error| ToolError::InvalidArguments(error.to_string()))?;
        let control = matches!(input["action"].as_str(), Some("peek" | "stop" | "list"));
        let key = (ctx.run_id.clone(), ctx.actor.agent_id.clone());
        let slot = Arc::clone(self.hosts.lock().await.entry(key).or_default());
        let host = {
            let mut slot = slot.lock().await;
            if slot.as_ref().is_some_and(|host| !host.is_alive()) {
                *slot = None;
            }
            if slot.is_none() {
                *slot = Some(Arc::new(host::Host::start(&ctx, &self.config).await?));
            }
            Arc::clone(
                slot.as_ref()
                    .ok_or_else(|| failure("eval host failed to start"))?,
            )
        };
        let id = ctx.tool_call_id.to_string();
        let catalog = ctx
            .coordinator
            .eval_tool_catalog(id.clone())
            .await
            .map_err(failure)?;
        let mut messages = host.execute(&id, input, catalog, ctx.interactive).await?;
        let mut calls = tokio::task::JoinSet::new();
        let mut cancellations = BTreeMap::<String, CancellationToken>::new();
        let mut cancel_deadline = None;
        let mut settled = None;
        let mut detached = false;
        let mut cancelled = false;
        let mut steering = tokio::time::interval(Duration::from_millis(250));
        let mut steered = false;
        let outcome = async { loop {
            tokio::select! {
                biased;
                () = ctx.cancellation.cancelled(), if !cancelled => {
                    cancelled = true;
                    cancel_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(10));
                    for cancel in cancellations.values() { cancel.cancel(); }
                    host.send(json!({"type":"cancel","id":id})).await?;
                }
                event = messages.recv() => {
                    let Some(event) = event else { break Err(failure("eval runtime disconnected")); };
                    match event["type"].as_str() {
                        Some("update") => {
                            if !cancelled {
                                let output = result::text(&event["result"]);
                                ctx.coordinator.eval_progress(id.clone(), output, event["result"]["details"].clone()).await.map_err(failure)?;
                            }
                        }
                        Some("settled") => {
                            let mut result = event["result"].clone();
                            result["details"]["execution"] = event["rpcExecution"].clone();
                            if detached { break Ok(result); }
                            settled = Some(result);
                        }
                        Some("result") => {
                            let result = event["result"].clone();
                            let running = !control && matches!(result["details"]["cells"][0]["status"].as_str(), Some("detached" | "running" | "queued"));
                            if running && settled.is_none() && !cancelled {
                                let mut receipt = result::convert(&ctx, &host, result).await?;
                                if let Some(data) = &mut receipt.structured_json { data["cell_id"] = id.clone().into(); data["detached"] = true.into(); }
                                ctx.coordinator.detach_eval(id.clone(), receipt).await.map_err(failure)?;
                                detached = true;
                            } else { break Ok(settled.take().unwrap_or(result)); }
                        }
                        Some("call") => {
                            let call = event["id"].as_str().ok_or_else(|| failure("missing eval bridge id"))?.to_owned();
                            let cancel = ctx.cancellation.child_token();
                            cancellations.insert(call.clone(), cancel.clone());
                            let (ctx, host) = (ctx.clone(), Arc::clone(&host));
                            calls.spawn(async move { (call, result::dispatch(&ctx, &host.session, event, cancel).await) });
                        }
                        Some("cancel_call") => {
                            if let Some(cancel) = event["id"].as_str().and_then(|id| cancellations.get(id)) { cancel.cancel(); }
                        }
                        Some("error") => {
                            let error = event["error"].as_str().unwrap_or("eval cell failed");
                            let mut result = settled.take().unwrap_or_else(|| json!({"content":[],"details":{}}));
                            result["content"] = json!([{"type":"text","text":error}]);
                            result["details"]["isError"] = true.into();
                            break Ok(result);
                        }
                        Some("fatal") => break Err(failure(event["error"].as_str().unwrap_or("eval runtime failed"))),
                        _ => break Err(failure("invalid eval runtime message")),
                    }
                }
                result = calls.join_next(), if !calls.is_empty() => { let (call, result) = result.ok_or_else(|| failure("eval bridge stopped"))?.map_err(failure)?; cancellations.remove(&call); result?; }
                _ = steering.tick() => {
                    if cancel_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
                        host.terminate();
                        break Err(failure("eval kernel did not acknowledge cancellation; runtime terminated"));
                    }
                    if !control && !detached && !steered && !cancelled && ctx.coordinator.eval_has_steering(id.clone()).await.map_err(failure)? {
                        steered = true; host.send(json!({"type":"steer","id":id})).await?;
                    }
                }
            }
        }}.await;
        if outcome.is_err() {
            let _ = host.send(json!({"type":"cancel","id":id})).await;
        }
        for cancel in cancellations.values() {
            cancel.cancel();
        }
        while let Some(result) = calls.join_next().await {
            let (_, result) = result.map_err(failure)?;
            result?;
        }
        if cancelled {
            return Err(ToolError::Cancelled);
        }
        result::convert(&ctx, &host, outcome?).await
    }
    async fn close_run(&self, run_id: &str) -> Result<(), ToolError> {
        let hosts = {
            let mut hosts = self.hosts.lock().await;
            hosts
                .extract_if(.., |(run, _), _| run == run_id)
                .map(|(_, host)| host)
                .collect::<Vec<_>>()
        };
        for slot in hosts {
            if let Some(host) = slot.lock().await.take() {
                host.close().await?;
            }
        }
        Ok(())
    }
}

fn failure(error: impl std::fmt::Display) -> ToolError {
    ToolError::Execution(error.to_string())
}
fn program(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|path| path.join(name))
            .find(|path| path.is_file())
    })
}
fn available_languages(config: &EvalConfig) -> Vec<String> {
    config
        .languages
        .iter()
        .filter(|language| match language.as_str() {
            "js" => true,
            "py" => program("python3").is_some() || program("python").is_some(),
            "rb" => program("ruby").is_some(),
            "jl" => program("julia").is_some(),
            _ => false,
        })
        .cloned()
        .collect()
}
