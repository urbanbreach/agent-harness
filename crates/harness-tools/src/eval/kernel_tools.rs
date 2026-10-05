use super::*;

pub(super) async fn publish(
    ctx: &ToolContext,
    session: &Arc<harness_eval::Session>,
    args: &Value,
) -> Result<(), ToolError> {
    let Some(names) = args["tools"].as_array() else {
        return Ok(());
    };
    let names = names
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let described = match session.describe_kernel_tools(&names).await {
        Ok(described) => described,
        Err(error)
            if error
                .downcast_ref::<harness_eval::KernelToolError>()
                .is_some_and(|error| error.code == "tools_unavailable") =>
        {
            return Ok(())
        }
        Err(error) => return Err(failure(error)),
    };
    let tools = described["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["ok"] == true)
        .map(|entry| {
            let descriptor = entry["descriptor"].clone();
            Arc::new(KernelTool {
                name: descriptor["name"].as_str().unwrap_or_default().into(),
                description: descriptor["description"]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
                descriptor,
                session: Arc::clone(session),
                run: ctx.run_id.clone(),
            }) as Arc<dyn Tool>
        })
        .collect::<Vec<_>>();
    if !tools.is_empty() {
        ctx.coordinator
            .register_eval_tools(ctx.tool_call_id.to_string(), tools)
            .await
            .map_err(failure)?;
    }
    Ok(())
}

struct KernelTool {
    name: String,
    description: String,
    descriptor: Value,
    session: Arc<harness_eval::Session>,
    run: String,
}

#[async_trait::async_trait]
impl Tool for KernelTool {
    fn id(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters_json_schema(&self) -> Value {
        self.descriptor["input_schema"].clone()
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }
    fn permission_requests(&self, _args: &Value) -> Vec<(String, String)> {
        vec![("eval".into(), format!("kernel tool {}", self.name))]
    }

    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        if ctx.run_id != self.run || !self.session.is_alive() {
            return Err(failure("kernel tool belongs to a closed session"));
        }
        let tools = ctx
            .coordinator
            .eval_tool_catalog(ctx.tool_call_id.to_string())
            .await
            .map_err(failure)?;
        let mut allowed = tools
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| tool["name"].as_str())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for (name, helper) in [
            ("spawn_subagent", "__agent__"),
            ("get_command_or_subagent_output", "__output__"),
        ] {
            if allowed.iter().any(|tool| tool == name) {
                allowed.push(helper.into());
            }
        }
        allowed.push("__schema__".into());
        let request = json!({"name":self.name,"kernel_generation":self.descriptor["kernel_generation"],"definition_revision":self.descriptor["definition_revision"],"args":args,"call_id":ctx.tool_call_id});
        let mut events = self
            .session
            .invoke_kernel_tool(
                request,
                Some(json!({"tools":{"allow":allowed}})),
                tools,
                ctx.cancellation.clone(),
            )
            .await
            .map_err(failure)?;
        let mut calls = tokio::task::JoinSet::new();
        let mut cancellations = BTreeMap::new();
        let outcome = async { loop {
            tokio::select! {
                event = events.recv() => {
                    let event = event.ok_or_else(|| failure("kernel tool disconnected"))?;
                    match event["type"].as_str() {
                        Some("result") => return Ok(ToolResult::structured(event["result"].as_str().map_or_else(|| event["result"].to_string(), str::to_owned), json!({"value":event["result"]}))),
                        Some("error") => return Err(ToolError::Custom { code:event["error"]["code"].as_str().unwrap_or("kernel_tool_failed").into(), message:event["error"]["message"].as_str().unwrap_or("kernel tool failed").into() }),
                        Some("call") => {
                            let cancel = ctx.cancellation.child_token();
                            let id = event["id"].as_str().ok_or_else(|| failure("missing kernel call id"))?.to_owned();
                            cancellations.insert(id.clone(), cancel.clone());
                            let (ctx, session) = (ctx.clone(), Arc::clone(&self.session));
                            calls.spawn(async move { (id, result::dispatch(&ctx, &session, event, cancel).await) });
                        }
                        Some("cancel_call") => { if let Some(cancel) = event["id"].as_str().and_then(|id| cancellations.get(id)) { cancel.cancel(); } }
                        _ => {},
                    }
                }
                reply = calls.join_next(), if !calls.is_empty() => { let (id, result) = reply.ok_or_else(|| failure("kernel helper stopped"))?.map_err(failure)?; cancellations.remove(&id); result?; }
            }
        }}.await;
        for cancellation in cancellations.values() {
            cancellation.cancel();
        }
        while calls.join_next().await.is_some() {}
        outcome
    }
}
