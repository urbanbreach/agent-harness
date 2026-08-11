use ratatui::style::{Color, Modifier, Style};

use crate::design_contract::{
    BorderRole, ColorRole, FocusRole, GlyphRole, LifecycleState, StateColorBinding, TextModifier,
    DESIGN_TOKENS,
};
use crate::theme::Theme;
use crate::transcript_identity::{ReplayTurn, TurnId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimelineStatus {
    Queued,
    Streaming,
    Completed,
    Failed,
    Compacted,
}

impl TimelineStatus {
    pub const fn is_failed(self) -> bool {
        matches!(self, Self::Failed)
    }

    pub const fn is_streaming(self) -> bool {
        matches!(self, Self::Streaming)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkerInteraction {
    Normal,
    Active,
    Hovered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineMarkerStyle {
    pub foreground: Color,
    pub background: Color,
    pub border: BorderRole,
    pub modifier: TextModifier,
    pub glyph: &'static str,
}

impl TimelineMarkerStyle {
    pub fn ratatui_style(self) -> Style {
        let style = Style::default().fg(self.foreground).bg(self.background);
        match self.modifier {
            TextModifier::Normal => style,
            TextModifier::Dim => style.add_modifier(Modifier::DIM),
            TextModifier::Bold => style.add_modifier(Modifier::BOLD),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineMarker {
    pub turn_id: TurnId,
    pub status: TimelineStatus,
    pub lifecycle_state: LifecycleState,
}

impl TimelineMarker {
    pub const fn new(
        turn_id: TurnId,
        status: TimelineStatus,
        lifecycle_state: LifecycleState,
    ) -> Self {
        Self {
            turn_id,
            status,
            lifecycle_state,
        }
    }

    pub const fn from_replay(
        turn: ReplayTurn,
        status: TimelineStatus,
        lifecycle_state: LifecycleState,
    ) -> Self {
        Self::new(turn.turn_id(), status, lifecycle_state)
    }

    pub const fn glyph_role(self) -> GlyphRole {
        match self.status {
            TimelineStatus::Queued => GlyphRole::Queued,
            TimelineStatus::Streaming => GlyphRole::Streaming,
            TimelineStatus::Completed => GlyphRole::Succeeded,
            TimelineStatus::Failed => GlyphRole::Failed,
            TimelineStatus::Compacted => GlyphRole::Streaming,
        }
    }

    pub fn glyph(self) -> &'static str {
        glyph_for(self.glyph_role())
    }

    pub fn style(self, interaction: MarkerInteraction, theme: &Theme) -> TimelineMarkerStyle {
        match interaction {
            MarkerInteraction::Normal => {
                let binding = state_binding(self.style_lifecycle_state());
                TimelineMarkerStyle {
                    foreground: color_for(theme, binding.foreground),
                    background: color_for(theme, binding.background),
                    border: BorderRole::None,
                    modifier: TextModifier::Normal,
                    glyph: glyph_for(self.glyph_role()),
                }
            }
            MarkerInteraction::Active => focus_style(theme, FocusRole::SelectedRow, self.glyph()),
            MarkerInteraction::Hovered => focus_style(theme, FocusRole::Panel, self.glyph()),
        }
    }

    const fn style_lifecycle_state(self) -> LifecycleState {
        match self.status {
            TimelineStatus::Failed => LifecycleState::Failed,
            TimelineStatus::Compacted => LifecycleState::Compacting,
            TimelineStatus::Queued | TimelineStatus::Streaming | TimelineStatus::Completed => {
                self.lifecycle_state
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineTurn {
    pub marker: TimelineMarker,
    pub row: usize,
    pub height: usize,
    pub label: String,
    replay: ReplayTurn,
}

impl TimelineTurn {
    pub fn from_replay(
        replay: ReplayTurn,
        row: usize,
        height: usize,
        status: TimelineStatus,
        lifecycle_state: LifecycleState,
    ) -> Self {
        Self {
            marker: TimelineMarker::from_replay(replay, status, lifecycle_state),
            row,
            height: height.max(1),
            label: String::new(),
            replay,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub const fn replay_turn(&self) -> ReplayTurn {
        self.replay
    }

    pub const fn turn_id(&self) -> TurnId {
        self.marker.turn_id
    }

    pub fn marker_label(&self) -> &str {
        if self.label.is_empty() {
            self.marker.glyph()
        } else {
            self.label.as_str()
        }
    }
}

fn state_binding(state: LifecycleState) -> StateColorBinding {
    DESIGN_TOKENS
        .state_colors
        .bindings
        .iter()
        .find(|binding| binding.state == state)
        .copied()
        .map_or(
            StateColorBinding {
                state,
                foreground: ColorRole::TextPrimary,
                background: ColorRole::Shell,
                glyph: GlyphRole::Done,
            },
            |binding| binding,
        )
}

fn focus_style(theme: &Theme, role: FocusRole, glyph: &'static str) -> TimelineMarkerStyle {
    let focus = DESIGN_TOKENS
        .focus_styles
        .all
        .iter()
        .find(|style| style.role == role)
        .copied();
    let Some(focus) = focus else {
        return TimelineMarkerStyle {
            foreground: color_for(theme, ColorRole::TextPrimary),
            background: color_for(theme, ColorRole::Shell),
            border: BorderRole::None,
            modifier: TextModifier::Normal,
            glyph,
        };
    };
    TimelineMarkerStyle {
        foreground: color_for(theme, focus.foreground),
        background: color_for(theme, focus.background),
        border: focus.border,
        modifier: focus.modifier,
        glyph,
    }
}

fn color_for(theme: &Theme, role: ColorRole) -> Color {
    match role {
        ColorRole::Canvas => theme.surface.canvas,
        ColorRole::Shell => theme.surface.shell,
        ColorRole::Panel => theme.surface.panel,
        ColorRole::PanelElevated => theme.surface.panel_elevated,
        ColorRole::Overlay => theme.surface.overlay,
        ColorRole::Card => theme.surface.card,
        ColorRole::SelectedCard => theme.surface.selected_card,
        ColorRole::BorderSubtle => theme.border.subtle,
        ColorRole::BorderStrong => theme.border.strong,
        ColorRole::BorderFocus => theme.border.focus,
        ColorRole::TextPrimary => theme.text.primary,
        ColorRole::TextSecondary => theme.text.secondary,
        ColorRole::TextTertiary => theme.text.tertiary,
        ColorRole::TextAccent => theme.text.accent,
        ColorRole::TextInverse => theme.text.inverse,
        ColorRole::StatusSuccess => theme.status.success,
        ColorRole::StatusWarning => theme.status.warning,
        ColorRole::StatusError => theme.status.error,
        ColorRole::StatusInfo => theme.status.info,
        ColorRole::StatusDisabled => theme.status.disabled,
        ColorRole::QuestionSurface => theme.question_prompt.surface,
        ColorRole::QuestionSelected => theme.question_prompt.selected,
        ColorRole::QuestionPrimary => theme.question_prompt.primary,
        ColorRole::QuestionAccent => theme.question_prompt.accent,
        ColorRole::QuestionSecondary => theme.question_prompt.secondary,
        ColorRole::AgentBuild
        | ColorRole::AgentPlan
        | ColorRole::AgentDocs
        | ColorRole::AgentAsk => theme.text.accent,
        ColorRole::TerminalPrimary => theme.reference_terminal.primary,
        ColorRole::TerminalSecondary => theme.reference_terminal.secondary,
        ColorRole::TerminalMuted => theme.reference_terminal.muted,
        ColorRole::TerminalError => theme.reference_terminal.error,
        ColorRole::TerminalPaletteSection => theme.reference_terminal.palette_section,
        ColorRole::TerminalForkAccent => theme.reference_terminal.fork_accent,
        ColorRole::DiffAdded => theme.reference_terminal.diff_added,
        ColorRole::DiffRemoved => theme.reference_terminal.diff_removed,
        ColorRole::DiffAddedGutter => theme.reference_terminal.diff_added_gutter,
        ColorRole::DiffRemovedGutter => theme.reference_terminal.diff_removed_gutter,
        ColorRole::DiffAddedHighlight => theme.reference_terminal.diff_added_highlight,
        ColorRole::DiffRemovedHighlight => theme.reference_terminal.diff_removed_highlight,
        ColorRole::DiffHunkHeader => theme.reference_terminal.diff_hunk_header,
    }
}

fn glyph_for(role: GlyphRole) -> &'static str {
    DESIGN_TOKENS
        .glyph_roles
        .all
        .iter()
        .find(|token| token.role == role)
        .map_or("?", |token| token.preferred)
}
