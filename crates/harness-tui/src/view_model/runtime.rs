use std::borrow::Cow;

use harness_core::event::{EventV1, TaskTerminalScope};

use super::{sanitize_runtime_summary_fragment, sanitized_runtime_guidance};
use crate::app::{
    ActivityEntry, ActivityStatus, LifecycleShellState, RuntimeState, RuntimeStateKind,
    ToolCallDisplayStatus,
};

const NEXT_PROMPT: &str = "Type a prompt for the next turn…";
const RETRY: &str = "After review, adjust the draft, then retry or continue.";
const POST_RUN: &str =
    "Session shell preserved — use commands for replay, new, or quit after review.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeStateView<'a> {
    pub kind: RuntimeStateKind,
    pub summary: Cow<'a, str>,
    pub detail: Option<Cow<'a, str>>,
    pub composer_disabled: bool,
    pub composer_hint: &'static str,
}

impl From<RuntimeStateView<'_>> for RuntimeState {
    fn from(state: RuntimeStateView<'_>) -> Self {
        Self {
            kind: state.kind,
            summary: state.summary.into_owned(),
            detail: state.detail.map(Cow::into_owned),
            composer_disabled: state.composer_disabled,
            composer_hint: state.composer_hint.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PermissionRuntimeInput {
    pub summary: String,
    pub submission_pending: bool,
}

pub(crate) struct RuntimeStateInput<'a> {
    pub replay_mode: bool,
    pub lifecycle_shell_state: LifecycleShellState,
    pub continue_disabled_banner: Option<&'a str>,
    pub status_banner: Option<&'a str>,
    pub event_count: usize,
    pub last_event: Option<&'a EventV1>,
    pub latest_activity: Option<&'a ActivityEntry>,
    pub activity_count: usize,
    pub active_permission: Option<PermissionRuntimeInput>,
}

pub(crate) fn runtime_state(input: RuntimeStateInput<'_>) -> RuntimeStateView<'_> {
    use RuntimeStateKind as Kind;
    let mut state = RuntimeStateView {
        kind: Kind::Ready,
        summary: "ready for first turn".into(),
        detail: None,
        composer_disabled: false,
        composer_hint: NEXT_PROMPT,
    };
    if input.lifecycle_shell_state == LifecycleShellState::Startup {
        state.summary = input.continue_disabled_banner.map_or_else(
            || "startup ready".into(),
            |reason| format!("startup ready · {reason}").into(),
        );
        state.detail = input.continue_disabled_banner.map(Cow::Borrowed);
        state.composer_hint = "Ask anything... \"What is the tech stack of this project?\"";
        return state;
    }
    if input.lifecycle_shell_state == LifecycleShellState::PostRun {
        return post_run(input.last_event, state);
    }
    if let Some(banner) = input.status_banner {
        let lower = banner.to_ascii_lowercase();
        state.detail = Some(banner.into());
        if lower.contains("disconnected") {
            state.kind = Kind::Disconnected;
            state.summary = if input.event_count == 0 {
                "live event stream unavailable · reopen the TUI to connect"
            } else {
                "live event stream disconnected · reopen the TUI to reconnect"
            }
            .into();
            state.composer_disabled = true;
            state.composer_hint = "Draft preserved locally — reopen the TUI to reconnect.";
        } else if lower.contains("lagged") || lower.contains("replaying") {
            state.kind = Kind::Degraded;
            state.summary = format!("{banner} · sending paused until recovery").into();
            state.composer_disabled = true;
            state.composer_hint = "Draft preserved locally while recovery completes.";
        } else if lower.contains("failed")
            || lower.contains("error")
            || lower.contains("no session path")
            || lower.contains("request_digest=")
        {
            state.kind = Kind::Failure;
            state.composer_hint = RETRY;
            state.summary = if lower.contains("request_digest=") {
                state.detail = Some(sanitized_runtime_guidance().into());
                format!("runtime failure · {}", sanitized_runtime_guidance()).into()
            } else if input.replay_mode {
                "reload failed · inspect events or transcript".into()
            } else {
                "runtime failure · inspect transcript, then retry or continue".into()
            };
        } else {
            state.summary = banner.into();
        }
        return state;
    }
    if input.replay_mode && matches!(input.last_event, Some(EventV1::RunFailed(_))) {
        return post_run(input.last_event, state);
    }
    if let Some(permission) = input.active_permission {
        if permission.submission_pending {
            state.kind = Kind::PermissionPending;
            state.summary = format!(
                "decision submitted · awaiting confirmation · {}",
                permission.summary
            )
            .into();
            state.composer_disabled = true;
            state.composer_hint =
                "Composer disabled — wait for confirmation on the permission decision.";
        } else {
            state.kind = Kind::PermissionBlocked;
            state.summary = format!("decision required · {}", permission.summary).into();
            state.composer_hint = "Keep drafting locally while the request waits for review.";
        }
        state.detail = Some(permission.summary.into());
        return state;
    }
    if let Some(EventV1::TaskCancelled(cancelled)) = input.last_event
        && cancelled.task_scope != Some(TaskTerminalScope::ToolCall)
    {
        state.kind = Kind::Cancelled;
        state.detail = nonempty(&cancelled.reason);
        state.summary = state.detail.as_deref().map_or_else(
            || "last turn cancelled · ready to try again".into(),
            |reason| {
                format!(
                    "last turn cancelled · {}",
                    sanitize_runtime_summary_fragment(reason)
                )
                .into()
            },
        );
        state.composer_hint = "Type a prompt to retry the cancelled turn…";
        return state;
    }
    let Some(activity) = input.latest_activity else {
        if input.replay_mode {
            state.summary = format!("{} events loaded", input.event_count).into();
        }
        return state;
    };
    if activity.status == ActivityStatus::Streaming
        && let Some(state) = activity.tool_calls.last().and_then(tool_state)
    {
        return state;
    }
    let (kind, suffix, hint) = match activity.status {
        ActivityStatus::Queued => (
            Kind::Sending,
            "queued for next turn",
            "Draft another follow-up while this prompt waits…",
        ),
        ActivityStatus::Streaming if activity.transcript_text.is_empty() => (
            Kind::Sending,
            "response starting",
            "Draft the next prompt while the response starts…",
        ),
        ActivityStatus::Streaming => (
            Kind::Streaming,
            "response in progress",
            "Draft the next prompt while the response continues…",
        ),
        ActivityStatus::Done => (Kind::Success, "ready for next turn", NEXT_PROMPT),
        ActivityStatus::Error => {
            state.detail = activity.error_message.as_deref().map(Cow::Borrowed);
            (
                Kind::Failure,
                "inspect transcript, then retry or continue",
                RETRY,
            )
        }
    };
    state.kind = kind;
    state.summary = format!("turn {} · {suffix}", input.activity_count).into();
    state.composer_hint = hint;
    state
}

fn post_run<'a>(
    last_event: Option<&'a EventV1>,
    mut state: RuntimeStateView<'a>,
) -> RuntimeStateView<'a> {
    let (kind, summary, detail, hint) = match last_event {
        Some(EventV1::RunFailed(data)) => (
            RuntimeStateKind::Failure,
            "run failed · inspect transcript · session shell preserved",
            nonempty(&data.error),
            "Session shell preserved — inspect transcript, then use commands to recover, replay, or quit.",
        ),
        Some(EventV1::RunFinished(data)) => (RuntimeStateKind::Success, "run finished · session shell preserved", nonempty(&data.summary), POST_RUN),
        _ => (RuntimeStateKind::Ready, "run complete · session shell preserved", None, POST_RUN),
    };
    state.kind = kind;
    state.summary = summary.into();
    state.detail = detail;
    state.composer_disabled = true;
    state.composer_hint = hint;
    state
}

