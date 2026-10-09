//! Event-driven terminal runtime. State changes precede painting; I/O stays here.
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::cursor::Show;
use ratatui::Terminal;

use crate::{
    app::AppState,
    input::{
        RuntimeInputIngress, ScrollNormalizer, TerminalIngress, TerminalIngressReader,
        TerminalReaderStatus,
    },
    presentation::{PresentationCauseKind as Cause, PresentationClock, RenderReason as Reason},
    runtime_integration::RuntimeExperience,
    runtime_live_updates::{apply_live_update_quantum, live_update_channel, LiveUpdateReceiver},
    runtime_presentation::PresentationTelemetrySession,
    runtime_scheduling::{
        SchedulingLiveReadiness, SchedulingReadinessSignal, SchedulingTelemetrySession,
    },
    runtime_wait_set::{FrameRuntimeEvent, RuntimeWaitSet, RuntimeWake},
    terminal::{FrameOutput, FrameOutputBackend, TerminalProfile},
};

mod contracts;
mod input;
mod paint;
mod setup;

pub use crate::terminal::session::close_preserved_terminal_session;
pub use contracts::{
    set_pending_replay_launch_metadata, LiveUpdate, OperatorNoticeLevel, TuiMode, TuiOptions,
};
#[cfg(test)]
pub(crate) use setup::apply_startup_capability_notice;

const DIRECTORY_REFRESH: Duration = Duration::from_secs(5);
const INPUT_QUANTUM: Duration = Duration::from_millis(2);

struct Runtime {
    app: AppState,
    terminal: Terminal<FrameOutputBackend>,
    output: FrameOutput,
    experience: RuntimeExperience,
    trace: Option<PresentationTelemetrySession>,
    scheduling: Option<SchedulingTelemetrySession>,
    readiness: Option<SchedulingReadinessSignal>,
    scroll: ScrollNormalizer,
    wheel: input::Wheel,
    epoch: Instant,
    suggestion_at: Instant,
    directory_at: Instant,
    frame_at: Option<Instant>,
    motion_at: Option<Instant>,
    flush_interval: Duration,
    reduced_motion: bool,
    urgent: bool,
    live_turn: bool,
}

pub fn run_tui_with_options(mut options: TuiOptions) -> Result<()> {
    let bindings = options
        .keybindings
        .take()
        .filter(|bindings| !bindings.is_empty());
    let (mut app, live) = setup::app_for_mode(
        options.mode,
        options.exit_on_finish,
        options.on_ui_intent,
        bindings.as_ref(),
    );
    if let Some(data_dir) = options.storage_data_dir {
        app.set_storage_data_dir(data_dir);
    }
    if let Some(toggles) = options.toggles {
        app.set_toggles_config(toggles);
    }
    app.configure_rewind();
    app.initialize_provider_connection();
    let profile = TerminalProfile::negotiate();
    let mut session = crate::terminal::session::Session::enter(options.skip_alternate_screen)
        .context("failed to set up terminal")?;
    let reduced_motion = setup::configure(&mut app, &profile);
    let trace = PresentationTelemetrySession::from_env()
        .context("failed to initialize local presentation telemetry")?;
    let scheduling = SchedulingTelemetrySession::from_env()
        .context("failed to initialize local scheduling telemetry")?;
    let readiness = SchedulingReadinessSignal::from_env()
        .context("failed to initialize local scheduling readiness signal")?;
    let clock = trace
        .as_ref()
        .map_or_else(PresentationClock::new, PresentationTelemetrySession::clock);
    let (mut output, writer, receiver) = FrameOutput::bounded_with_clock(1, clock);
    output.require_full_repaint();
    let terminal = Terminal::new(FrameOutputBackend::new(writer))?;
    let writer = receiver.spawn(std::io::stdout())?;
    let (reader, ingress) = TerminalIngressReader::spawn(128);
    let epoch = Instant::now();
    let mut runtime = Runtime {
        app,
        terminal,
        output,
        trace,
        scheduling,
        readiness,
        reduced_motion,
        urgent: false,
        live_turn: false,
        experience: RuntimeExperience::new(),
        scroll: input::scroll_normalizer(&profile),
        wheel: input::Wheel::default(),
        epoch,
        suggestion_at: epoch,
        directory_at: epoch + DIRECTORY_REFRESH,
        frame_at: Some(epoch),
        motion_at: None,
        flush_interval: Duration::from_millis(crate::scheduling::runtime_flush_interval_ms()),
    };
    runtime.cause(Cause::Startup, Reason::Startup);
    let mut result = runtime.run(ingress, live);

    // Always stop producers and drain the writer before restoring stdout modes.
    if reader.stop_and_join().is_err() {
        result = result.and(Err(anyhow::anyhow!("terminal ingress reader panicked")));
    }
    runtime.terminal.backend_mut().prepare_for_terminal_drop();
    drop(runtime.terminal);
    while runtime.output.has_in_flight_frame() {
        match runtime.output.acknowledgement_receiver().recv() {
            Ok(ack) => runtime.output.accept_acknowledgement(ack),
            Err(_) => {
                result = result.and(Err(anyhow::anyhow!(
                    "terminal frame writer acknowledgement disconnected"
                )));
                break;
            }
        }
    }
    if let Some(error) = runtime.output.take_fatal_failure() {
        result = result.and(Err(error.into()));
    }
    if let Some(mut trace) = runtime.trace {
        trace.record_acknowledgements(runtime.output.take_acknowledgements());
        if let Some(demand) = trace.take_render_demand() {
            result = result.and(
                trace
                    .record_no_visible_change(&demand)
                    .context("failed to close unpresented shutdown demand"),
            );
        }
        result = result.and(
            trace
                .finish()
                .context("failed to persist local presentation telemetry"),
        );
    }
    drop(runtime.output);
    result = result.and(
        writer
            .join()
            .map(|_| ())
            .context("terminal frame writer failed"),
    );
    if let Some(trace) = runtime.scheduling {
        result = result.and(
            trace
                .finish()
                .context("failed to persist local scheduling telemetry"),
        );
    }
    let mut stdout = std::io::stdout();
    result = result.and(
        crossterm::execute!(stdout, Show).context("failed to restore terminal cursor after TUI"),
    );
    runtime.experience.cleanup(&mut stdout);
    if result.is_ok() && options.preserve_terminal_on_exit {
        session.preserve();
    } else {
        result = result.and(
            session
                .finish(&mut stdout)
                .context("failed to restore terminal"),
        );
    }
    result
}

