use std::time::Instant;

use anyhow::Result;
use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::{
    event::TuiEvent,
    input::{
        ScrollConfigOverrides, ScrollNormalizer, ScrollNormalizerConfig, ScrollSampleDirection,
    },
    presentation::{PresentationCauseKind as Cause, RenderReason as Reason},
    runtime_presentation::InteractionEventClass as Class,
    runtime_scheduling::SchedulingLiveReadiness,
    terminal::TerminalProfile,
    ui,
};

use super::Runtime;

#[derive(Default)]
pub(super) struct Wheel {
    delta: i16,
    column: u16,
    row: u16,
    pub(super) at: Option<Instant>,
}

pub(super) fn scroll_normalizer(profile: &TerminalProfile) -> ScrollNormalizer {
    let values = [
        "HARNESS_TUI_SCROLL_MODE",
        "HARNESS_TUI_SCROLL_LINES",
        "HARNESS_TUI_SCROLL_SPEED",
        "HARNESS_TUI_INVERT_SCROLL",
    ]
    .map(|key| std::env::var(key).ok());
    ScrollNormalizer::new(
        ScrollNormalizerConfig::for_terminal(profile.context.brand, profile.context.multiplexer)
            .with_overrides(ScrollConfigOverrides::from_values(
                values[0].as_deref(),
                values[1].as_deref(),
                values[2].as_deref(),
                values[3].as_deref(),
            )),
    )
}

impl Runtime {
    pub(super) fn input(&mut self, event: TuiEvent, live: SchedulingLiveReadiness) -> Result<bool> {
        let (class, cause, reason) = match &event {
            TuiEvent::Resize(..) => (Class::Resize, Cause::Resize, Reason::Resize),
            TuiEvent::FocusGained | TuiEvent::FocusLost => {
                (Class::Focus, Cause::Focus, Reason::Focus)
            }
            TuiEvent::Mouse(mouse)
                if matches!(
                    mouse.kind,
                    MouseEventKind::ScrollDown
                        | MouseEventKind::ScrollUp
                        | MouseEventKind::ScrollLeft
                        | MouseEventKind::ScrollRight
                ) =>
            {
                (Class::Wheel, Cause::Wheel, Reason::Wheel)
            }
            TuiEvent::Mouse(_) => (Class::Mouse, Cause::Mouse, Reason::Mouse),
            TuiEvent::Paste(_) => (Class::Paste, Cause::TerminalInput, Reason::TerminalInput),
            TuiEvent::Key(_) => (Class::Key, Cause::TerminalInput, Reason::TerminalInput),
        };
        let interaction = self
            .trace
            .as_mut()
            .map(|trace| trace.take_interaction_id(class))
            .transpose()?
            .flatten();
        let immediate = matches!(
            event,
            TuiEvent::Resize(..)
                | TuiEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(_) | MouseEventKind::Up(_),
                    ..
                })
        );
        let was_active = self.app.active_turn_in_progress();
        let area = self.frame_area()?;
        self.app.set_frame_area(area);
        let changed = match event {
            TuiEvent::Key(key) => {
                self.app.handle_key(key);
                true
            }
            TuiEvent::Paste(text) => {
                self.app.handle_paste(&text);
                true
            }
            TuiEvent::Resize(..) => true,
            TuiEvent::FocusGained => {
                self.experience.set_focus(true, self.terminal.backend_mut());
                true
            }
            TuiEvent::FocusLost => {
                self.experience
                    .set_focus(false, self.terminal.backend_mut());
                true
            }
            TuiEvent::Mouse(mouse) => self.mouse(mouse, area),
        };
        let immediate = immediate || (!was_active && self.app.active_turn_in_progress());
        let cause_id = if let Some(trace) = self.trace.as_mut() {
            Some(if changed {
                trace.record_visible_cause(cause, reason, interaction.clone())
            } else {
                trace.record_no_visible_cause(cause, interaction.clone())?
            })
        } else {
            None
        };
        if changed && !matches!(class, Class::Wheel) {
            self.request_frame(immediate || self.app.should_quit, Instant::now());
        }
        if let (Some(trace), Some(id)) = (self.scheduling.as_mut(), cause_id.as_ref()) {
            trace.record_terminal_ready(
                interaction.as_ref(),
                id,
                live,
                false,
                Some(u64::try_from(self.flush_interval.as_millis()).unwrap_or(u64::MAX)),
            );
        }
        Ok(changed && immediate)
    }

    fn mouse(&mut self, mouse: MouseEvent, area: Rect) -> bool {
        let direction = match mouse.kind {
            MouseEventKind::ScrollUp => Some(ScrollSampleDirection::Up),
            MouseEventKind::ScrollDown => Some(ScrollSampleDirection::Down),
            _ => None,
        };
        if let Some(direction) = direction {
            let sample = self.scroll.push(
                self.epoch.elapsed(),
                direction,
                mouse.column,
                mouse.row,
                area.height,
            );
            self.wheel.delta = self.wheel.delta.saturating_add(sample.lines).clamp(-8, 8);
            self.wheel.column = sample.column;
            self.wheel.row = sample.row;
            if sample.lines != 0 {
                self.wheel
                    .at
                    .get_or_insert_with(|| Instant::now() + self.flush_interval);
            }
            return sample.lines != 0;
        }
        self.app.handle_pointer_event(mouse, area)
    }

    pub(super) fn apply_wheel(&mut self) -> Result<bool> {
        let wheel = std::mem::take(&mut self.wheel);
        if wheel.delta == 0 {
            return Ok(false);
        }
        let area = self.frame_area()?;
        self.app.set_frame_area(area);
        let hovered = ui::hovered_wheel_target(&self.app, area, wheel.column, wheel.row);
        let mut changed = false;
        for _ in 0..wheel.delta.unsigned_abs() {
            changed |= self.app.handle_mouse(
                MouseEvent {
                    kind: if wheel.delta < 0 {
                        MouseEventKind::ScrollUp
                    } else {
                        MouseEventKind::ScrollDown
                    },
                    column: wheel.column,
                    row: wheel.row,
                    modifiers: KeyModifiers::NONE,
                },
                area,
                hovered,
                None,
                None,
            );
        }
        Ok(changed)
    }

    pub(super) fn frame_area(&self) -> Result<Rect> {
        let size = self.terminal.size()?;
        Ok(Rect::new(0, 0, size.width, size.height))
    }
}
