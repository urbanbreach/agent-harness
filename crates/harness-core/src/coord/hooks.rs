use super::*;
use crate::event::HookExecutionStatus;
use async_trait::async_trait;
use std::process::ExitStatus;

pub(in crate::coord) async fn run_lifecycle_hooks<C>(
    clock: &C,
    executor: &(dyn LifecycleHookCommandExecutor + Send + Sync),
    runtime: &HookRuntimeConfig,
    context: HookInvocationContext,
) -> HookExecutionBatch
where
    C: Clock + ?Sized,
{
    let mut batch = HookExecutionBatch::default();

    for (index, hook) in runtime.hooks.lifecycle.iter().enumerate() {
        if hook.event != context.event {
            continue;
        }

        if runtime.suppress_execution {
            batch.hook_executions.push(HookExecutionMetadata {
                hook_name: hook_identifier(hook, index),
                status: HookExecutionStatus::Skipped,
                hook_event: Some(context.event.as_str().to_string()),
                command_digest: Some(digest12(hook.command.join("\u{0}").as_bytes())),
                output_digest: None,
                output_summary: Some("suppressed during deterministic execution".to_string()),
                duration_ms: Some(0),
            });
            continue;
        }

        let (metadata, failure) =
            execute_lifecycle_hook(clock, executor, runtime, hook, index, &context).await;
        batch.hook_executions.push(metadata);
        if hook.critical {
            if let Some(failure) = failure {
                batch.critical_failure = Some(failure);
                break;
            }
        }
    }

    batch
}

async fn execute_lifecycle_hook<C>(
    clock: &C,
    executor: &(dyn LifecycleHookCommandExecutor + Send + Sync),
    runtime: &HookRuntimeConfig,
    hook: &LifecycleHookConfig,
    index: usize,
    context: &HookInvocationContext,
) -> (HookExecutionMetadata, Option<String>)
where
    C: Clock + ?Sized,
{
    let hook_name = hook_identifier(hook, index);
    let command_digest = digest12(hook.command.join("\u{0}").as_bytes());
    let started_mono_ms = clock.mono_ms();

    let execution =
        execute_lifecycle_hook_command(executor, runtime, hook, &hook_name, context).await;
    let finished_mono_ms = clock.mono_ms();

    match execution {
        Ok((output_digest, output_summary)) => (
            HookExecutionMetadata {
                hook_name,
                status: HookExecutionStatus::Succeeded,
                hook_event: Some(context.event.as_str().to_string()),
                command_digest: Some(command_digest),
                output_digest: Some(output_digest),
                output_summary: Some(output_summary),
                duration_ms: Some(finished_mono_ms.saturating_sub(started_mono_ms)),
            },
            None,
        ),
        Err((reason, output_summary)) => {
            let failure = format!(
                "hook `{hook_name}` for `{}` failed: {reason}",
                context.event.as_str()
            );
            (
                HookExecutionMetadata {
                    hook_name,
                    status: HookExecutionStatus::Failed,
                    hook_event: Some(context.event.as_str().to_string()),
                    command_digest: Some(command_digest),
                    output_digest: Some(digest12(reason.as_bytes())),
                    output_summary: Some(output_summary),
                    duration_ms: Some(finished_mono_ms.saturating_sub(started_mono_ms)),
                },
                Some(failure),
            )
        }
    }
}

