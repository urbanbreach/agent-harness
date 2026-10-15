use super::{runtime::digest, *};
use crate::config::{HookLifecycleEvent as Hook, LifecycleHookConfig};
use serde_json::{json, Map, Value};
use std::time::{Duration, Instant};
#[cfg(test)]
mod compaction_tests;
mod context;

/// Largest `messages_text` a summarizing hook receives, in bytes.
pub(super) const MESSAGES_TEXT_LIMIT: usize = 56 * 1024;
/// Context fields that may exceed the generic 4096 character cap. They travel in
/// `HARNESS_HOOK_CONTEXT_JSON` only, never as their own environment variable, and each is
/// bounded in bytes so the whole JSON stays under the per-string environment limit.
const LARGE_FIELDS: [(&str, usize); 3] = [
    ("messages_text", MESSAGES_TEXT_LIMIT),
    ("previous_summary", 24 * 1024),
    ("custom_instructions", 8 * 1024),
];
/// Largest `HARNESS_HOOK_CONTEXT_JSON` the runtime hands a hook.
const CONTEXT_JSON_LIMIT: usize = 120 * 1024;

#[derive(Default)]
pub(super) struct Batch {
    pub executions: Vec<HookExecutionMetadata>,
    pub failure: Option<String>,
}

/// What a handler produced: its receipt status and text, plus the JSON object it printed
/// on stdout when it exited 0 and printed one.
pub(super) struct Executed {
    pub status: HookExecutionStatus,
    pub text: String,
    pub object: Option<Map<String, Value>>,
}

impl Executed {
    fn plain(status: HookExecutionStatus, text: impl Into<String>) -> Self {
        Self {
            status,
            text: text.into(),
            object: None,
        }
    }

    fn from_result(result: Result<crate::process::Output, crate::tool::ToolError>) -> Self {
        let output = match result {
            Ok(output) => output,
            Err(error) => return Self::plain(HookExecutionStatus::Failed, error.to_string()),
        };
        let success = output.status.success();
        let object = (success && !output.truncated)
            .then(|| parse_object(&output.stdout))
            .flatten();
        let text = format!(
            "{}{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            if success {
                String::new()
            } else {
                format!("\nexit status {}", output.status)
            }
        );
        Self {
            status: if success {
                HookExecutionStatus::Succeeded
            } else {
                HookExecutionStatus::Failed
            },
            text,
            object,
        }
    }
}

/// Stdout that is exactly one JSON object; anything else stays a plain receipt.
fn parse_object(stdout: &[u8]) -> Option<Map<String, Value>> {
    match serde_json::from_slice::<Value>(stdout.trim_ascii()).ok()? {
        Value::Object(object) => Some(object),
        _ => None,
    }
}

/// A handler that passed the policy checks and is ready to run, or why it will not.
pub(super) enum Prepared {
    Ready {
        command: tokio::process::Command,
        timeout: Duration,
    },
    Blocked(String),
    Failed(String),
    Skipped(String),
}

impl Prepared {
    fn immediate(self) -> Executed {
        match self {
            Self::Blocked(text) => Executed::plain(HookExecutionStatus::Blocked, text),
            Self::Failed(text) => Executed::plain(HookExecutionStatus::Failed, text),
            Self::Skipped(text) => Executed::plain(HookExecutionStatus::Skipped, text),
            Self::Ready { .. } => Executed::plain(HookExecutionStatus::Failed, "hook not run"),
        }
    }

    fn run_blocking(self) -> Executed {
        match self {
            Self::Ready { command, timeout } => {
                Executed::from_result(crate::process::run_blocking(command, timeout))
            }
            other => other.immediate(),
        }
    }

    /// Runs the handler off the coordinator thread; `None` means `cancel` fired first.
    pub(super) async fn run(self, cancel: &CancellationToken) -> Option<Executed> {
        match self {
            Self::Ready { command, timeout } => {
                match crate::process::run(command, timeout, cancel).await {
                    Err(crate::tool::ToolError::Cancelled) => None,
                    result => Some(Executed::from_result(result)),
                }
            }
            other => Some(other.immediate()),
        }
    }
}

