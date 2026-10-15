use super::{AppState, UiIntent};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct IdleCompactionTimer {
    pub(super) deadline: Option<Instant>,
    /// Threshold read when the timer was armed; the firing recheck compares against it, so
    /// a settings change after arming cannot move the goalposts of a timer already running.
    captured_threshold: u32,
    armed_for_idle: bool,
}

impl AppState {
    pub(super) fn cancel_idle_for_durable_event(
        &mut self,
        event: &harness_core::event::EventV1,
        historical: bool,
    ) {
        if !historical
            && matches!(
                event,
                harness_core::event::EventV1::RunStarted(_)
                    | harness_core::event::EventV1::UserMessageSubmitted(_)
                    | harness_core::event::EventV1::ProviderRequestStarted(_)
            )
        {
            self.cancel_idle_compaction();
        }
    }
    pub(super) fn rearm_idle_after_settled(&mut self, was_in_progress: bool) {
        if was_in_progress && !self.active_turn_in_progress() {
            self.rearm_idle_compaction();
        }
    }
    /// Keep motion polling from rearming until a settled turn or live settings refresh.
    pub(super) fn cancel_idle_compaction(&mut self) {
        self.idle_compaction.deadline = None;
        self.idle_compaction.armed_for_idle = true;
    }

    pub(super) fn rearm_idle_compaction(&mut self) {
        self.idle_compaction = IdleCompactionTimer::default();
        self.refresh_idle_compaction();
    }

