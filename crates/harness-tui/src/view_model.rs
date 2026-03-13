use harness_core::event::EventV1;

use crate::app::{
    ActivityEntry, ActivityStatus, Focus, LifecycleShellState, RuntimeState, RuntimeStateKind,
    ToolCallDisplayStatus,
};
use crate::Action;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlDockVariant {
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

            if let Some(permission) = input.active_permission.as_ref() {
                return permission_runtime_state(permission);
            }

            if let Some(EventV1::TaskCancelled(cancelled)) = input.last_event {
                let detail =
                    (!cancelled.reason.trim().is_empty()).then(|| cancelled.reason.clone());
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

            if let Some(activity) = input.latest_activity {
                let summary = activity_status_summary(activity, input.activity_count);
                if let Some(state) = tool_runtime_state(activity) {
                    return state;
                }

                return match activity.status {
                    ActivityStatus::Streaming if activity.transcript_text.is_empty() => {
                        RuntimeState {
                            kind: RuntimeStateKind::Sending,
                            summary: format!("{summary} · waiting for first tokens"),
                            detail: None,
                            composer_disabled: false,
                            composer_hint: "Draft the next prompt while the current turn starts…"
                                .to_string(),
                        }
                    }
                    ActivityStatus::Streaming => RuntimeState {
                        kind: RuntimeStateKind::Streaming,
                        summary: format!("{summary} · receiving output"),
                        detail: None,
                        composer_disabled: false,
                        composer_hint: "Draft the next prompt while output continues…".to_string(),
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
    let mut segments = vec![format!("Preset {profile}"), format!("{provider}/{model}")];
    if let Some(mode) = startup_mode_label(startup_mode, launch_mode_label) {
        segments.push(mode.to_string());
    }
    StartupCardViewModel {
        metadata: segments.join(" · "),
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
                action: Action::Help,
                label: "shortcuts",
            },
            FooterHint {
                action: Action::Quit,
                label: "quit",
            },
        ]
    } else if input.startup_shell_visible && input.focus == Focus::List {
        vec![
            FooterHint {
                action: Action::HistoryUp,
                label: "prev",
            },
            FooterHint {
                action: Action::HistoryDown,
                label: "next",
            },
            FooterHint {
                action: Action::SubmitPrompt,
                label: "select",
            },
            FooterHint {
                action: Action::FocusNext,
                label: "composer",
            },
            FooterHint {
                action: Action::Palette,
                label: "palette",
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
                action: Action::InsertNewline,
                label: "nl",
            },
            FooterHint {
                action: Action::Palette,
                label: "commands",
            },
            FooterHint {
                action: Action::Help,
                label: "shortcuts",
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
            action: Action::Help,
            label: "shortcuts",
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
            action: Action::Help,
            label: "shortcuts",
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
        .unwrap_or_else(|| "startup ready · type below or use Ctrl+P for saved runs".to_string());

    RuntimeState {
        kind: RuntimeStateKind::Ready,
        summary,
        detail,
        composer_disabled: false,
        composer_hint: "Type to start a new session.".to_string(),
    }
}

fn startup_mode_label(startup_mode: bool, launch_mode_label: Option<&str>) -> Option<&'static str> {
    if !startup_mode {
        return None;
    }

    let mode = launch_mode_label?.trim();
    if mode.eq_ignore_ascii_case("demo") {
        Some("Demo")
    } else if mode.eq_ignore_ascii_case("mock") {
        Some("Mock")
    } else {
        None
    }
}

fn post_run_runtime_state(last_event: Option<&EventV1>) -> RuntimeState {
    match last_event {
        Some(EventV1::RunFailed(data)) => RuntimeState {
            kind: RuntimeStateKind::Failure,
            summary: "run failed · inspect transcript · session shell preserved".to_string(),
            detail: (!data.error.trim().is_empty()).then(|| data.error.clone()),
            composer_disabled: true,
            composer_hint: POST_RUN_FAILURE_COMPOSER_HINT.to_string(),
        },
        Some(EventV1::RunFinished(data)) => RuntimeState {
            kind: RuntimeStateKind::Success,
            summary: "run finished · session shell preserved".to_string(),
            detail: (!data.summary.trim().is_empty()).then(|| data.summary.clone()),
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
                "reload failed · inspect events or diff".to_string()
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
            summary: format!("permission response pending · {}", permission.summary),
            detail: Some(permission.summary.clone()),
            composer_disabled: true,
            composer_hint: "Composer disabled — waiting for the permission decision to complete."
                .to_string(),
        }
    } else {
        RuntimeState {
            kind: RuntimeStateKind::PermissionBlocked,
            summary: format!("permission required · {}", permission.summary),
            detail: Some(permission.summary.clone()),
            composer_disabled: false,
            composer_hint:
                "Keep drafting locally while the permission request waits for a decision."
                    .to_string(),
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
            summary: format!("tool queued · {}", tool_call.tool_id),
            detail: tool_call.transcript_summary(),
            composer_disabled: false,
            composer_hint: "Draft the next prompt while the queued tool waits to start…"
                .to_string(),
        }),
        ToolCallDisplayStatus::Running => Some(RuntimeState {
            kind: RuntimeStateKind::Streaming,
            summary: format!("tool running · {}", tool_call.tool_id),
            detail: tool_call.transcript_summary(),
            composer_disabled: false,
            composer_hint: "Draft the next prompt while the tool runs…".to_string(),
        }),
        ToolCallDisplayStatus::Succeeded => Some(RuntimeState {
            kind: RuntimeStateKind::Streaming,
            summary: format!(
                "tool finished · waiting for final response · {}",
                tool_call.tool_id
            ),
            detail: tool_call.truncated_output.clone(),
            composer_disabled: false,
            composer_hint:
                "Draft the next prompt while the assistant finishes after the tool result…"
                    .to_string(),
        }),
        ToolCallDisplayStatus::Failed => Some(RuntimeState {
            kind: RuntimeStateKind::Failure,
            summary: format!("tool failed · {}", tool_call.tool_id),
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
pub(crate) fn exact_test_control_dock_view_model_handles_live_runtime_variants() {
    let streaming = control_dock_view_model(ControlDockInput::Live {
        runtime_context: Some("live".to_string()),
        runtime_state: control_dock_runtime_fixture(
            RuntimeStateKind::Streaming,
            "turn 3 · receiving output",
            false,
            "Draft the next prompt while output continues…",
        ),
        primary_summary: "turn 3 · receiving output".to_string(),
        summary_segment: Some(ControlDockSummarySegment {
            kind: ControlDockSummarySegmentKind::Tool,
            text: "tool bash running".to_string(),
            tone: ControlDockSummaryTone::Accent,
        }),
        composer_body: "Queue the next turn while this one finishes…".to_string(),
        composer_disclosure: "shift+enter newline".to_string(),
        composer_focused: true,
    });

    assert_eq!(streaming.variant, ControlDockVariant::Live);
    assert_eq!(streaming.runtime_context.as_deref(), Some("live"));
    assert_eq!(streaming.runtime_badge, "Streaming");
    assert_eq!(streaming.runtime_kind, RuntimeStateKind::Streaming);
    assert_eq!(streaming.primary_summary, "turn 3 · receiving output");
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
    assert_eq!(streaming.composer_disclosure, "shift+enter newline");
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
