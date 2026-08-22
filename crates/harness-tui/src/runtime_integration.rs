use std::io::Write;
use std::time::Instant;

use harness_core::event::{BackgroundTaskNotificationStatus, EventEnvelopeV1, EventV1};

use crate::app::AppState;
use crate::contextual_tips::{TipContext, TipManager};
use crate::inline_image::{ImageCapability, ImagePipeline, ImageRequest};
use crate::lifecycle_choreography::LifecycleAuthority;
use crate::mermaid_worker::MermaidWorker;
use crate::perf_budgets::{
    FrameMetrics, FramePhase, QueueBounds, ResourceBudget, ResourceSnapshot, SampleWindow,
    StressSample,
};
use crate::terminal_notifications::{
    FocusState, NotificationEvent, NotificationKind, NotificationPolicy, NotificationWriter,
    ProtocolSet,
};
use crate::terminal_title::{TitleActivity, TitleState, TitleWriter};
use crate::theme_tokens::LifecycleState;
use crate::video_viewer::{FramePacing, SubprocessDescriptor, SubprocessSupervisor, VideoViewer};

pub(crate) struct RuntimeExperience {
    image_pipeline: ImagePipeline,
    mermaid_worker: MermaidWorker,
    mermaid_requests: Vec<u64>,
    video_viewer: VideoViewer,
    video_supervisor: SubprocessSupervisor,
    video_requests: Vec<usize>,
    title_state: TitleState,
    title_writer: TitleWriter,
    notification_policy: NotificationPolicy,
    notification_writer: NotificationWriter,
    notifications: Vec<NotificationEvent>,
    tips: TipManager,
    lifecycle: LifecycleAuthority,
    resource_budget: ResourceBudget,
    queue_bounds: QueueBounds,
    samples: SampleWindow,
    frame_index: u64,
    frame_started_at: Instant,
    session_name: String,
}

impl RuntimeExperience {
    pub fn new() -> Self {
        Self {
            image_pipeline: ImagePipeline::new(ImageCapability::negotiate_from_env()),
            mermaid_worker: MermaidWorker::new(2, 32),
            mermaid_requests: Vec::new(),
            video_viewer: VideoViewer::new(FramePacing::for_width(80)),
            video_supervisor: SubprocessSupervisor::new(),
            video_requests: Vec::new(),
            title_state: TitleState::new(),
            title_writer: TitleWriter::new(),
            notification_policy: NotificationPolicy::default(),
            notification_writer: NotificationWriter::new(ProtocolSet::negotiate_from_env()),
            notifications: Vec::new(),
            tips: TipManager::new(),
            lifecycle: LifecycleAuthority::new(),
            resource_budget: ResourceBudget::defaults(),
            queue_bounds: QueueBounds::defaults(),
            samples: SampleWindow::new(300),
            frame_index: 0,
            frame_started_at: Instant::now(),
            session_name: "session".to_string(),
        }
    }

    pub fn on_event(&mut self, event: &EventEnvelopeV1) {
        match &event.payload {
            EventV1::RunStarted(data) => {
                self.session_name = data.run_name.to_string();
                self.title_state.set_activity(TitleActivity::Idle);
                self.transition(LifecycleState::Idle);
            }
            EventV1::SessionTitleUpdated(data) => self.session_name = data.title.clone(),
            EventV1::UserMessageSubmitted(data) => {
                self.title_state.set_activity(TitleActivity::Streaming);
                self.transition(LifecycleState::Streaming);
                self.submit_media(&data.text, event.seq);
            }
            EventV1::ProviderStreamDelta(data) => {
                self.title_state.set_activity(TitleActivity::Streaming);
                self.submit_media(&data.delta, event.seq);
            }
            EventV1::ToolCallRequested(data) => {
                self.title_state.set_activity(TitleActivity::ToolRunning);
                self.transition(LifecycleState::Tool);
                if data.args_summary.to_ascii_lowercase().contains("video") {
                    self.submit_video(data.args_summary.as_bytes());
                }
            }
            EventV1::PermissionRequested(_) => {
                self.title_state
                    .set_activity(TitleActivity::AwaitingPermission);
                self.transition(LifecycleState::Permission);
                self.notifications.push(NotificationEvent {
                    kind: NotificationKind::ActionRequired,
                    title: "Harness permission".to_string(),
                    body: "Permission requires attention".to_string(),
                    created_at_tick: event.seq,
                });
            }
            EventV1::BackgroundTaskNotification(data) => {
                let kind = match data.status {
                    BackgroundTaskNotificationStatus::Completed => NotificationKind::Complete,
                    BackgroundTaskNotificationStatus::Failed
                    | BackgroundTaskNotificationStatus::Cancelled
                    | BackgroundTaskNotificationStatus::TimedOut => NotificationKind::Failed,
                };
                self.notifications.push(NotificationEvent {
                    kind,
                    title: data.description.clone(),
                    body: data.summary.clone(),
                    created_at_tick: event.seq,
                });
            }
            EventV1::RunFinished(_) => {
                self.title_state.set_activity(TitleActivity::Completed);
                self.transition(LifecycleState::Completed);
            }
            EventV1::RunFailed(_) => {
                self.title_state.set_activity(TitleActivity::Failed);
                self.transition(LifecycleState::Failed);
            }
            _ => {}
        }
    }

