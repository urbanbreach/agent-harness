use std::time::Instant;

use crossterm::event::MouseEventKind;

use crate::event::TuiEvent;
use crate::scheduling::RuntimePacer;
use crate::terminal::Presenter;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputPresentation {
    Immediate,
    Coalesced,
}

impl InputPresentation {
    pub(crate) const fn for_event(event: &TuiEvent) -> Self {
        match event {
            TuiEvent::Resize(_, _) => Self::Immediate,
            TuiEvent::Mouse(mouse)
                if matches!(mouse.kind, MouseEventKind::Down(_) | MouseEventKind::Up(_)) =>
            {
                Self::Immediate
            }
            TuiEvent::Key(_)
            | TuiEvent::Paste(_)
            | TuiEvent::Mouse(_)
            | TuiEvent::FocusGained
            | TuiEvent::FocusLost => Self::Coalesced,
        }
    }

    pub(crate) fn request(
        self,
        changed: bool,
        presenter: &mut Presenter,
        pacer: &mut RuntimePacer,
        now: Instant,
    ) {
        if !changed {
            return;
        }
        match self {
            Self::Immediate => presenter.request_redraw(now),
            Self::Coalesced => pacer.request_flush(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::Write;
    use std::time::Instant;

    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    use super::InputPresentation;
    use crate::event::TuiEvent;
    use crate::scheduling::{
        FrameNow, MotionPlan, RuntimeArbiter, RuntimeDecision, RuntimePacer, RuntimeReady,
    };
    use crate::terminal::{FrameKind, FrameOutput, FrameSubmission, Presenter};

    fn click(kind: MouseEventKind) -> TuiEvent {
        TuiEvent::Mouse(MouseEvent {
            kind,
            column: 6,
            row: 8,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn two_event_click_under_live_backlog_is_ready_before_next_flush_cycle() {
        // Given: a clean presenter and one logical click encoded as down then up.
        let now = Instant::now();
        let mut presenter = Presenter::new();
        presenter.record_submission(FrameSubmission::Accepted(FrameKind::Differential), now);
        let mut pacer = RuntimePacer::new();
        let mut input = VecDeque::from([
            (click(MouseEventKind::Down(MouseButton::Left)), true),
            (click(MouseEventKind::Up(MouseButton::Left)), false),
        ]);
        let arbiter = RuntimeArbiter::default();

        // When: down changes disclosure state and up completes the input batch unchanged.
        let mut decisions = Vec::new();
        while let Some((event, changed)) = input.pop_front() {
            let decision = arbiter.decide(RuntimeReady {
                terminal_input: true,
                live_update: true,
                ..RuntimeReady::default()
            });
            decisions.push(decision);
            InputPresentation::for_event(&event).request(changed, &mut presenter, &mut pacer, now);
        }
        let (mut output, mut writer, receiver) = FrameOutput::bounded(1);
        output.begin_frame().expect("begin click frame");
        writer.write_all(b"disclosure glyph").expect("write glyph");
        let submission = output.finish_frame().expect("finish click frame");
        let mut sink = Vec::new();
        receiver
            .write_next(&mut sink)
            .expect("write physical frame");

        // Then: the completed click is ready without waiting for a 16 ms pacer cycle.
        assert_eq!(
            decisions,
            [
                RuntimeDecision::TerminalInput,
                RuntimeDecision::TerminalInput
            ]
        );
        assert!(presenter.should_present(true));
        assert_eq!(pacer.next_wait_ms(FrameNow::default()), None);
        assert!(!pacer.needs_poll(FrameNow::default(), MotionPlan::none()));
        assert_eq!(
            submission,
            FrameSubmission::Accepted(FrameKind::Differential)
        );
        assert!(output.is_ready_for_frame());
        assert!(!sink.is_empty());
    }

    #[test]
    fn coalesced_terminal_resize_is_ready_without_a_second_flush_deadline() {
        // Given: TerminalQueue already reduced a resize burst to its newest dimensions.
        let now = Instant::now();
        let mut presenter = Presenter::new();
        presenter.record_submission(FrameSubmission::Accepted(FrameKind::Differential), now);
        let mut pacer = RuntimePacer::new();
        let resize = TuiEvent::Resize(160, 55);

        // When: the visible resize reaches the production presentation boundary.
        InputPresentation::for_event(&resize).request(true, &mut presenter, &mut pacer, now);

        // Then: it can render immediately instead of paying another 16 ms coalescing deadline.
        assert!(presenter.should_present(true));
        assert_eq!(pacer.next_wait_ms(FrameNow::default()), None);
        assert!(!pacer.needs_poll(FrameNow::default(), MotionPlan::none()));
    }
}
