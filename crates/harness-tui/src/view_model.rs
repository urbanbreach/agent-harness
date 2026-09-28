// Pure presentation adapters (AppState/SessionProjection → immutable view models).
// Must not mutate app state, append events, or emit UiIntent.

use crate::app::{Focus, RuntimeState, RuntimeStateKind};
use crate::Action;
use harness_core::proj::RunStatus;

mod runtime;
pub(crate) use runtime::{
    runtime_state, PermissionRuntimeInput, RuntimeStateInput, RuntimeStateView,
};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageChildDialogViewModel {
    pub run_id: String,
    pub title: String,
    pub label: String,
    pub parent_run_id: Option<String>,
    pub child_index: usize,
    pub child_total: usize,
    pub usage: Option<String>,
    pub first_child_shortcut: String,
    pub previous_shortcut: String,
    pub next_shortcut: String,
    pub parent_shortcut: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlDockVariant {
    Startup,
    Live,
    ReplayReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlDockSummarySegmentKind {
    Retry,
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
            Self::ContinuedRuntime => "Context",
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

pub(crate) fn post_run_handoff_notice(can_reopen: bool) -> Option<&'static str> {
    (!can_reopen).then_some("current run cannot be reopened")
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
