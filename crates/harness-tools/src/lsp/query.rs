use super::*;
use std::time::Duration;
pub(super) struct Response {
    pub data: Value,
    pub primary_digest: blake3::Hash,
    pub versions: BTreeMap<String, i32>,
}
impl LspTool {
    pub(super) async fn query(
        &self,
        context: &ToolContext,
        args: Args,
    ) -> Result<Response, ToolError> {
        self.query_until(
            context,
            args,
            tokio::time::Instant::now() + Duration::from_secs(30),
        )
        .await
    }
    pub(super) async fn query_until(
        &self,
        context: &ToolContext,
        args: Args,
        deadline: tokio::time::Instant,
    ) -> Result<Response, ToolError> {
        if context.cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(failure("LSP request timed out"));
        }
        let (name, config, language) = server(&self.config, &args)?;
        let primary = context.resolve_workspace_path(&args.file)?;
        let uri = session::uri(&primary)?;
        let path = primary.clone();
        let text = tokio::task::spawn_blocking(move || {
            if path.metadata()?.len() > 512 * 1024 {
                return Err(failure("LSP source exceeds 512 KiB"));
            }
            crate::files::contents(&path)
        })
        .await
        .map_err(|_| failure("LSP source reader stopped"))??;
        if text.len() > 512 * 1024 {
            return Err(failure("LSP source exceeds 512 KiB"));
        }
        let (method, capability, params) = request(&args, &uri, &text)?;
        let mut sessions = tokio::select! {
            biased;
            () = context.cancellation.cancelled() => return Err(ToolError::Cancelled),
            sessions = tokio::time::timeout_at(deadline, self.sessions.lock()) => sessions.map_err(|_| failure("LSP request timed out waiting for another query"))?,
        };
        let key = (context.run_id.clone(), name.clone());
        if !sessions.contains_key(&key) {
            if sessions.len() >= 16 {
                return Err(failure("LSP session limit reached (16 servers)"));
            }
            sessions.insert(
                key.clone(),
                session::Session::start(&config, &context.workspace_root)?,
            );
        }
        let session = sessions
            .get_mut(&key)
            .ok_or_else(|| failure("LSP session unavailable"))?;
        let result = tokio::select! {
            biased;
            () = context.cancellation.cancelled() => Err(ToolError::Cancelled),
            result = tokio::time::timeout_at(deadline, async {
                session.initialize().await?;
                session.document(&uri, &language, &text).await?;
                if matches!(args.operation, Operation::WorkspaceDiagnostics) && session.capabilities["diagnosticProvider"]["workspaceDiagnostics"] != true {
                    return Ok(None);
                }
                if matches!(args.operation, Operation::FileDiagnostics) && !supports(&session.capabilities, "diagnosticProvider") {
                    return session.published_diagnostics(&uri).await.map(Some);
                }
                if !supports(&session.capabilities, capability) {
                    return Err(failure("LSP server does not support this operation"));
                }
                if matches!(args.operation, Operation::Rename(_)) {
                    let prepared = if session.capabilities["renameProvider"]["prepareProvider"] == true {
                        let result = session.request("textDocument/prepareRename", json!({"textDocument":params["textDocument"],"position":params["position"]})).await?;
                        if result.is_null() { return Err(failure("the symbol cannot be renamed")); }
                        result
                    } else { Value::Null };
                    let edit = session.request(method, params).await?;
                    return Ok(Some(json!({"workspaceEdit":edit,"prepareRename":prepared})));
                }
                let result = session.request(method, params).await?;
                if matches!(args.operation, Operation::IncomingCalls | Operation::OutgoingCalls) {
                    if result.is_null() { return Ok(Some(json!([]))); }
                    let items = result.as_array().filter(|v| v.len() <= 16).ok_or_else(|| failure("invalid or oversized LSP call hierarchy"))?;
                    let mut calls = Vec::new();
                    let mut bytes = 0;
                    for item in items {
                        let method = if matches!(args.operation, Operation::IncomingCalls) { "callHierarchy/incomingCalls" } else { "callHierarchy/outgoingCalls" };
                        let result = session.request(method, json!({"item":item})).await?;
                        if result.is_null() { continue; }
                        bytes += result.to_string().len();
                        if bytes > wire::LIMIT { return Err(failure("LSP call hierarchy exceeds 4 MiB")); }
                        let result = result.as_array().ok_or_else(|| failure("invalid LSP call hierarchy result"))?;
                        calls.extend(result.iter().cloned());
                        if calls.len() > 4096 { return Err(failure("LSP call hierarchy exceeds 4096 entries")); }
                    }
                    return Ok(Some(Value::Array(calls)));
                }
                Ok(Some(result))
            }) => result.unwrap_or_else(|_| Err(failure("LSP request timed out"))),
        };
        let result = match result {
            Ok(Some(result)) => Ok(result),
            Ok(None) => diagnostics::workspace(session, context, &config, primary, deadline).await,
            Err(error) => Err(error),
        };
        match result {
            Ok(result) => Ok(Response {
                data: json!({"server":name,"result":result,"status":session.status}),
                primary_digest: blake3::hash(text.as_bytes()),
                versions: if matches!(args.operation, Operation::Rename(_)) {
                    session.versions()
                } else {
                    BTreeMap::new()
                },
            }),
            Err(error) => {
                if let Some(mut session) = sessions.remove(&key) {
                    let _ = session.close().await;
                }
                Err(error)
            }
        }
    }
}
