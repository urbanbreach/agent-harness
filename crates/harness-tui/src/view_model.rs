use harness_core::event::EventV1;

use crate::app::{
    ActivityEntry, ActivityStatus, Focus, LifecycleShellState, RuntimeState, RuntimeStateKind, Tab,
    ToolCallDisplayStatus,
};
use crate::Action;

const CONTINUED_LIVE_RUN_PREFIX: &str = "continued live run";
const POST_RUN_COMPOSER_HINT: &str =
    "Post-run handoff active — select the next action instead of sending another prompt.";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PostRunCardViewModel {
    pub summary: String,
    pub warning: bool,
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
    pub post_run_handoff_visible: bool,
    pub active_tab: Tab,
    pub startup_shell_visible: bool,
    pub focus: Focus,
    pub details_drawer_open: bool,
    pub composer_disabled: bool,
    pub continued_live_run: bool,
}

pub(crate) fn lifecycle_shell_state(
    replay_mode: bool,
    startup_mode: bool,
    run_terminal_seen: bool,
    continued_post_run_handoff_active: bool,
    post_run_handoff_enabled: bool,
) -> LifecycleShellState {
    if replay_mode {
        return LifecycleShellState::None;
    }

    if startup_mode {
        return LifecycleShellState::Startup;
    }

    if (run_terminal_seen || continued_post_run_handoff_active) && post_run_handoff_enabled {
        return LifecycleShellState::PostRun;
    }

    LifecycleShellState::None
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
                    .map(|reason| format!("last turn cancelled · {reason}"))
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
                        summary: format!("{summary} · inspect transcript and retry when ready"),
                        detail: activity.error_message.clone(),
                        composer_disabled: false,
                        composer_hint: "Type a prompt to retry or continue from the failure…"
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

pub(crate) fn post_run_card_view_model(
    post_run_notice: Option<&'static str>,
    runtime_summary: &str,
) -> PostRunCardViewModel {
    PostRunCardViewModel {
        summary: post_run_notice
            .map(str::to_string)
            .unwrap_or_else(|| runtime_summary.to_string()),
        warning: post_run_notice.is_some(),
    }
}

pub(crate) fn footer_hints_view_model(input: FooterHintsInput) -> FooterHintsViewModel {
    let details_hint = FooterHint {
        action: Action::ToggleDetailsDrawer,
        label: if input.details_drawer_open {
            "close"
        } else {
            "details"
        },
    };

    let hints = if input.replay_mode {
        vec![
            FooterHint {
                action: Action::FocusNext,
                label: "nav",
            },
            FooterHint {
                action: Action::TabRun,
                label: "convo",
            },
            FooterHint {
                action: Action::TabEvents,
                label: "events",
            },
            FooterHint {
                action: Action::TabDiff,
                label: "diff",
            },
            FooterHint {
                action: Action::TabHelp,
                label: "help",
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
    } else if input.post_run_handoff_visible && input.active_tab == Tab::Run {
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
                action: Action::TabEvents,
                label: "events",
            },
            FooterHint {
                action: Action::TabDiff,
                label: "diff",
            },
            FooterHint {
                action: Action::TabHelp,
                label: "help",
            },
            FooterHint {
                action: Action::Quit,
                label: "quit",
            },
        ]
    } else if !input.replay_mode && matches!(input.active_tab, Tab::Events | Tab::Diff | Tab::Help)
    {
        vec![
            FooterHint {
                action: Action::TabRun,
                label: "convo",
            },
            details_hint,
            FooterHint {
                action: Action::TabEvents,
                label: "events",
            },
            FooterHint {
                action: Action::TabDiff,
                label: "diff",
            },
            FooterHint {
                action: Action::TabHelp,
                label: "help",
            },
            FooterHint {
                action: Action::FocusNext,
                label: "focus",
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
    } else if input.composer_disabled {
        vec![
            details_hint,
            FooterHint {
                action: Action::TabEvents,
                label: "events",
            },
            FooterHint {
                action: Action::TabDiff,
                label: "diff",
            },
            FooterHint {
                action: Action::TabHelp,
                label: "help",
            },
            FooterHint {
                action: Action::Quit,
                label: "quit",
            },
        ]
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
            details_hint,
            FooterHint {
                action: Action::TabEvents,
                label: "events",
            },
            FooterHint {
                action: Action::TabDiff,
                label: "diff",
            },
            FooterHint {
                action: Action::TabHelp,
                label: "help",
            },
            FooterHint {
                action: Action::Quit,
                label: "quit",
            },
        ]
    };

    FooterHintsViewModel {
        prefix: input
            .continued_live_run
            .then_some(CONTINUED_LIVE_RUN_PREFIX),
        hints,
    }
}

fn startup_runtime_state(continue_disabled_banner: Option<&str>) -> RuntimeState {
    let detail = continue_disabled_banner.map(str::to_string);
    let summary = detail
        .as_deref()
        .map(|reason| format!("startup launcher ready · {reason}"))
        .unwrap_or_else(|| {
            "startup launcher ready · choose New/Continue/Replay or type to quick-start".to_string()
        });

    RuntimeState {
        kind: RuntimeStateKind::Ready,
        summary,
        detail,
        composer_disabled: false,
        composer_hint:
            "Type to quick-start a new session while the lifecycle actions stay available."
                .to_string(),
    }
}

fn post_run_runtime_state(last_event: Option<&EventV1>) -> RuntimeState {
    match last_event {
        Some(EventV1::RunFailed(data)) => RuntimeState {
            kind: RuntimeStateKind::Failure,
            summary: "run failed · choose what to do next".to_string(),
            detail: (!data.error.trim().is_empty()).then(|| data.error.clone()),
            composer_disabled: true,
            composer_hint: POST_RUN_COMPOSER_HINT.to_string(),
        },
        Some(EventV1::RunFinished(data)) => RuntimeState {
            kind: RuntimeStateKind::Success,
            summary: "run finished · choose what to do next".to_string(),
            detail: (!data.summary.trim().is_empty()).then(|| data.summary.clone()),
            composer_disabled: true,
            composer_hint: POST_RUN_COMPOSER_HINT.to_string(),
        },
        _ => RuntimeState {
            kind: RuntimeStateKind::Ready,
            summary: "run complete · choose what to do next".to_string(),
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
            composer_hint: "Composer disabled — live stream disconnected. Reopen the TUI to reconnect, then continue from the visible transcript.".to_string(),
        });
    }

    if lower.contains("lagged") || lower.contains("replaying") {
        return Some(RuntimeState {
            kind: RuntimeStateKind::Degraded,
            summary: format!("{banner} · sending paused until recovery"),
            detail: Some(banner.to_string()),
            composer_disabled: true,
            composer_hint:
                "Composer disabled — waiting for live recovery before sending the next turn."
                    .to_string(),
        });
    }

    if lower.contains("failed") || lower.contains("error") || lower.contains("no session path") {
        return Some(RuntimeState {
            kind: RuntimeStateKind::Failure,
            summary: if replay_mode {
                "reload failed · inspect details".to_string()
            } else {
                "runtime failure · inspect transcript and retry when ready".to_string()
            },
            detail: Some(banner.to_string()),
            composer_disabled: false,
            composer_hint: "Type a prompt to retry or continue after the failure…".to_string(),
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
            composer_disabled: true,
            composer_hint:
                "Composer disabled — approve or deny the pending permission request to continue."
                    .to_string(),
        }
    }
}

fn activity_status_summary(activity: &ActivityEntry, turn_count: usize) -> String {
    let provider = if activity.provider_id.is_empty() {
        "-"
    } else {
        activity.provider_id.as_str()
    };
    let model = if activity.model_id.is_empty() {
        "-"
    } else {
        activity.model_id.as_str()
    };

    [
        format!("turn {turn_count}/{turn_count}"),
        if activity.request_id.is_empty() {
            "pending turn".to_string()
        } else {
            activity.request_id.clone()
        },
        format!("{provider}/{model}"),
    ]
    .join(" · ")
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
            composer_hint: "Inspect the tool failure, then retry or continue with a new prompt…"
                .to_string(),
        }),
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
