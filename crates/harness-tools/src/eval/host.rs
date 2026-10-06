use super::*;
use harness_eval::{Session, SessionOptions};
use tokio::sync::mpsc;

pub(super) struct Host {
    pub session: Arc<Session>,
    pub scratch: Arc<tempfile::TempDir>,
}
impl Host {
    pub async fn start(ctx: &ToolContext, config: &EvalConfig) -> Result<Self, ToolError> {
        let scratch = Arc::new(tempfile::Builder::new().prefix("harness-eval-").tempdir()?);
        let mut environment = tokio::process::Command::new("node");
        harness_core::process::environment(&mut environment);
        let environment = environment
            .as_std()
            .get_envs()
            .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value.to_owned())))
            .collect();
        let info = ctx.coordinator.run_info().await.map_err(failure)?;
        let owner = ctx.actor.agent_id.as_deref().unwrap_or(&ctx.run_id);
        let mut session_env = BTreeMap::from([
            ("PI_SESSION_ID".into(), owner.to_owned()),
            (
                "PI_SESSION_FILE".into(),
                info.events_path.to_string_lossy().into_owned(),
            ),
            (
                "PI_SESSION_CWD".into(),
                ctx.workspace_root.to_string_lossy().into_owned(),
            ),
        ]);
        if let Some(model) = &ctx.current_model_ref {
            let model = harness_core::agent::AgentModelRef::parse(model);
            session_env.insert("PI_PROVIDER".into(), model.provider_id);
            session_env.insert("PI_MODEL".into(), model.model_id);
        }
        if let Some(reasoning) = ctx
            .current_model_settings
            .as_ref()
            .and_then(|settings| settings.reasoning_effort.as_ref())
        {
            session_env.insert("PI_REASONING_LEVEL".into(), reasoning.clone());
        }
        let mut settings = serde_json::to_value(config).map_err(failure)?;
        if let Some(settings) = settings.as_object_mut() {
            settings.remove("languages");
            settings.remove("route_tools");
        }
        let session = Session::new(SessionOptions {
            environment,
            cwd: ctx.workspace_root.clone(),
            artifacts: scratch.path().join("artifacts"),
            local_dir: ctx
                .artifacts_dir
                .join("eval-local")
                .join(blake3::hash(owner.as_bytes()).to_hex().as_str()),
            languages: config.languages.clone(),
            session_env,
            settings: serde_json::from_value(settings).map_err(failure)?,
        })
        .map_err(failure)?;
        Ok(Self {
            session: Arc::new(session),
            scratch,
        })
    }
    pub fn terminate(&self) {
        self.session.terminate();
    }
    pub fn is_alive(&self) -> bool {
        self.session.is_alive()
    }
    pub async fn send(&self, value: Value) -> Result<(), ToolError> {
        self.session.send(value).await.map_err(failure)
    }
    pub async fn execute(
        &self,
        id: &str,
        args: Value,
        tools: Value,
        interactive: bool,
    ) -> Result<mpsc::Receiver<Value>, ToolError> {
        match self.session.execute(id, args, tools, interactive).await {
            Ok(events) => Ok(events),
            Err(error) => {
                let (sender, receiver) = mpsc::channel(1);
                sender
                    .send(json!({"type":"error","id":id,"error":error.to_string()}))
                    .await
                    .map_err(failure)?;
                Ok(receiver)
            }
        }
    }
    pub async fn close(&self) -> Result<(), ToolError> {
        self.session.close().await.map_err(failure)
    }
}
