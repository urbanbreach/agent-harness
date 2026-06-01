use harness_core::event::EventV1;

use crate::app::{
    ActivityEntry, ActivityStatus, Focus, LifecycleShellState, RuntimeState, RuntimeStateKind,
    ToolCallDisplayStatus,
};
use crate::text::non_empty_preserved_string;
use crate::Action;
use harness_core::proj::RunStatus;

const POST_RUN_COMPOSER_HINT: &str =
    "Session shell preserved — use commands for replay, new, or quit after review.";
const POST_RUN_FAILURE_COMPOSER_HINT: &str =
    "Session shell preserved — inspect transcript, then use commands to recover, replay, or quit.";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupCardViewModel {
    pub metadata: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FooterHint {
    pub action: Action,
    pub label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FooterHintsViewModel {
    pub prefix: Option<&'static str>,
    pub hints: Vec<FooterHint>,
}

pub(crate) struct FooterHintsInput {
    pub replay_mode: bool,
    pub review_surface_active: bool,
    pub startup_shell_visible: bool,
    pub focus: Focus,
    pub composer_disabled: bool,
    pub completed_session_shell_active: bool,
    pub continued_live_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageBrowserViewModel {
    pub filter_input: String,
    pub rows: Vec<LineageBrowserRowViewModel>,
    pub empty_message: Option<String>,
    pub selected_run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageBrowserRowViewModel {
    pub run_id: String,
    pub title: String,
    pub depth: usize,
    pub parent_run_id: Option<String>,
    pub status: Option<RunStatus>,
    pub updated_at: Option<String>,
    pub profile: Option<String>,
    pub provider_model: Option<String>,
    pub child_count: usize,
    pub expanded: bool,
    pub selected: bool,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkSelectorViewModel {
    pub filter_input: String,
    pub rows: Vec<ForkSelectorRowViewModel>,
    pub empty_message: Option<String>,
    pub selected_cutoff_seq: Option<u64>,
    pub confirmed_cutoff_seq: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkSelectorRowViewModel {
    pub cutoff_seq: u64,
    pub event_count: usize,
    pub run_id: Option<String>,
    pub status: Option<RunStatus>,
    pub event_id: Option<String>,
    pub event_kind: &'static str,
    pub prompt_text: String,
    pub timestamp: Option<String>,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlDockVariant {
    Startup,
    Live,
    ReplayReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlDockSummarySegmentKind {
    Orchestration,
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlDockSummaryTone {
    Secondary,
    Accent,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlDockSummarySegment {
    pub kind: ControlDockSummarySegmentKind,
    pub text: String,
    pub tone: ControlDockSummaryTone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeContextLabel {
    Launch,
    CurrentRuntime,
    ContinuedRuntime,
    RecordedRuntimeReadOnly,
}

impl RuntimeContextLabel {
    fn text(self) -> &'static str {
        match self {
            Self::Launch => "Launch",
            Self::CurrentRuntime => "Context",
            Self::ContinuedRuntime => "Continued runtime",
            Self::RecordedRuntimeReadOnly => "Recorded runtime · read-only",
        }
    }

    fn allows_next_turns_segment(self) -> bool {
        matches!(self, Self::CurrentRuntime | Self::ContinuedRuntime)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeContextGrammar {
    pub primary_summary: String,
    pub summary_segment: Option<ControlDockSummarySegment>,
}

pub(crate) struct RuntimeContextGrammarInput {
    pub label: RuntimeContextLabel,
    pub identity: String,
    pub next_turn_identity: Option<String>,
}

pub(crate) fn runtime_context_grammar(input: RuntimeContextGrammarInput) -> RuntimeContextGrammar {
    let identity = sanitize_runtime_summary_fragment(input.identity.trim());
    let primary_summary = format!("{}: {identity}", input.label.text());
    let summary_segment = input
        .label
        .allows_next_turns_segment()
        .then_some(input.next_turn_identity)
        .flatten()
        .as_deref()
        .map(str::trim)
        .filter(|identity| !identity.is_empty())
        .map(sanitize_runtime_summary_fragment)
        .map(|identity| ControlDockSummarySegment {
            kind: ControlDockSummarySegmentKind::Orchestration,
            text: format!("Next turns: {identity}"),
            tone: ControlDockSummaryTone::Secondary,
        });

    RuntimeContextGrammar {
        primary_summary,
        summary_segment,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlDockViewModel {
    pub variant: ControlDockVariant,
    pub runtime_context: Option<String>,
    pub runtime_badge: String,
    pub runtime_kind: RuntimeStateKind,
    pub primary_summary: String,
    pub summary_segment: Option<ControlDockSummarySegment>,
    pub composer_body: String,
    pub composer_disclosure: String,
    pub composer_focused: bool,
    pub composer_disabled: bool,
}

pub(crate) enum ControlDockInput {
    Startup {
        runtime_context: Option<String>,
        runtime_state: RuntimeState,
        primary_summary: String,
        composer_body: String,
        composer_disclosure: String,
        composer_focused: bool,
    },
    Live {
        runtime_context: Option<String>,
        runtime_state: RuntimeState,
        primary_summary: String,
        summary_segment: Option<ControlDockSummarySegment>,
        composer_body: String,
        composer_disclosure: String,
        composer_focused: bool,
    },
    ReplayReadOnly {
        runtime_context: Option<String>,
        runtime_state: RuntimeState,
        primary_summary: String,
        composer_body: String,
        composer_disclosure: String,
        composer_focused: bool,
    },
}

pub(crate) fn control_dock_view_model(input: ControlDockInput) -> ControlDockViewModel {
    match input {
        ControlDockInput::Startup {
            runtime_context,
            runtime_state,
            primary_summary,
            composer_body,
            composer_disclosure,
            composer_focused,
        } => ControlDockViewModel {
            variant: ControlDockVariant::Startup,
            runtime_context,
            runtime_badge: runtime_state.kind.label().to_string(),
            runtime_kind: runtime_state.kind,
            primary_summary,
            summary_segment: None,
            composer_body,
            composer_disclosure,
            composer_focused,
            composer_disabled: runtime_state.composer_disabled,
        },
        ControlDockInput::Live {
            runtime_context,
            runtime_state,
            primary_summary,
            summary_segment,
            composer_body,
            composer_disclosure,
            composer_focused,
        } => ControlDockViewModel {
            variant: ControlDockVariant::Live,
            runtime_context,
            runtime_badge: runtime_state.kind.label().to_string(),
            runtime_kind: runtime_state.kind,
            primary_summary,
            summary_segment,
            composer_body,
            composer_disclosure,
            composer_focused,
            composer_disabled: runtime_state.composer_disabled,
        },
        ControlDockInput::ReplayReadOnly {
            runtime_context,
            runtime_state,
            primary_summary,
            composer_body,
            composer_disclosure,
            composer_focused,
        } => ControlDockViewModel {
            variant: ControlDockVariant::ReplayReadOnly,
            runtime_context,
            runtime_badge: runtime_state.kind.label().to_string(),
            runtime_kind: runtime_state.kind,
            primary_summary,
            summary_segment: None,
            composer_body,
            composer_disclosure,
            composer_focused,
            composer_disabled: true,
        },
    }
}

pub(crate) fn runtime_state(input: RuntimeStateInput<'_>) -> RuntimeState {
    match input.lifecycle_shell_state {
        LifecycleShellState::Startup => startup_runtime_state(input.continue_disabled_banner),
        LifecycleShellState::PostRun => post_run_runtime_state(input.last_event),
        LifecycleShellState::None => {
            if let Some(state) = status_banner_runtime_state(
                input.status_banner,
                input.event_count == 0,
                input.replay_mode,
            ) {
                return state;
            }

            if input.replay_mode && matches!(input.last_event, Some(EventV1::RunFailed(_))) {
                return post_run_runtime_state(input.last_event);
            }

            if let Some(permission) = input.active_permission.as_ref() {
                return permission_runtime_state(permission);
            }

            if let Some(EventV1::TaskCancelled(cancelled)) = input.last_event {
                if !matches!(
                    cancelled.task_scope,
                    Some(harness_core::event::TaskTerminalScope::ToolCall)
                ) {
                    let detail = non_empty_preserved_string(&cancelled.reason);
                    let summary = detail
                        .as_deref()
                        .map(|reason| {
                            format!(
                                "last turn cancelled · {}",
                                sanitize_runtime_summary_fragment(reason)
                            )
                        })
                        .unwrap_or_else(|| "last turn cancelled · ready to try again".to_string());
                    return RuntimeState {
                        kind: RuntimeStateKind::Cancelled,
                        summary,
                        detail,
                        composer_disabled: false,
                        composer_hint: "Type a prompt to retry the cancelled turn…".to_string(),
                    };
                }
            }

            if let Some(activity) = input.latest_activity {
                let summary = activity_status_summary(activity, input.activity_count);
                if let Some(state) = tool_runtime_state(activity) {
                    return state;
                }

                return match activity.status {
                    ActivityStatus::Queued => RuntimeState {
                        kind: RuntimeStateKind::Sending,
                        summary: format!("{summary} · queued for next turn"),
                        detail: None,
                        composer_disabled: false,
                        composer_hint: "Draft another follow-up while this prompt waits…"
                            .to_string(),
                    },
                    ActivityStatus::Streaming if activity.transcript_text.is_empty() => {
                        RuntimeState {
                            kind: RuntimeStateKind::Sending,
                            summary: format!("{summary} · response starting"),
                            detail: None,
                            composer_disabled: false,
                            composer_hint: "Draft the next prompt while the response starts…"
                                .to_string(),
                        }
                    }
                    ActivityStatus::Streaming => RuntimeState {
                        kind: RuntimeStateKind::Streaming,
                        summary: format!("{summary} · response in progress"),
                        detail: None,
                        composer_disabled: false,
                        composer_hint: "Draft the next prompt while the response continues…"
                            .to_string(),
                    },
                    ActivityStatus::Done => RuntimeState {
                        kind: RuntimeStateKind::Success,
                        summary: format!("{summary} · ready for next turn"),
                        detail: None,
                        composer_disabled: false,
                        composer_hint: "Type a prompt for the next turn…".to_string(),
                    },
                    ActivityStatus::Error => RuntimeState {
                        kind: RuntimeStateKind::Failure,
                        summary: format!("{summary} · inspect transcript, then retry or continue"),
                        detail: activity.error_message.clone(),
                        composer_disabled: false,
                        composer_hint: "After review, adjust the draft, then retry or continue."
                            .to_string(),
                    },
                };
            }

            RuntimeState {
                kind: RuntimeStateKind::Ready,
                summary: if input.replay_mode {
                    format!("{} events loaded", input.event_count)
                } else {
                    "ready for first turn".to_string()
                },
                detail: None,
                composer_disabled: false,
                composer_hint: "Type a prompt for the next turn…".to_string(),
            }
        }
    }
}

pub(crate) fn post_run_handoff_notice(can_reopen: bool) -> Option<&'static str> {
    (!can_reopen).then_some("current run cannot be reopened")
}

pub(crate) fn startup_card_view_model(
    startup_mode: bool,
    launch_mode_label: Option<&str>,
    profile: &str,
    provider: &str,
    model: &str,
) -> StartupCardViewModel {
    let _ = (startup_mode, launch_mode_label, provider);
    StartupCardViewModel {
        metadata: format!("Launch: {profile} · {model}"),
    }
}

pub(crate) fn footer_hints_view_model(input: FooterHintsInput) -> FooterHintsViewModel {
    let _ = input.continued_live_run;
    let hints = if input.replay_mode {
        vec![
            FooterHint {
                action: Action::Help,
                label: "shortcuts",
            },
            FooterHint {
                action: Action::FocusNext,
                label: "focus",
            },
            FooterHint {
                action: Action::Reload,
                label: "reload",
            },
            FooterHint {
                action: Action::Quit,
                label: "quit",
            },
        ]
    } else if !input.replay_mode && input.review_surface_active {
        vec![
            FooterHint {
                action: Action::CloseReviewSurface,
                label: "convo",
            },
            FooterHint {
                action: Action::Palette,
                label: "commands",
            },
            FooterHint {
                action: Action::Quit,
                label: "quit",
            },
        ]
    } else if input.startup_shell_visible && input.focus == Focus::List {
        vec![
            FooterHint {
                action: Action::Palette,
                label: "open",
            },
            FooterHint {
                action: Action::Quit,
                label: "quit",
            },
        ]
    } else if input.startup_shell_visible {
        vec![
            FooterHint {
                action: Action::SubmitPrompt,
                label: "send",
            },
            FooterHint {
                action: Action::Palette,
                label: "open",
            },
            FooterHint {
                action: Action::Quit,
                label: "quit",
            },
        ]
    } else if input.completed_session_shell_active {
        completed_live_shell_footer_hints()
    } else if input.composer_disabled {
        disabled_live_shell_footer_hints()
    } else {
        vec![
            FooterHint {
                action: Action::SubmitPrompt,
                label: "send",
            },
            FooterHint {
                action: Action::Palette,
                label: "commands",
            },
            FooterHint {
                action: Action::Quit,
                label: "quit",
            },
        ]
    };

    FooterHintsViewModel {
        prefix: None,
        hints,
    }
}

fn disabled_live_shell_footer_hints() -> Vec<FooterHint> {
    vec![
        FooterHint {
            action: Action::Palette,
            label: "commands",
        },
        FooterHint {
            action: Action::Quit,
            label: "quit",
        },
    ]
}

fn completed_live_shell_footer_hints() -> Vec<FooterHint> {
    vec![
        FooterHint {
            action: Action::FocusNext,
            label: "focus",
        },
        FooterHint {
            action: Action::Palette,
            label: "commands",
        },
        FooterHint {
            action: Action::Quit,
            label: "quit",
        },
    ]
}

fn startup_runtime_state(continue_disabled_banner: Option<&str>) -> RuntimeState {
    let detail = continue_disabled_banner.map(str::to_string);
    let summary = detail
        .as_deref()
        .map(|reason| format!("startup ready · {reason}"))
        .unwrap_or_else(|| "startup ready".to_string());

    RuntimeState {
        kind: RuntimeStateKind::Ready,
        summary,
        detail,
        composer_disabled: false,
        composer_hint: "Ask anything... \"What is the tech stack of this project?\"".to_string(),
    }
}

fn post_run_runtime_state(last_event: Option<&EventV1>) -> RuntimeState {
    match last_event {
        Some(EventV1::RunFailed(data)) => RuntimeState {
            kind: RuntimeStateKind::Failure,
            summary: "run failed · inspect transcript · session shell preserved".to_string(),
            detail: non_empty_preserved_string(&data.error),
            composer_disabled: true,
            composer_hint: POST_RUN_FAILURE_COMPOSER_HINT.to_string(),
        },
        Some(EventV1::RunFinished(data)) => RuntimeState {
            kind: RuntimeStateKind::Success,
            summary: "run finished · session shell preserved".to_string(),
            detail: non_empty_preserved_string(&data.summary),
            composer_disabled: true,
            composer_hint: POST_RUN_COMPOSER_HINT.to_string(),
        },
        _ => RuntimeState {
            kind: RuntimeStateKind::Ready,
            summary: "run complete · session shell preserved".to_string(),
            detail: None,
            composer_disabled: true,
            composer_hint: POST_RUN_COMPOSER_HINT.to_string(),
        },
    }
}

fn status_banner_runtime_state(
    banner: Option<&str>,
    no_events_loaded: bool,
    replay_mode: bool,
) -> Option<RuntimeState> {
    let banner = banner?;
    let lower = banner.to_ascii_lowercase();

    if lower.contains("disconnected") {
        return Some(RuntimeState {
            kind: RuntimeStateKind::Disconnected,
            summary: if no_events_loaded {
                "live event stream unavailable · reopen the TUI to connect".to_string()
            } else {
                "live event stream disconnected · reopen the TUI to reconnect".to_string()
            },
            detail: Some(banner.to_string()),
            composer_disabled: true,
            composer_hint: "Draft preserved locally — reopen the TUI to reconnect.".to_string(),
        });
    }

    if lower.contains("lagged") || lower.contains("replaying") {
        return Some(RuntimeState {
            kind: RuntimeStateKind::Degraded,
            summary: format!("{banner} · sending paused until recovery"),
            detail: Some(banner.to_string()),
            composer_disabled: true,
            composer_hint: "Draft preserved locally while recovery completes.".to_string(),
        });
    }

    if lower.contains("failed")
        || lower.contains("error")
        || lower.contains("no session path")
        || lower.contains("request_digest=")
    {
        let detail = if lower.contains("request_digest=") {
            Some(sanitized_runtime_guidance().to_string())
        } else {
            Some(banner.to_string())
        };
        return Some(RuntimeState {
            kind: RuntimeStateKind::Failure,
            summary: if lower.contains("request_digest=") {
                format!("runtime failure · {}", sanitized_runtime_guidance())
            } else if replay_mode {
                "reload failed · inspect events or transcript".to_string()
            } else {
                "runtime failure · inspect transcript, then retry or continue".to_string()
            },
            detail,
            composer_disabled: false,
            composer_hint: "After review, adjust the draft, then retry or continue.".to_string(),
        });
    }

    Some(RuntimeState {
        kind: RuntimeStateKind::Ready,
        summary: banner.to_string(),
        detail: Some(banner.to_string()),
        composer_disabled: false,
        composer_hint: "Type a prompt for the next turn…".to_string(),
    })
}

fn permission_runtime_state(permission: &PermissionRuntimeInput) -> RuntimeState {
    if permission.submission_pending {
        RuntimeState {
            kind: RuntimeStateKind::PermissionPending,
            summary: format!(
                "decision submitted · awaiting confirmation · {}",
                permission.summary
            ),
            detail: Some(permission.summary.clone()),
            composer_disabled: true,
            composer_hint: "Composer disabled — wait for confirmation on the permission decision."
                .to_string(),
        }
    } else {
        RuntimeState {
            kind: RuntimeStateKind::PermissionBlocked,
            summary: format!("decision required · {}", permission.summary),
            detail: Some(permission.summary.clone()),
            composer_disabled: false,
            composer_hint: "Keep drafting locally while the request waits for review.".to_string(),
        }
    }
}

fn activity_status_summary(_activity: &ActivityEntry, turn_count: usize) -> String {
    format!("turn {turn_count}")
}

fn sanitize_runtime_summary_fragment(detail: &str) -> String {
    if detail.to_ascii_lowercase().contains("request_digest=") {
        sanitized_runtime_guidance().to_string()
    } else {
        detail.to_string()
    }
}

fn sanitized_runtime_guidance() -> &'static str {
    "check transcript for details"
}

fn tool_runtime_state(activity: &ActivityEntry) -> Option<RuntimeState> {
    if activity.status != ActivityStatus::Streaming {
        return None;
    }

    let tool_call = activity.tool_calls.last()?;
    match tool_call.status {
        ToolCallDisplayStatus::PendingPermission => None,
        ToolCallDisplayStatus::Queued => Some(RuntimeState {
            kind: RuntimeStateKind::Streaming,
            summary: format!("tool queued · {}", tool_call.effective_tool_id()),
            detail: tool_call.transcript_summary(),
            composer_disabled: false,
            composer_hint: "Draft the next prompt while the queued tool waits to start…"
                .to_string(),
        }),
        ToolCallDisplayStatus::Running => Some(RuntimeState {
            kind: RuntimeStateKind::Streaming,
            summary: format!("tool running · {}", tool_call.effective_tool_id()),
            detail: tool_call.transcript_summary(),
            composer_disabled: false,
            composer_hint: "Draft the next prompt while the tool runs…".to_string(),
        }),
        ToolCallDisplayStatus::Succeeded => Some(RuntimeState {
            kind: RuntimeStateKind::Streaming,
            summary: format!(
                "tool finished · waiting for final response · {}",
                tool_call.effective_tool_id()
            ),
            detail: tool_call.truncated_output.clone(),
            composer_disabled: false,
            composer_hint:
                "Draft the next prompt while the assistant finishes after the tool result…"
                    .to_string(),
        }),
        ToolCallDisplayStatus::Failed => Some(RuntimeState {
            kind: RuntimeStateKind::Failure,
            summary: format!("tool failed · {}", tool_call.effective_tool_id()),
            detail: tool_call.output_summary.clone(),
            composer_disabled: false,
            composer_hint: "After review, adjust the draft, then retry or continue.".to_string(),
        }),
    }
}

#[cfg(test)]
fn control_dock_runtime_fixture(
    kind: RuntimeStateKind,
    summary: &str,
    composer_disabled: bool,
    composer_hint: &str,
) -> RuntimeState {
    RuntimeState {
        kind,
        summary: summary.to_string(),
        detail: None,
        composer_disabled,
        composer_hint: composer_hint.to_string(),
    }
}

#[cfg(test)]
fn runtime_tool_identity_fixture(status: ToolCallDisplayStatus) -> ActivityEntry {
    ActivityEntry {
        request_id: "req_tool_identity".to_string(),
        profile_label: "build".to_string(),
        model_id: "gpt-5.4".to_string(),
        provider_id: "default".to_string(),
        status: ActivityStatus::Streaming,
        user_message: None,
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: String::new(),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: vec![crate::app::ToolCallEntry {
            tool_call_id: "tc_task".to_string(),
            tool_id: "task".to_string(),
            canonical_tool_id: Some("agent.spawn".to_string()),
            alias_source_tool_id: Some("task".to_string()),
            resolved_tool_identity: Some(harness_core::event::ResolvedToolIdentity {
                invoked_tool_id: Some("task".to_string()),
                effective_tool_id: None,
                canonical_tool_id: Some("agent.spawn".to_string()),
                alias_source_tool_id: Some("task".to_string()),
            }),
            args_summary: r#"{"description":"inspect"}"#.to_string(),
            args_digest: "digest-task".to_string(),
            lifecycle_state: Some(match status {
                ToolCallDisplayStatus::PendingPermission | ToolCallDisplayStatus::Queued => {
                    harness_core::event::ToolCallLifecycleState::Pending
                }
                ToolCallDisplayStatus::Running => {
                    harness_core::event::ToolCallLifecycleState::Running
                }
                ToolCallDisplayStatus::Succeeded => {
                    harness_core::event::ToolCallLifecycleState::Completed
                }
                ToolCallDisplayStatus::Failed => harness_core::event::ToolCallLifecycleState::Error,
            }),
            status,
            output_summary: Some("tool failed summary".to_string()),
            output_digest: None,
            output_json: None,
            truncated_output: Some("tool succeeded summary".to_string()),
            edit: None,
            lineage: None,
            artifact_refs: Vec::new(),
            timing_elapsed_ms: None,
            permissions: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
            first_timestamp: None,
            last_timestamp: None,
        }],
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
    }
}

#[cfg(test)]
pub(crate) fn exact_test_control_dock_view_model_handles_live_runtime_variants() {
    let streaming = control_dock_view_model(ControlDockInput::Live {
        runtime_context: Some("live".to_string()),
        runtime_state: control_dock_runtime_fixture(
            RuntimeStateKind::Streaming,
            "turn 3 · response in progress",
            false,
            "Draft the next prompt while the response continues…",
        ),
        primary_summary: "turn 3 · response in progress".to_string(),
        summary_segment: Some(ControlDockSummarySegment {
            kind: ControlDockSummarySegmentKind::Tool,
            text: "tool bash running".to_string(),
            tone: ControlDockSummaryTone::Accent,
        }),
        composer_body: "Queue the next turn while this one finishes…".to_string(),
        composer_disclosure: "shift+enter/ctrl+j newline".to_string(),
        composer_focused: true,
    });

    assert_eq!(streaming.variant, ControlDockVariant::Live);
    assert_eq!(streaming.runtime_context.as_deref(), Some("live"));
    assert_eq!(streaming.runtime_badge, "Streaming");
    assert_eq!(streaming.runtime_kind, RuntimeStateKind::Streaming);
    assert_eq!(streaming.primary_summary, "turn 3 · response in progress");
    assert_eq!(
        streaming.summary_segment,
        Some(ControlDockSummarySegment {
            kind: ControlDockSummarySegmentKind::Tool,
            text: "tool bash running".to_string(),
            tone: ControlDockSummaryTone::Accent,
        })
    );
    assert_eq!(
        streaming.composer_body,
        "Queue the next turn while this one finishes…"
    );
    assert_eq!(streaming.composer_disclosure, "shift+enter/ctrl+j newline");
    assert!(streaming.composer_focused);
    assert!(!streaming.composer_disabled);

    let failed = control_dock_view_model(ControlDockInput::Live {
        runtime_context: Some("recovery".to_string()),
        runtime_state: control_dock_runtime_fixture(
            RuntimeStateKind::Failure,
            "turn failed · inspect transcript, then retry or continue",
            true,
            "After review, adjust the draft, then retry or continue.",
        ),
        primary_summary: "run failed · inspect transcript · session shell preserved".to_string(),
        summary_segment: Some(ControlDockSummarySegment {
            kind: ControlDockSummarySegmentKind::Orchestration,
            text: "orch 0a 1q 0r 0s".to_string(),
            tone: ControlDockSummaryTone::Secondary,
        }),
        composer_body: "After review, adjust the draft, then retry or continue.".to_string(),
        composer_disclosure: "ctrl+p commands".to_string(),
        composer_focused: false,
    });

    assert_eq!(failed.variant, ControlDockVariant::Live);
    assert_eq!(failed.runtime_context.as_deref(), Some("recovery"));
    assert_eq!(failed.runtime_badge, "Failure");
    assert_eq!(failed.runtime_kind, RuntimeStateKind::Failure);
    assert_eq!(
        failed.primary_summary,
        "run failed · inspect transcript · session shell preserved"
    );
    assert_eq!(
        failed.summary_segment,
        Some(ControlDockSummarySegment {
            kind: ControlDockSummarySegmentKind::Orchestration,
            text: "orch 0a 1q 0r 0s".to_string(),
            tone: ControlDockSummaryTone::Secondary,
        })
    );
    assert_eq!(
        failed.composer_body,
        "After review, adjust the draft, then retry or continue."
    );
    assert_eq!(failed.composer_disclosure, "ctrl+p commands");
    assert!(!failed.composer_focused);
    assert!(failed.composer_disabled);
}

#[cfg(test)]
pub(crate) fn exact_test_tool_runtime_state_uses_effective_tool_identity() {
    let queued = tool_runtime_state(&runtime_tool_identity_fixture(
        ToolCallDisplayStatus::Queued,
    ))
    .expect("queued runtime state");
    assert_eq!(queued.summary, "tool queued · agent.spawn");
    assert!(!queued.summary.contains("task"));

    let running = tool_runtime_state(&runtime_tool_identity_fixture(
        ToolCallDisplayStatus::Running,
    ))
    .expect("running runtime state");
    assert_eq!(running.summary, "tool running · agent.spawn");
    assert!(!running.summary.contains("task"));

    let succeeded = tool_runtime_state(&runtime_tool_identity_fixture(
        ToolCallDisplayStatus::Succeeded,
    ))
    .expect("succeeded runtime state");
    assert_eq!(
        succeeded.summary,
        "tool finished · waiting for final response · agent.spawn"
    );
    assert!(!succeeded.summary.contains("task"));

    let failed = tool_runtime_state(&runtime_tool_identity_fixture(
        ToolCallDisplayStatus::Failed,
    ))
    .expect("failed runtime state");
    assert_eq!(failed.summary, "tool failed · agent.spawn");
    assert!(!failed.summary.contains("task"));
}

#[cfg(test)]
pub(crate) fn exact_test_control_dock_view_model_preserves_replay_read_only_variant() {
    let replay = control_dock_view_model(ControlDockInput::ReplayReadOnly {
        runtime_context: Some("replay".to_string()),
        runtime_state: control_dock_runtime_fixture(
            RuntimeStateKind::Ready,
            "12 events loaded",
            false,
            "Type a prompt for the next turn…",
        ),
        primary_summary: "12 events loaded".to_string(),
        composer_body: "Replay is read-only.".to_string(),
        composer_disclosure: "? shortcuts  ·  tab focus  ·  r reload  ·  q quit".to_string(),
        composer_focused: false,
    });

    assert_eq!(replay.variant, ControlDockVariant::ReplayReadOnly);
    assert_eq!(replay.runtime_context.as_deref(), Some("replay"));
    assert_eq!(replay.runtime_badge, "Ready");
    assert_eq!(replay.runtime_kind, RuntimeStateKind::Ready);
    assert_eq!(replay.primary_summary, "12 events loaded");
    assert_eq!(replay.summary_segment, None);
    assert_eq!(replay.composer_body, "Replay is read-only.");
    assert_eq!(
        replay.composer_disclosure,
        "? shortcuts  ·  tab focus  ·  r reload  ·  q quit"
    );
    assert!(!replay.composer_focused);
    assert!(replay.composer_disabled);
    assert!(!replay.composer_disclosure.contains("send"));
    assert!(!replay.composer_disclosure.contains("newline"));
}