/// One configured handler of an event, prepared on the coordinator thread so it can run
/// elsewhere.
pub(super) struct Invocation {
    pub index: usize,
    pub name: String,
    pub hook: LifecycleHookConfig,
    pub prepared: Prepared,
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
        self.record_hook_batch(task, batch)
    }

    /// Keeps the receipts on the task and turns a critical failure into an error.
    pub(super) fn record_hook_batch(
        &mut self,
        task: Option<&str>,
        batch: Batch,
    ) -> Result<(), CoordinatorError> {
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
        for (index, hook) in self
            .config
            .hook_runtime_config
            .hooks
            .lifecycle
            .iter()
            .enumerate()
            .filter(|(_, h)| h.event == event)
        {
            let name = hook_name(hook, event, index);
            let start = Instant::now();
            let executed = self
                .prepare_hook(hook, &name, actor, task, &fields)
                .run_blocking();
            self.account_hook(
                event,
                index,
                &name,
                hook,
                actor,
                task,
                start.elapsed(),
                executed,
                &mut batch,
            );
            if batch.failure.is_some() {
                break;
            }
        }
        batch
    }

    /// Every handler of `event` in configuration order, prepared for execution elsewhere.
    pub(super) fn hook_invocations(
        &self,
        event: Hook,
        actor: &EventActor,
        task: Option<&str>,
        fields: &Value,
    ) -> Vec<Invocation> {
        self.config
            .hook_runtime_config
            .hooks
            .lifecycle
            .iter()
            .enumerate()
            .filter(|(_, h)| h.event == event)
            .map(|(index, hook)| {
                let name = hook_name(hook, event, index);
                let prepared = self.prepare_hook(hook, &name, actor, task, fields);
                Invocation {
                    index,
                    name,
                    hook: hook.clone(),
                    prepared,
                }
            })
            .collect()
    }

    /// Receipt, failure warning and critical-failure bookkeeping for one finished handler.
    #[expect(
        clippy::too_many_arguments,
        reason = "One handler's identity and result travel together"
    )]
    pub(super) fn account_hook(
        &self,
        event: Hook,
        index: usize,
        name: &str,
        hook: &LifecycleHookConfig,
        actor: &EventActor,
        task: Option<&str>,
        elapsed: Duration,
        executed: Executed,
        batch: &mut Batch,
    ) {
        let Executed { status, text, .. } = executed;
        let output = self.redactor.redact_text(&text);
        let summary: String = output.chars().take(1024).collect();
        if matches!(
            status,
            HookExecutionStatus::Failed | HookExecutionStatus::Blocked
        ) {
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
            hook_name: self.redactor.redact_text(name),
            status,
            hook_event: Some(event.as_str().into()),
            command_digest: Some(digest(&hook.command.join("\0"))),
            output_digest: Some(digest(&output)),
            output_summary: Some(summary),
            duration_ms: Some(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)),
        });
    }

    fn prepare_hook(
        &self,
        hook: &LifecycleHookConfig,
        name: &str,
        actor: &EventActor,
        task: Option<&str>,
        fields: &Value,
    ) -> Prepared {
        if self.config.hook_runtime_config.suppress_execution {
            return Prepared::Skipped("suppressed during deterministic execution".into());
        }
        self.try_prepare_hook(hook, name, actor, task, fields)
            .unwrap_or_else(|error| Prepared::Failed(error.to_string()))
    }

    fn try_prepare_hook(
        &self,
        hook: &LifecycleHookConfig,
        name: &str,
        actor: &EventActor,
        task: Option<&str>,
        fields: &Value,
    ) -> Result<Prepared, Box<dyn std::error::Error>> {
        hook.validate()?;
        let allowlist = &self.config.hook_runtime_config.shell_allowlist;
        let executable = &hook.command[0];
        if !allowlist.executables.contains(executable) {
            return Ok(Prepared::Blocked(
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
            return Ok(Prepared::Blocked(
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
            let Some(text) = value.as_str() else {
                continue;
            };
            let text = self.redactor.redact_text(text);
            if let Some((_, limit)) = LARGE_FIELDS
                .iter()
                .find(|(large, _)| *large == key.as_str())
            {
                *value = Value::String(truncate_bytes(&text, *limit).to_owned());
                continue;
            }
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
        let mut json = serde_json::to_string(&context)?;
        while json.len() > CONTEXT_JSON_LIMIT {
            // Shed the biggest transcript field first; the rest is small by construction.
            let Some(Value::String(text)) = context.get_mut("messages_text") else {
                break;
            };
            if text.is_empty() {
                break;
            }
            let keep = truncate_bytes(text, text.len() / 2).len();
            text.truncate(keep);
            context.insert("messages_truncated".into(), Value::Bool(true));
            json = serde_json::to_string(&context)?;
        }
        command.env("HARNESS_HOOK_CONTEXT_JSON", json);
        // ponytail: configured hooks serialize the actor, bounded by their timeout; move gates to async intents if hook latency matters.
        Ok(Prepared::Ready {
            command,
            timeout: Duration::from_millis(hook.effective_timeout_ms()),
        })
    }
}

fn hook_name(hook: &LifecycleHookConfig, event: Hook, index: usize) -> String {
    hook.id
        .clone()
        .unwrap_or_else(|| format!("{}_{:02}", event.as_str(), index + 1))
}

/// `text` cut at the last character boundary at or below `limit` bytes.
pub(super) fn truncate_bytes(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