    /// Idle eligibility is independent of the automatic maintenance preference. A threshold
    /// at or below zero disables the pass; a negative delay clamps like any other short one.
    pub(super) fn refresh_idle_compaction(&mut self) {
        let enabled = self.compaction_setting("runtime.compaction.idle_enabled") == Some("true");
        let now = self.now();
        let armed = self.idle_compaction.armed_for_idle;
        let deadline = self.idle_compaction.deadline;
        let threshold = if armed && deadline.is_some() {
            self.idle_compaction.captured_threshold
        } else {
            self.compaction_setting("runtime.compaction.idle_threshold_tokens")
                .and_then(|value| value.parse::<i64>().ok())
                .map_or(200_000, |value| {
                    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
                })
        };
        let tokens = self
            .active_context_usage()
            .and_then(|usage| usage.tokens)
            .unwrap_or(0);
        let eligible = enabled
            && !self.replay_mode
            && !self.should_quit
            && !self.active_turn_in_progress()
            && !self.maintenance_running()
            && self.composer.prompt_buffer.trim().is_empty()
            && threshold > 0
            && tokens >= threshold;
        if !eligible {
            self.cancel_idle_compaction();
            return;
        }
        if !armed {
            let seconds = self
                .compaction_setting("runtime.compaction.idle_timeout_seconds")
                .and_then(|value| value.parse::<i64>().ok())
                .map_or(300, |value| value.clamp(60, 3600));
            self.idle_compaction.deadline = Some(now + Duration::from_secs(seconds.unsigned_abs()));
            self.idle_compaction.captured_threshold = threshold;
            self.idle_compaction.armed_for_idle = true;
        } else if deadline.is_some_and(|deadline| now >= deadline) {
            self.idle_compaction.deadline = None;
            self.emit_ui_intent(UiIntent::IdleCompaction);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ActiveContextUsage;
    use harness_core::event::{
        ActorKind, EventActor, LiveEventEnvelope, LiveEventV1, RuntimeEvent,
    };
    use std::sync::{Arc, Mutex};
    fn turn_event(
        seq: u64,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("idle-turn-{seq}"),
            seq,
            run_id: "idle-test".into(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some("agent-alpha".into())),
            correlation_id: Some("idle-request".into()),
            causation_id: None,
            stream_key: Some("agent:agent-alpha".into()),
            payload,
        }
    }

    #[test]
    fn idle_timer_rechecks_independent_settings_draft_and_disposal_at_threshold(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (
            scenario,
            automatic,
            enable_idle,
            usage,
            draft,
            change,
            seconds,
            expected_delay,
            fires,
        ) in [
            (
                "automatic disabled",
                false,
                true,
                Some(100),
                "",
                "",
                1,
                60,
                true,
            ),
            (
                "automatic enabled",
                true,
                true,
                Some(100),
                "",
                "",
                60,
                60,
                true,
            ),
            (
                "idle disabled",
                true,
                false,
                Some(100),
                "",
                "",
                60,
                60,
                false,
            ),
            (
                "blank draft",
                false,
                true,
                Some(100),
                " \t\n",
                "",
                60,
                60,
                true,
            ),
            (
                "existing draft",
                false,
                true,
                Some(100),
                "draft",
                "",
                60,
                60,
                false,
            ),
            (
                "new draft",
                false,
                true,
                Some(100),
                "",
                "draft",
                60,
                60,
                false,
            ),
            (
                "disposed",
                false,
                true,
                Some(100),
                "",
                "dispose",
                60,
                60,
                false,
            ),
            (
                "live disabled",
                false,
                true,
                Some(100),
                "",
                "disable",
                60,
                60,
                false,
            ),
            (
                "usage dropped",
                false,
                true,
                Some(100),
                "",
                "usage",
                60,
                60,
                false,
            ),
            ("absent usage", false, true, None, "", "", 60, 60, false),
            (
                "below threshold",
                false,
                true,
                Some(99),
                "",
                "",
                60,
                60,
                false,
            ),
            (
                "zero threshold",
                false,
                true,
                Some(100),
                "",
                "threshold",
                60,
                60,
                false,
            ),
            (
                "maintenance starts",
                false,
                true,
                Some(100),
                "",
                "maintenance",
                60,
                60,
                false,
            ),
            (
                "maintenance ends",
                false,
                true,
                Some(100),
                "",
                "maintenance end",
                60,
                60,
                false,
            ),
            (
                "streaming refresh",
                false,
                true,
                Some(100),
                "",
                "streaming",
                60,
                60,
                false,
            ),
            (
                "maximum delay",
                false,
                true,
                Some(100),
                "",
                "",
                4000,
                3600,
                true,
            ),
        ] {
            let started = Instant::now();
            let clock = Arc::new(Mutex::new(started));
            let injected = Arc::clone(&clock);
            let intents = Arc::new(Mutex::new(Vec::new()));
            let sink = Arc::clone(&intents);
            let mut app = AppState::new_live(
                None,
                false,
                Some(Arc::new(move |intent| {
                    if let Ok(mut intents) = sink.lock() {
                        intents.push(intent);
                    }
                })),
            );
            app.set_now_fn_for_test(Arc::new(move || {
                injected.lock().map_or(started, |time| *time)
            }));
            let workspace = tempfile::tempdir()?;
            let path = workspace.path().join("harness.json");
            std::fs::write(
                &path,
                format!(
                    r#"{{"runtime":{{"compaction":{{"enabled":{automatic},"idle_threshold_tokens":100,"idle_timeout_seconds":{seconds}}}}}}}"#,
                ),
            )?;
            app.projection.active_context_usage = usage.map(ActiveContextUsage::estimate);
            app.composer.prompt_buffer = draft.into();
            app.bind_settings_project_config(&path, automatic, true, true, true, false);
            app.refresh_idle_compaction();
            assert!(app.idle_compaction.deadline.is_none(), "{scenario}");
            if enable_idle {
                harness_core::config::write_project_setting_value(
                    &path,
                    "runtime.compaction.idle_enabled",
                    "true",
                )?;
                app.bind_settings_project_config(&path, automatic, true, true, true, false);
            }
            let initially_eligible =
                enable_idle && usage.is_some_and(|tokens| tokens >= 100) && draft.trim().is_empty();
            assert_eq!(
                app.idle_compaction.deadline,
                initially_eligible.then_some(started + Duration::from_secs(expected_delay)),
                "{scenario}",
            );
            *clock.lock().map_err(|_| "clock poisoned")? = started
                + Duration::from_millis(expected_delay.saturating_mul(1000).saturating_sub(1));
            app.refresh_idle_compaction();
            assert!(
                intents.lock().map_err(|_| "intents poisoned")?.is_empty(),
                "{scenario}"
            );
            match change {
                "draft" => app.composer.prompt_buffer = "new draft".into(),
                "dispose" => app.should_quit = true,
                "disable" => app.set_idle_compaction_settings(false, 100, seconds),
                "threshold" => app.set_idle_compaction_settings(true, 0, seconds),
                "usage" => {
                    app.projection.active_context_usage = Some(ActiveContextUsage::estimate(99));
                }
                "maintenance" | "maintenance end" => {
                    app.ingest_runtime_event(RuntimeEvent::Live(Box::new(LiveEventEnvelope {
                        event_id: "idle-maintenance".into(),
                        run_id: "idle-test".into(),
                        mono_ms: 0,
                        ts: None,
                        actor: EventActor::new(ActorKind::Worker, Some("agent-alpha".into())),
                        correlation_id: None,
                        causation_id: None,
                        stream_key: Some("agent:agent-alpha".into()),
                        payload: LiveEventV1::CompactionProgress {
                            agent_id: "agent-alpha".into(),
                            generation: 1,
                            trigger_reason: "manual".into(),
                            preview: (change == "maintenance").then(String::new),
                            stage: None,
                        },
                    })));
                }
                "streaming" => {
                    app.ingest_event(turn_event(
                        1,
                        harness_core::event::EventV1::ProviderRequestStarted(
                            harness_core::event::ProviderRequestStartedEvent {
                                request_id: "idle-request".into(),
                                provider_id: "mock".into(),
                                model_id: "mock-model".into(),
                                prompt_summary: "prompt".into(),
                                request_digest: "idle-digest".into(),
                                metadata: None,
                            },
                        ),
                    ));
                    app.set_idle_compaction_settings(true, 100, seconds);
                }
                _ => {}
            }
            *clock.lock().map_err(|_| "clock poisoned")? =
                started + Duration::from_secs(expected_delay);
            app.refresh_idle_compaction();
            app.refresh_idle_compaction();
            let delivered = intents.lock().map_err(|_| "intents poisoned")?;
            assert_eq!(delivered.len(), usize::from(fires), "{scenario}");
            assert!(delivered
                .iter()
                .all(|intent| matches!(intent, UiIntent::IdleCompaction)));
            assert!(app.idle_compaction.deadline.is_none(), "{scenario}");
            drop(delivered);
            if change == "streaming" {
                app.ingest_event(turn_event(
                    2,
                    harness_core::event::EventV1::ProviderRequestFinished(
                        harness_core::event::ProviderRequestFinishedEvent {
                            request_id: "idle-request".into(),
                            finish_reason: "stop".into(),
                            output_digest: None,
                            usage: None,
                            metadata: None,
                        },
                    ),
                ));
                *clock.lock().map_err(|_| "clock poisoned")? =
                    started + Duration::from_secs(expected_delay * 2);
                app.refresh_idle_compaction();
                app.refresh_idle_compaction();
                assert_eq!(intents.lock().map_err(|_| "intents poisoned")?.len(), 1);
            }
        }
        Ok(())
    }

    #[test]
    fn negative_settings_disable_the_threshold_and_clamp_the_delay(
    ) -> Result<(), Box<dyn std::error::Error>> {
        // (threshold, timeout, armed delay in seconds)
        for (threshold, timeout, delay) in [
            (-1, 300, None),
            (100, -5, Some(60)),
            (100, 99_999, Some(3600)),
        ] {
            let started = Instant::now();
            let mut app = AppState::new_live(None, false, None);
            app.set_now_fn_for_test(Arc::new(move || started));
            let workspace = tempfile::tempdir()?;
            let path = workspace.path().join("harness.json");
            std::fs::write(
                &path,
                format!(
                    r#"{{"runtime":{{"compaction":{{"idle_enabled":true,"idle_threshold_tokens":{threshold},"idle_timeout_seconds":{timeout}}}}}}}"#
                ),
            )?;
            app.projection.active_context_usage = Some(ActiveContextUsage::estimate(100));
            app.bind_settings_project_config(&path, true, true, true, true, false);
            app.refresh_idle_compaction();
            assert_eq!(
                app.idle_compaction.deadline,
                delay.map(|seconds| started + Duration::from_secs(seconds)),
                "threshold {threshold} timeout {timeout}"
            );
        }
        Ok(())
    }
}
