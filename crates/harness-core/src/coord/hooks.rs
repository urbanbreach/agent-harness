use super::{runtime::digest, *};
use crate::config::{HookLifecycleEvent as Hook, LifecycleHookConfig};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
mod context;

#[derive(Default)]
pub(super) struct Batch {
    pub executions: Vec<HookExecutionMetadata>,
    pub failure: Option<String>,
}

impl Runtime {
    pub fn hook(
        &mut self,
        event: Hook,
        actor: &EventActor,
        task: Option<&str>,
        fields: Value,
    ) -> Result<(), CoordinatorError> {
        let batch = self.hooks(event, actor, task, fields);
        if let Some(job) = task.and_then(|id| self.running.get_mut(id)) {
            // ponytail: retain the first 128 hook receipts per task; export a separate log if more are needed.
            let remaining = 128usize.saturating_sub(job.hooks.len());
            job.hooks
                .extend(batch.executions.into_iter().take(remaining));
        }
        batch
            .failure
            .map_or(Ok(()), |error| Err(CoordinatorError::Invalid(error)))
    }

    pub fn hooks(
        &self,
        event: Hook,
        actor: &EventActor,
        task: Option<&str>,
        fields: Value,
    ) -> Batch {
        let mut batch = Batch::default();
        let runtime = &self.config.hook_runtime_config;
        for (index, hook) in runtime
            .hooks
            .lifecycle
            .iter()
            .enumerate()
            .filter(|(_, h)| h.event == event)
        {
            let name = hook
                .id
                .clone()
                .unwrap_or_else(|| format!("{}_{:02}", event.as_str(), index + 1));
            let start = Instant::now();
            let result = if runtime.suppress_execution {
                Ok((
                    HookExecutionStatus::Skipped,
                    "suppressed during deterministic execution".into(),
                ))
            } else {
                self.execute_hook(hook, &name, actor, task, &fields)
            };
            let (status, output) = match result {
                Ok(result) => result,
                Err(error) => (HookExecutionStatus::Failed, error.to_string()),
            };
            let output = self.redactor.redact_text(&output);
            let summary: String = output.chars().take(1024).collect();
            let failed = matches!(
                status,
                HookExecutionStatus::Failed | HookExecutionStatus::Blocked
            );
            if failed {
                let error = self.redactor.redact_text(&format!(
                    "hook {name} for {} failed: {summary}",
                    event.as_str()
                ));
                if let (Some(store), Some(info)) = (&self.store, &self.info) {
                    store.publish_live(LiveEventEnvelope {
                        event_id: format!("hook-{index}-{}", self.last_seq),
                        run_id: info.run_id.clone(),
                        mono_ms: self.clock.mono_ms(),
                        ts: None,
                        actor: actor.clone(),
                        correlation_id: task.map(str::to_owned),
                        causation_id: None,
                        stream_key: None,
                        payload: LiveEventV1::RuntimeWarning {
                            message: error.clone(),
                        },
                    });
                }
                if hook.critical {
                    batch.failure = Some(error);
                }
            }
            batch.executions.push(HookExecutionMetadata {
                hook_name: self.redactor.redact_text(&name),
                status,
                hook_event: Some(event.as_str().into()),
                command_digest: Some(digest(&hook.command.join("\0"))),
                output_digest: Some(digest(&output)),
                output_summary: Some(summary),
                duration_ms: Some(u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)),
            });
            if batch.failure.is_some() {
                break;
            }
        }
        batch
    }

    fn execute_hook(
        &self,
        hook: &LifecycleHookConfig,
        name: &str,
        actor: &EventActor,
        task: Option<&str>,
        fields: &Value,
    ) -> Result<(HookExecutionStatus, String), Box<dyn std::error::Error>> {
        hook.validate()?;
        let allowlist = &self.config.hook_runtime_config.shell_allowlist;
        let executable = &hook.command[0];
        if !allowlist.executables.contains(executable) {
            return Ok((
                HookExecutionStatus::Blocked,
                "executable is not in the shell allowlist".into(),
            ));
        }
        let info = self.info()?;
        let cwd = self
            .execution_cwd(actor)?
            .join(hook.cwd.as_deref().unwrap_or("."))
            .canonicalize()?;
        if !cwd.is_dir()
            || !cwd.starts_with(&info.workspace_root)
            || (!allowlist.cwd_roots.is_empty()
                && !allowlist.cwd_roots.iter().any(|root| {
                    info.workspace_root
                        .join(root)
                        .canonicalize()
                        .is_ok_and(|root| cwd.starts_with(root))
                }))
        {
            return Ok((
                HookExecutionStatus::Blocked,
                "hook cwd is outside the permitted workspace directories".into(),
            ));
        }
        crate::store::create_private_dir(&info.artifacts_dir)?;
        let mut context = self.hook_context(actor, task);
        if let Some(fields) = fields.as_object() {
            context.extend(fields.clone());
        }
        context.extend([
            ("hook_id".into(), json!(name)),
            ("event".into(), json!(hook.event.as_str())),
            ("run_id".into(), json!(info.run_id)),
            ("workspace_root".into(), json!(info.workspace_root)),
            ("artifacts_dir".into(), json!(info.artifacts_dir)),
            ("cwd".into(), json!(cwd)),
        ]);
        let mut command = tokio::process::Command::new(executable);
        crate::process::environment(&mut command);
        command
            .args(&hook.command[1..])
            .current_dir(cwd)
            .envs(&hook.env);
        for (key, value) in &mut context {
            if let Some(text) = value.as_str() {
                let text = self.redactor.redact_text(text);
                let text: String = text.chars().take(4096).collect();
                command.env(
                    format!(
                        "HARNESS_HOOK_{}",
                        if key == "hook_id" {
                            "ID".into()
                        } else {
                            key.to_ascii_uppercase()
                        }
                    ),
                    &text,
                );
                *value = Value::String(text);
            }
        }
        command.env(
            "HARNESS_HOOK_CONTEXT_JSON",
            serde_json::to_string(&context)?,
        );
        // ponytail: configured hooks serialize the actor, bounded by their timeout; move gates to async intents if hook latency matters.
        let output = crate::process::run_blocking(command, Duration::from_millis(hook.timeout_ms))?;
        Ok((
            if output.status.success() {
                HookExecutionStatus::Succeeded
            } else {
                HookExecutionStatus::Failed
            },
            format!(
                "{}{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
                if output.status.success() {
                    String::new()
                } else {
                    format!("\nexit status {}", output.status)
                }
            ),
        ))
    }
}
