use std::time::Instant;

use crossterm::event::MouseEventKind;

use crate::event::TuiEvent;
use crate::scheduling::{RuntimeDecision, RuntimePacer};
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
            Self::Immediate => presenter.request_immediate_redraw(now),
            Self::Coalesced => pacer.request_flush(),
        }
    }

    pub(crate) const fn for_turn_start(self, was_active: bool, is_active: bool) -> Self {
        if !was_active && is_active {
            Self::Immediate
        } else {
            self
        }
    }
}

pub(crate) const fn should_apply_live_update(
    decision: RuntimeDecision,
    presenter: &Presenter,
    frame_ready: bool,
) -> bool {
    matches!(decision, RuntimeDecision::LiveUpdate)
        && !(presenter.immediate_pending() && presenter.should_present(frame_ready))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::Write;
    use std::time::{Duration, Instant};

    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    use super::InputPresentation;
    use crate::event::TuiEvent;
    use crate::input::{RuntimeInputIngress, TerminalEnvelope, TerminalQueue, TerminalSequence};
    use crate::scheduling::{
        FrameNow, MotionPlan, RuntimeArbiter, RuntimeDecision, RuntimePacer, RuntimeReady,
    };
    use crate::terminal::{FrameKind, FrameOutput, FrameSubmission, Presenter};

    fn envelope(sequence: u64, event: TuiEvent) -> TerminalEnvelope {
        TerminalEnvelope::new(TerminalSequence::new(sequence), Instant::now(), event)
    }

    fn click(kind: MouseEventKind) -> TuiEvent {
        TuiEvent::Mouse(MouseEvent {
            kind,
            column: 6,
            row: 8,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn resize_burst_emits_only_latest_dimensions_after_quiet_boundary() {
        // Given: three production resize events inside one 16 ms burst.
        let mut ingress = RuntimeInputIngress::default();
        assert!(ingress
            .ingest_at(Duration::ZERO, envelope(1, TuiEvent::Resize(80, 24)))
            .is_none());
        assert!(ingress
            .ingest_at(
                Duration::from_millis(5),
                envelope(2, TuiEvent::Resize(100, 30)),
            )
            .is_none());
        assert!(ingress
            .ingest_at(
                Duration::from_millis(10),
                envelope(3, TuiEvent::Resize(120, 40)),
            )
            .is_none());

        // When: the explicit runtime clock reaches the first event's quiet boundary.
        let before = ingress.flush_due(Duration::from_millis(15));
        let due = ingress.flush_due(Duration::from_millis(16));

        // Then: no early resize escapes and only the latest dimensions become ready.
        assert!(before.is_none());
        assert!(matches!(
            due.map(|envelope| envelope.event),
            Some(TuiEvent::Resize(120, 40))
        ));
    }

    #[test]
    fn sustained_resize_storm_still_flushes_from_the_first_event_window() {
        // Given: resize events arriving every 5 ms, faster than the 16 ms debounce.
        let mut ingress = RuntimeInputIngress::default();
        for tick in 0..8u64 {
            let at = Duration::from_millis(tick * 5);
            let cols = 80 + u16::try_from(tick).expect("storm tick fits u16");
            assert!(
                ingress
                    .ingest_at(at, envelope(tick + 1, TuiEvent::Resize(cols, 24)))
                    .is_none(),
                "resize at {at:?} must never bypass the debounce window"
            );
        }

        // When: the clock passes the first event's quiet boundary mid-storm.
        let due = ingress.flush_due(Duration::from_millis(40));

        // Then: the newest dimensions flush even though the storm never paused.
        assert!(matches!(
            due.map(|envelope| envelope.event),
            Some(TuiEvent::Resize(87, 24))
        ));
    }

    #[test]
    fn due_resize_flushes_before_backlogged_input_is_drained() {
        // Given: a resize whose quiet boundary passed while input kept arriving.
        let epoch = Instant::now();
        let (queue_tx, queue_rx) = crossbeam_channel::unbounded();
        let mut queue = TerminalQueue::new(queue_rx);
        let mut ingress = RuntimeInputIngress::default();
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            KeyModifiers::NONE,
        );
        ingress.ingest_at(Duration::ZERO, envelope(1, TuiEvent::Resize(80, 24)));
        queue_tx
            .send(envelope(2, TuiEvent::Key(key)))
            .expect("queue key");

        // When: take_ready runs well past the boundary with backlog still queued.
        let now = epoch + Duration::from_millis(500);
        let first = ingress.take_ready(&mut queue, epoch, now);
        let second = ingress.take_ready(&mut queue, epoch, now);

        // Then: the due resize is served before the queued key, and the key follows.
        assert!(matches!(
            first.map(|envelope| envelope.event),
            Some(TuiEvent::Resize(80, 24))
        ));
        assert!(matches!(
            second.map(|envelope| envelope.event),
            Some(TuiEvent::Key(ready)) if ready == key
        ));
    }

    #[test]
    fn non_resize_input_bypasses_pending_resize_quiet_boundary() {
        // Given: a resize waiting for its 16 ms quiet boundary.
        let mut ingress = RuntimeInputIngress::default();
        assert!(ingress
            .ingest_at(Duration::ZERO, envelope(1, TuiEvent::Resize(80, 24)))
            .is_none());
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            KeyModifiers::NONE,
        );

        // When: ordinary input arrives one millisecond later.
        let ready = ingress.ingest_at(Duration::from_millis(1), envelope(2, TuiEvent::Key(key)));

        // Then: the key is immediate while the resize remains pending.
        assert!(matches!(
            ready.map(|envelope| envelope.event),
            Some(TuiEvent::Key(ready_key)) if ready_key == key
        ));
        assert!(ingress.flush_due(Duration::from_millis(15)).is_none());
        assert!(matches!(
            ingress
                .flush_due(Duration::from_millis(16))
                .map(|envelope| envelope.event),
            Some(TuiEvent::Resize(80, 24))
        ));
    }

    #[test]
    fn submit_key_requests_an_immediate_pre_provider_frame() {
        // arrange
        // Given: a prompt submission that synchronously enters the waiting state.
        let event = TuiEvent::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            KeyModifiers::NONE,
        ));

        // When: input presentation classifies the submit key.
        let presentation = InputPresentation::for_event(&event).for_turn_start(false, true);

        // act
        // Then: the waiting frame is not coalesced with the first provider update.
        // assert
        assert_eq!(presentation, InputPresentation::Immediate);
    }

    #[test]
    fn two_event_click_under_live_backlog_is_ready_before_next_flush_cycle() {
        // arrange
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

        // act
        // Then: the completed click is ready without waiting for a 16 ms pacer cycle.
        // assert
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

    #[test]
    fn ready_immediate_resize_presents_before_queued_live_quantum() {
        // arrange
        // Given: a resize has requested immediate presentation while live work is queued.
        let now = Instant::now();
        let mut presenter = Presenter::new();
        let mut pacer = RuntimePacer::new();
        InputPresentation::for_event(&TuiEvent::Resize(160, 55)).request(
            true,
            &mut presenter,
            &mut pacer,
            now,
        );
        let decision = RuntimeDecision::LiveUpdate;

        // When: production chooses whether to apply that live quantum before rendering.
        let apply_live = super::should_apply_live_update(decision, &presenter, true);

        // act
        // Then: the resize frame starts first instead of inheriting the live batch latency.
        // assert
        assert!(!apply_live);
    }

    #[test]
    fn ordinary_dirty_presenter_does_not_suppress_live_quantum() {
        // arrange
        // Given: ordinary non-input work dirtied a presenter while live work is selected.
        let presenter = Presenter::new();

        // When: production checks whether the selected live quantum may run.
        let apply_live =
            super::should_apply_live_update(RuntimeDecision::LiveUpdate, &presenter, true);

        // act
        // Then: only typed immediate input priority can defer live work.
        // assert
        assert!(apply_live);
    }

    #[test]
    fn blocked_immediate_presenter_does_not_busy_loop_a_live_quantum() {
        // arrange
        // Given: immediate input is dirty while the capacity-one writer is unavailable.
        let now = Instant::now();
        let mut presenter = Presenter::new();
        presenter.request_immediate_redraw(now);

        // When: production checks the selected live quantum before writer acknowledgement.
        let apply_live =
            super::should_apply_live_update(RuntimeDecision::LiveUpdate, &presenter, false);

        // act
        // Then: live work progresses until immediate presentation becomes possible.
        // assert
        assert!(apply_live);
    }
}