async fn execute_lifecycle_hook_command(
    executor: &(dyn LifecycleHookCommandExecutor + Send + Sync),
    runtime: &HookRuntimeConfig,
    hook: &LifecycleHookConfig,
    hook_name: &str,
    context: &HookInvocationContext,
) -> Result<(String, String), (String, String)> {
    let executable = hook.command.first().ok_or_else(|| {
        (
            format!("hook `{hook_name}` is missing a command executable"),
            "no output".to_string(),
        )
    })?;

    if !hook_executable_allowed(&runtime.shell_allowlist, executable) {
        return Err((
            format!("executable `{executable}` is not in the shell allowlist"),
            "no output".to_string(),
        ));
    }

    let cwd = resolve_hook_cwd(
        &runtime.shell_allowlist,
        &context.workspace_root,
        hook.cwd.as_deref(),
    )
    .map_err(|err| (err, "no output".to_string()))?;
    let invocation = LifecycleHookCommandInvocation {
        executable: executable.clone(),
        args: hook.command[1..].to_vec(),
        cwd: cwd.clone(),
        env: hook_environment(hook_name, context, &cwd, hook),
        timeout_ms: hook.timeout_ms,
    };
    let output = executor.execute(invocation).await.map_err(|reason| {
        let output_summary = "no output".to_string();
        (reason, output_summary)
    })?;

    let output_summary = summarize_hook_output(&output.stdout, &output.stderr);
    let output_digest = digest12(
        format!(
            "{}\u{0}{}\u{0}{:?}",
            output.stdout, output.stderr, output.status
        )
        .as_bytes(),
    );

    if output.status.success() {
        Ok((output_digest, output_summary))
    } else {
        Err((
            format!(
                "exit status {:?}: {output_summary}",
                output.status.code().unwrap_or(-1)
            ),
            output_summary,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleHookCommandInvocation {
    pub executable: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub timeout_ms: u64,
}

#[derive(Debug)]
pub struct LifecycleHookCommandOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

#[async_trait]
pub trait LifecycleHookCommandExecutor {
    async fn execute(
        &self,
        invocation: LifecycleHookCommandInvocation,
    ) -> Result<LifecycleHookCommandOutput, String>;
}

#[derive(Debug, Default)]
pub struct TokioLifecycleHookCommandExecutor;

#[async_trait]
impl LifecycleHookCommandExecutor for TokioLifecycleHookCommandExecutor {
    async fn execute(
        &self,
        invocation: LifecycleHookCommandInvocation,
    ) -> Result<LifecycleHookCommandOutput, String> {
        let mut command = tokio::process::Command::new(&invocation.executable);
        command.args(&invocation.args);
        command.current_dir(&invocation.cwd);
        command.kill_on_drop(true);
        for (key, value) in invocation.env {
            command.env(key, value);
        }

        let output = tokio::time::timeout(
            Duration::from_millis(invocation.timeout_ms),
            command.output(),
        )
        .await
        .map_err(|_| format!("timed out after {} ms", invocation.timeout_ms))?
        .map_err(|err| format!("failed to execute command: {err}"))?;

        Ok(LifecycleHookCommandOutput {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

fn hook_identifier(hook: &LifecycleHookConfig, index: usize) -> String {
    hook.id
        .clone()
        .unwrap_or_else(|| format!("{}_{:02}", hook.event.as_str(), index + 1))
}

fn hook_executable_allowed(allowlist: &ShellAllowlist, executable: &str) -> bool {
    allowlist
        .executables
        .iter()
        .any(|allowed| allowed == executable)
}

fn resolve_hook_cwd(
    allowlist: &ShellAllowlist,
    workspace_root: &Path,
    cwd: Option<&str>,
) -> Result<PathBuf, String> {
    let cwd = match cwd {
        Some(value) => workspace_root.join(value),
        None => workspace_root.to_path_buf(),
    };

    if allowlist.cwd_roots.is_empty() {
        return Ok(cwd);
    }

    let canonical_cwd = cwd
        .canonicalize()
        .map_err(|err| format!("failed to resolve cwd: {err}"))?;
    let allowed = allowlist.cwd_roots.iter().any(|root| {
        workspace_root
            .join(root)
            .canonicalize()
            .map(|allowed_root| canonical_cwd.starts_with(&allowed_root))
            .unwrap_or(false)
    });

    if allowed {
        Ok(canonical_cwd)
    } else {
        Err(format!(
            "cwd {} is not in the shell allowlist",
            canonical_cwd.display()
        ))
    }
}

fn hook_environment(
    hook_name: &str,
    context: &HookInvocationContext,
    cwd: &Path,
    hook: &LifecycleHookConfig,
) -> BTreeMap<String, String> {
    let mut env = hook.env.clone();
    env.insert("HARNESS_HOOK_ID".to_string(), hook_name.to_string());
    env.insert(
        "HARNESS_HOOK_EVENT".to_string(),
        context.event.as_str().to_string(),
    );
    env.insert("HARNESS_HOOK_RUN_ID".to_string(), context.run_id.clone());
    env.insert(
        "HARNESS_HOOK_WORKSPACE_ROOT".to_string(),
        context.workspace_root.display().to_string(),
    );
    env.insert(
        "HARNESS_HOOK_ARTIFACTS_DIR".to_string(),
        context.artifacts_dir.display().to_string(),
    );
    env.insert("HARNESS_HOOK_CWD".to_string(), cwd.display().to_string());
    if let Some(actor) = context.actor.as_ref() {
        env.insert(
            "HARNESS_HOOK_ACTOR_KIND".to_string(),
            format!("{:?}", actor.kind).to_ascii_lowercase(),
        );
        if let Some(actor_agent_id) = actor.agent_id.as_ref() {
            env.insert(
                "HARNESS_HOOK_ACTOR_AGENT_ID".to_string(),
                actor_agent_id.clone(),
            );
        }
    }
    if let Some(agent_id) = context.agent_id.as_ref() {
        env.insert("HARNESS_HOOK_AGENT_ID".to_string(), agent_id.clone());
    }
    if let Some(request_id) = context.request_id.as_ref() {
        env.insert("HARNESS_HOOK_REQUEST_ID".to_string(), request_id.clone());
    }
    if let Some(permission_id) = context.permission_id.as_ref() {
        env.insert(
            "HARNESS_HOOK_PERMISSION_ID".to_string(),
            permission_id.clone(),
        );
    }
    if let Some(task_id) = context.task_id.as_ref() {
        env.insert("HARNESS_HOOK_TASK_ID".to_string(), task_id.clone());
    }
    if let Some(tool_call_id) = context.tool_call_id.as_ref() {
        env.insert(
            "HARNESS_HOOK_TOOL_CALL_ID".to_string(),
            tool_call_id.clone(),
        );
    }
    if let Some(tool_id) = context.tool_id.as_ref() {
        env.insert("HARNESS_HOOK_TOOL_ID".to_string(), tool_id.clone());
    }
    if let Some(provider_id) = context.provider_id.as_ref() {
        env.insert("HARNESS_HOOK_PROVIDER_ID".to_string(), provider_id.clone());
    }
    if let Some(model_id) = context.model_id.as_ref() {
        env.insert("HARNESS_HOOK_MODEL_ID".to_string(), model_id.clone());
    }
    if let Some(parent_agent_id) = context.parent_agent_id.as_ref() {
        env.insert(
            "HARNESS_HOOK_PARENT_AGENT_ID".to_string(),
            parent_agent_id.clone(),
        );
    }
    if let Some(category) = context.category.as_ref() {
        env.insert("HARNESS_HOOK_CATEGORY".to_string(), category.clone());
    }
    if let Some(outcome) = context.outcome.as_ref() {
        env.insert("HARNESS_HOOK_OUTCOME".to_string(), outcome.clone());
    }
    if let Some(output_summary) = context.output_summary.as_ref() {
        env.insert(
            "HARNESS_HOOK_OUTPUT_SUMMARY".to_string(),
            output_summary.clone(),
        );
    }
    if let Some(failure_reason) = context.failure_reason.as_ref() {
        env.insert(
            "HARNESS_HOOK_FAILURE_REASON".to_string(),
            failure_reason.clone(),
        );
    }

    env.insert(
        "HARNESS_HOOK_CONTEXT_JSON".to_string(),
        json!({
            "hook_id": hook_name,
            "event": context.event.as_str(),
            "run_id": context.run_id,
            "workspace_root": context.workspace_root.display().to_string(),
            "artifacts_dir": context.artifacts_dir.display().to_string(),
            "cwd": cwd.display().to_string(),
            "actor": context.actor.as_ref().map(|actor| json!({
                "kind": format!("{:?}", actor.kind).to_ascii_lowercase(),
                "agent_id": actor.agent_id,
            })),
            "agent_id": context.agent_id,
            "request_id": context.request_id,
            "permission_id": context.permission_id,
            "task_id": context.task_id,
            "tool_call_id": context.tool_call_id,
            "tool_id": context.tool_id,
            "provider_id": context.provider_id,
            "model_id": context.model_id,
            "parent_agent_id": context.parent_agent_id,
            "category": context.category,
            "outcome": context.outcome,
            "output_summary": context.output_summary,
            "failure_reason": context.failure_reason,
        })
        .to_string(),
    );

    env
}

pub(super) fn summarize_hook_output(stdout: &str, stderr: &str) -> String {
    let stdout = non_empty_trimmed(stdout);
    let stderr = non_empty_trimmed(stderr);
    let combined = if stderr.is_none() {
        stdout
    } else if stdout.is_none() {
        stderr
    } else {
        Some("stdout/stderr captured")
    };

    combined
        .map(|output| truncate_with_ellipsis(output, 160))
        .unwrap_or_else(|| "no output".to_string())
}