fn nonempty(text: &str) -> Option<Cow<'_, str>> {
    (!text.trim().is_empty()).then_some(Cow::Borrowed(text))
}

fn tool_state(tool: &crate::app::ToolCallEntry) -> Option<RuntimeStateView<'_>> {
    let (prefix, hint, detail) = match tool.status {
        ToolCallDisplayStatus::Queued => (
            "tool queued",
            "Draft the next prompt while the queued tool waits to start…",
            tool.transcript_summary().map(Cow::Owned),
        ),
        ToolCallDisplayStatus::Running => (
            "tool running",
            "Draft the next prompt while the tool runs…",
            tool.transcript_summary().map(Cow::Owned),
        ),
        ToolCallDisplayStatus::Succeeded => (
            "tool finished · waiting for final response",
            "Draft the next prompt while the assistant finishes after the tool result…",
            tool.truncated_output.as_deref().map(Cow::Borrowed),
        ),
        ToolCallDisplayStatus::Failed => (
            "tool failed",
            RETRY,
            tool.output_summary.as_deref().map(Cow::Borrowed),
        ),
        ToolCallDisplayStatus::PendingPermission => return None,
    };
    Some(RuntimeStateView {
        kind: if tool.status == ToolCallDisplayStatus::Failed {
            RuntimeStateKind::Failure
        } else {
            RuntimeStateKind::Streaming
        },
        summary: format!("{prefix} · {}", tool.effective_tool_id()).into(),
        detail,
        composer_disabled: false,
        composer_hint: hint,
    })
}