    fn submit_media(&mut self, text: &str, tick: u64) {
        let lower = text.to_ascii_lowercase();
        if lower.contains("![") {
            let _ = self.image_pipeline.submit(
                ImageRequest {
                    source: text.as_bytes().to_vec(),
                    target_width: 80,
                    target_height: 24,
                    post_flush: true,
                },
                tick,
            );
        }
        if lower.contains("```mermaid") {
            let request_id = self
                .mermaid_worker
                .submit(text.to_string(), 0, 80, tick + 30);
            if self.mermaid_worker.start_render(request_id, tick).is_ok() {
                self.mermaid_requests.push(request_id);
            }
        }
    }

    fn submit_video(&mut self, source: &[u8]) {
        let descriptor = SubprocessDescriptor {
            binary: "ffmpeg".to_string(),
            args: vec![
                "-i".to_string(),
                String::from_utf8_lossy(source).into_owned(),
            ],
            max_duration_ms: 10_000,
            max_width: 1_920,
            max_height: 1_080,
        };
        if let Ok(index) = self.video_supervisor.submit(descriptor.clone()) {
            if self.video_viewer.open(descriptor).is_ok() {
                self.video_requests.push(index);
            }
        }
    }

    fn transition(&mut self, state: LifecycleState) {
        let _ = self.lifecycle.transition(state);
    }

    pub fn set_focus<W: Write>(&mut self, focused: bool, out: &mut W) {
        self.notification_policy.set_focus(if focused {
            FocusState::Focused
        } else {
            FocusState::Unfocused
        });
        if focused {
            self.title_writer.resume();
        } else {
            self.title_writer.suspend();
        }
        let _ = out.flush();
    }

    pub fn tick(&mut self, app: &AppState) {
        self.frame_index = self.frame_index.saturating_add(1);
        self.frame_started_at = Instant::now();
        self.mermaid_worker.tick(self.frame_index);
        self.title_state.tick();
        self.lifecycle
            .set_pending_permissions(u8::from(app.active_permission().is_some()));
        self.lifecycle
            .set_queued_prompts(u8::try_from(app.queued_prompt_count).unwrap_or(u8::MAX));
        self.lifecycle.set_recovering(false);
        self.lifecycle.tick();
        let _ = self.tips.update(&TipContext {
            is_first_run: app.startup_shell_visible(),
            composer_empty: app.composer_render_text().is_empty(),
            is_streaming: app.has_active_animations(),
            permission_pending: app.active_permission().is_some(),
            tool_running: app.has_active_animations(),
            transcript_blocks: app.selected_event_index,
            reduced_motion: false,
            viewport_compact: app.last_frame_area().is_some_and(|area| area.width < 80),
            model_selected: true,
            queue_items: app.queued_prompt_count,
        });
        let _ = self.queue_bounds.decide(
            u16::try_from(self.mermaid_worker.pending_count()).unwrap_or(u16::MAX),
            0,
        );
        let snapshot = ResourceSnapshot::new();
        if !self.resource_budget.is_within_budget(&snapshot) {
            self.notifications.push(NotificationEvent {
                kind: NotificationKind::Info,
                title: "Harness budget".to_string(),
                body: "frame queue throttled".to_string(),
                created_at_tick: self.frame_index,
            });
        }
    }

    pub fn post_flush<W: Write>(&mut self, out: &mut W) {
        self.image_pipeline.mark_flush_complete();
        let title = self.title_state.current_title(&self.session_name);
        let _ = self.title_writer.write_title(&title, out);
        for notification in self.notifications.drain(..) {
            if self.notification_policy.should_notify(&notification) {
                let _ = self.notification_writer.write(&notification, out);
            }
        }
        let elapsed =
            u64::try_from(self.frame_started_at.elapsed().as_micros()).unwrap_or(u64::MAX);
        let metrics = FrameMetrics {
            input_to_render_us: 0,
            render_to_flush_us: elapsed,
            total_frame_us: elapsed,
            phase: FramePhase::Flush,
        };
        self.samples.record(StressSample {
            tick: self.frame_index,
            frame_metrics: metrics,
            resources: ResourceSnapshot::new(),
        });
        let _ = out.flush();
    }

    pub fn cleanup<W: Write>(&mut self, out: &mut W) {
        for request_id in self.mermaid_requests.drain(..) {
            self.mermaid_worker.cancel(request_id);
        }
        self.video_viewer.cancel();
        self.video_requests.clear();
        self.video_supervisor.clear();
        self.notifications.clear();
        self.notification_policy.reset();
        let _ = self.title_writer.reset(out);
        let _ = self.notification_writer.shutdown(out);
    }
}