impl Runtime {
    fn run(
        &mut self,
        mut ingress: TerminalIngress,
        mut live: Option<LiveUpdateReceiver>,
    ) -> Result<()> {
        let mut input = RuntimeInputIngress::default();
        let mut selected = None;
        let stopped_input = crossbeam_channel::never();
        loop {
            let acknowledgements = self.output.take_acknowledgements();
            let writer_ready = !self.output.has_in_flight_frame();
            if let Some(error) = self.output.take_fatal_failure() {
                return Err(error.into());
            }
            if let Some(trace) = self.trace.as_mut() {
                trace.record_acknowledgements(acknowledgements);
            }
            match ingress.status.try_recv() {
                Ok(TerminalReaderStatus::Failed(error)) => return Err(error.into()),
                Ok(TerminalReaderStatus::Stopped) => {
                    anyhow::bail!("terminal ingress reader stopped")
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    anyhow::bail!("terminal ingress reader disconnected")
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
            }
            let readiness = self.live_readiness(live.as_ref());
            if let Some(signal) = self.readiness.as_mut() {
                signal
                    .publish_if_changed(readiness)
                    .context("failed to publish local scheduling readiness")?;
            }
            // A bounded input batch gets priority; live events and due frames get a turn
            // after it. Submit/resize/click form a paint barrier before provider work.
            let live_turn = std::mem::take(&mut self.live_turn)
                && live.as_ref().is_some_and(|receiver| !receiver.is_empty());
            let started = Instant::now();
            let mut consumed = 0;
            while !self.app.should_quit
                && !live_turn
                && consumed < 16
                && (consumed == 0 || started.elapsed() < INPUT_QUANTUM)
            {
                let ready = selected
                    .take()
                    .or_else(|| input.take_ready(&mut ingress.queue, self.epoch, Instant::now()));
                let Some(envelope) = ready else {
                    break;
                };
                consumed += 1;
                let immediate = self.input(envelope.event, self.live_readiness(live.as_ref()))?;
                if immediate {
                    break;
                }
            }
            if let Some(receiver) = live
                .as_ref()
                .filter(|_| !self.urgent && !self.app.should_quit)
            {
                let batch =
                    apply_live_update_quantum(&mut self.app, receiver, &mut self.experience);
                if batch.changed {
                    self.cause(Cause::LiveUpdate, Reason::LiveUpdate);
                    self.request_frame(self.app.should_quit, Instant::now());
                }
                if batch.disconnected {
                    live = None;
                }
            }
            if !self.app.should_quit {
                self.timers(Instant::now());
            }
            let frame_due = self.frame_at.is_some_and(|at| at <= Instant::now());
            let wheel_due = self.wheel.at.is_some_and(|at| at <= Instant::now());
            if (frame_due || wheel_due) && writer_ready {
                if self.apply_wheel()? || frame_due {
                    self.paint()?;
                } else if self.frame_at.is_none() {
                    self.finish_unchanged_demand()?;
                }
            }
            // A reserved live turn yields back to input even when animation painted.
            if live_turn {
                self.live_turn = false;
            }
            if self.app.should_quit && self.frame_at.is_none() {
                return Ok(());
            }
            if consumed != 0
                || live_turn
                || (selected.is_some() && !self.app.should_quit)
                || (!self.urgent
                    && !self.app.should_quit
                    && live.as_ref().is_some_and(|receiver| !receiver.is_empty()))
            {
                continue;
            }
            let frame_deadline = [self.frame_at, self.wheel.at]
                .into_iter()
                .flatten()
                .min()
                .filter(|_| writer_ready);
            let deadline = if self.app.should_quit {
                frame_deadline
            } else {
                [
                    frame_deadline,
                    self.motion_at,
                    input.deadline().map(|at| self.epoch + at),
                    (!self.app.replay_mode).then_some(self.directory_at),
                ]
                .into_iter()
                .flatten()
                .min()
            };
            let wake = RuntimeWaitSet {
                frame: self.output.acknowledgement_receiver(),
                reader: &ingress.status,
                terminal: if self.app.should_quit {
                    &stopped_input
                } else {
                    ingress.queue.receiver()
                },
                live: live
                    .as_ref()
                    .filter(|_| !self.urgent && !self.app.should_quit)
                    .map(LiveUpdateReceiver::receiver),
            }
            .wait(deadline);
            match wake {
                RuntimeWake::Terminal(envelope) => {
                    selected = input.ingest_at(
                        envelope.received_at.saturating_duration_since(self.epoch),
                        envelope,
                    );
                }
                RuntimeWake::Live(update) => {
                    if let Some(receiver) = live.as_ref() {
                        receiver.defer_selected(update);
                    }
                }
                RuntimeWake::LiveDisconnected => {
                    if self.app.apply_runtime_event_stream_closed() {
                        self.cause(Cause::LiveUpdate, Reason::LiveUpdate);
                        self.request_frame(self.app.should_quit, Instant::now());
                    }
                    live = None;
                }
                RuntimeWake::Frame(FrameRuntimeEvent::Acknowledged(ack)) => {
                    self.output.accept_acknowledgement(ack)
                }
                RuntimeWake::Frame(FrameRuntimeEvent::Failed { ack, stage }) => {
                    self.output.accept_acknowledgement(ack);
                    return Err(crate::terminal::FrameOutputFailure::Write(stage).into());
                }
                RuntimeWake::Frame(FrameRuntimeEvent::Disconnected) => {
                    anyhow::bail!("terminal frame writer disconnected")
                }
                RuntimeWake::Reader(TerminalReaderStatus::Failed(error)) => {
                    return Err(error.into())
                }
                RuntimeWake::Reader(_)
                | RuntimeWake::ReaderDisconnected
                | RuntimeWake::TerminalDisconnected => {
                    anyhow::bail!("terminal ingress reader disconnected")
                }
                RuntimeWake::Deadline => {}
            }
        }
    }

    fn live_readiness(&self, live: Option<&LiveUpdateReceiver>) -> SchedulingLiveReadiness {
        let stream_active = self.app.active_turn_in_progress();
        live.map_or(
            SchedulingLiveReadiness {
                stream_active,
                ..Default::default()
            },
            |receiver| receiver.scheduling_readiness(stream_active),
        )
    }

    fn cause(&mut self, kind: Cause, reason: Reason) {
        if let Some(trace) = self.trace.as_mut() {
            trace.record_visible_cause(kind, reason, None);
        }
    }

    fn request_frame(&mut self, immediate: bool, now: Instant) {
        self.urgent |= immediate;
        let at = if immediate || self.reduced_motion {
            now
        } else {
            now + self.flush_interval
        };
        self.frame_at = Some(self.frame_at.map_or(at, |current| current.min(at)));
    }

    fn timers(&mut self, now: Instant) {
        let elapsed = u64::try_from(
            now.saturating_duration_since(self.suggestion_at)
                .as_millis(),
        )
        .unwrap_or(u64::MAX);
        self.suggestion_at = now;
        let mut changed = self.app.poll_local_ghost_suggestion(elapsed);
        changed |= self.app.refresh_motion_state();
        changed |= self.app.clear_expired_quit_confirmation();
        if !self.app.replay_mode && now >= self.directory_at {
            self.directory_at = now + DIRECTORY_REFRESH;
            changed |= self.app.refresh_current_directory_label();
        }
        if changed {
            self.cause(Cause::Expiry, Reason::Expiry);
            self.request_frame(false, now);
        }
        if self.motion_at.is_some_and(|at| at <= now) {
            self.motion_at = None;
            self.app.sample_motion_clock();
            self.cause(Cause::AnimationTimer, Reason::Animation);
            self.request_frame(true, now);
        }
        let motion = self.app.motion_plan();
        self.motion_at = motion
            .until()
            .or_else(|| motion.cadence().interval())
            .map(|delay| {
                let millis =
                    u64::try_from(delay.as_nanos().div_ceil(1_000_000)).unwrap_or(u64::MAX);
                let at = now + Duration::from_millis(millis);
                self.motion_at.map_or(at, |previous| previous.min(at))
            });
    }
}

pub fn run_tui() -> Result<()> {
    let (_sender, receiver) = live_update_channel();
    run_tui_with_options(TuiOptions {
        storage_data_dir: None,
        mode: TuiMode::Live {
            run_dir: PathBuf::from("."),
            historical_events: Vec::new(),
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx: receiver,
            compact_session_supported: false,
        },
        exit_on_finish: false,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
        skip_alternate_screen: false,
    })
}
