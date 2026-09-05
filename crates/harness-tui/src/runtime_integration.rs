use std::io::Write;

use harness_core::event::{BackgroundTaskNotificationStatus, EventEnvelopeV1, EventV1};
use harness_core::session::{canonical_provider_fragment_for_event, CanonicalProviderFragmentKind};

use crate::terminal_notifications::{
    FocusState, NotificationEvent, NotificationKind, NotificationPolicy, NotificationWriter,
    ProtocolSet,
};
use crate::terminal_title::{TitleActivity, TitleState, TitleWriter};

pub(crate) struct RuntimeExperience {
    title_state: TitleState,
    title_writer: TitleWriter,
    notification_policy: NotificationPolicy,
    notification_writer: NotificationWriter,
    notifications: Vec<NotificationEvent>,
    session_name: String,
}

impl RuntimeExperience {
    pub fn new() -> Self {
        Self {
            title_state: TitleState::new(),
            title_writer: TitleWriter::new(),
            notification_policy: NotificationPolicy::default(),
            notification_writer: NotificationWriter::new(ProtocolSet::negotiate_from_env()),
            notifications: Vec::new(),
            session_name: "session".to_string(),
        }
    }

    pub fn on_event(&mut self, event: &EventEnvelopeV1) {
        if let Some(fragment) = canonical_provider_fragment_for_event(event) {
            if fragment.kind == CanonicalProviderFragmentKind::Text {
                self.title_state.set_activity(TitleActivity::Streaming);
            }
            return;
        }
        match &event.payload {
            EventV1::RunStarted(data) => {
                self.session_name = data.run_name.to_string();
                self.title_state.set_activity(TitleActivity::Idle);
            }
            EventV1::SessionTitleUpdated(data) => self.session_name = data.title.clone(),
            EventV1::UserMessageSubmitted(_) => {
                self.title_state.set_activity(TitleActivity::Streaming);
            }
            EventV1::ToolCallRequested(_) => {
                self.title_state.set_activity(TitleActivity::ToolRunning);
            }
            EventV1::PermissionRequested(_) => {
                self.title_state
                    .set_activity(TitleActivity::AwaitingPermission);
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
            }
            EventV1::RunFailed(_) => {
                self.title_state.set_activity(TitleActivity::Failed);
            }
            _ => {}
        }
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

    pub fn tick(&mut self) {
        self.title_state.tick();
    }

    pub fn post_flush<W: Write>(&mut self, out: &mut W) {
        let title = self.title_state.current_title(&self.session_name);
        let _ = self.title_writer.write_title(&title, out);
        for notification in self.notifications.drain(..) {
            if self.notification_policy.should_notify(&notification) {
                let _ = self.notification_writer.write(&notification, out);
            }
        }
        let _ = out.flush();
    }

    pub fn cleanup<W: Write>(&mut self, out: &mut W) {
        self.notifications.clear();
        self.notification_policy.reset();
        let _ = self.title_writer.reset(out);
        let _ = self.notification_writer.shutdown(out);
    }
}
