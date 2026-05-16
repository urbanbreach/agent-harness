#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuationBounds {
    pub max_iterations: u32,
    pub max_wall_clock_ms: u64,
    pub max_provider_calls: u32,
    pub max_tool_calls: u32,
}

impl Default for ContinuationBounds {
    fn default() -> Self {
        Self {
            max_iterations: 8,
            max_wall_clock_ms: 30 * 60 * 1000,
            max_provider_calls: 32,
            max_tool_calls: 256,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuationState {
    pub continuation_id: String,
    pub mode: String,
    pub command: String,
    pub bounds: ContinuationBounds,
    pub started_mono_ms: u64,
    pub iteration: u32,
    pub provider_calls: u32,
    pub tool_calls: u32,
    pub stopped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuationDecision {
    ReminderQueued { iteration: u32, reminder: String },
    LimitReached { limit: &'static str, iteration: u32 },
    Stopped,
}

#[derive(Debug, Default)]
pub struct ContinuationController {
    active: Option<ContinuationState>,
}

impl ContinuationController {
    pub fn start(
        &mut self,
        continuation_id: impl Into<String>,
        mode: impl Into<String>,
        command: impl Into<String>,
        bounds: ContinuationBounds,
    ) -> &ContinuationState {
        self.start_at(continuation_id, mode, command, bounds, 0)
    }

    pub fn start_at(
        &mut self,
        continuation_id: impl Into<String>,
        mode: impl Into<String>,
        command: impl Into<String>,
        bounds: ContinuationBounds,
        started_mono_ms: u64,
    ) -> &ContinuationState {
        self.active = Some(ContinuationState {
            continuation_id: continuation_id.into(),
            mode: mode.into(),
            command: command.into(),
            bounds,
            started_mono_ms,
            iteration: 0,
            provider_calls: 0,
            tool_calls: 0,
            stopped: false,
        });
        self.active.as_ref().expect("active continuation")
    }

    pub fn active(&self) -> Option<&ContinuationState> {
        self.active.as_ref().filter(|state| !state.stopped)
    }

    pub fn restore(&mut self, state: ContinuationState) {
        self.active = (!state.stopped).then_some(state);
    }

    pub fn record_activity(&mut self, provider_calls: u32, tool_calls: u32) {
        if let Some(state) = self.active.as_mut() {
            state.provider_calls = state.provider_calls.saturating_add(provider_calls);
            state.tool_calls = state.tool_calls.saturating_add(tool_calls);
        }
    }

    pub fn record_reminder(&mut self, continuation_id: &str, iteration: u32) {
        if let Some(state) = self.active.as_mut() {
            if state.continuation_id == continuation_id {
                state.iteration = state.iteration.max(iteration);
            }
        }
    }

    pub fn stop(&mut self) -> Option<ContinuationState> {
        let mut state = self.active.take()?;
        state.stopped = true;
        Some(state)
    }

    pub fn queue_idle_reminder(
        &mut self,
        incomplete_todos: bool,
        done_marker_seen: bool,
        now_mono_ms: u64,
    ) -> Option<ContinuationDecision> {
        let state = self.active.as_mut()?;
        if state.stopped || done_marker_seen {
            state.stopped = true;
            return Some(ContinuationDecision::Stopped);
        }
        if state.iteration >= state.bounds.max_iterations {
            state.stopped = true;
            return Some(ContinuationDecision::LimitReached {
                limit: "max_iterations",
                iteration: state.iteration,
            });
        }
        if now_mono_ms.saturating_sub(state.started_mono_ms) >= state.bounds.max_wall_clock_ms {
            state.stopped = true;
            return Some(ContinuationDecision::LimitReached {
                limit: "max_wall_clock_ms",
                iteration: state.iteration,
            });
        }
        if state.provider_calls >= state.bounds.max_provider_calls {
            state.stopped = true;
            return Some(ContinuationDecision::LimitReached {
                limit: "max_provider_calls",
                iteration: state.iteration,
            });
        }
        if state.tool_calls >= state.bounds.max_tool_calls {
            state.stopped = true;
            return Some(ContinuationDecision::LimitReached {
                limit: "max_tool_calls",
                iteration: state.iteration,
            });
        }
        if !incomplete_todos {
            state.stopped = true;
            return Some(ContinuationDecision::Stopped);
        }
        state.iteration += 1;
        Some(ContinuationDecision::ReminderQueued {
            iteration: state.iteration,
            reminder: format!(
                "continue {} loop iteration {} unless all todos are complete",
                state.mode, state.iteration
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ContinuationBounds, ContinuationController, ContinuationDecision};

    #[test]
    fn continuation_controller_starts_reminds_stops_and_limits() {
        let mut controller = ContinuationController::default();
        let bounds = ContinuationBounds {
            max_iterations: 1,
            ..ContinuationBounds::default()
        };
        let started = controller.start_at("cont_1", "ralph", "/ralph-loop", bounds, 100);
        assert_eq!(started.mode, "ralph");

        assert_eq!(
            controller.queue_idle_reminder(true, false, 100),
            Some(ContinuationDecision::ReminderQueued {
                iteration: 1,
                reminder: "continue ralph loop iteration 1 unless all todos are complete"
                    .to_string(),
            })
        );
        assert_eq!(
            controller.queue_idle_reminder(true, false, 100),
            Some(ContinuationDecision::LimitReached {
                limit: "max_iterations",
                iteration: 1,
            })
        );
        assert!(controller.active().is_none());

        controller.start(
            "cont_2",
            "ultrawork",
            "/ulw-loop",
            ContinuationBounds::default(),
        );
        assert!(controller.stop().is_some());
        assert!(controller.active().is_none());
    }

    #[test]
    fn continuation_controller_enforces_wall_clock_bound() {
        let mut controller = ContinuationController::default();
        controller.start_at(
            "cont_wall",
            "ralph",
            "/ralph-loop",
            ContinuationBounds {
                max_iterations: 10,
                max_wall_clock_ms: 25,
                ..ContinuationBounds::default()
            },
            100,
        );

        assert_eq!(
            controller.queue_idle_reminder(true, false, 124),
            Some(ContinuationDecision::ReminderQueued {
                iteration: 1,
                reminder: "continue ralph loop iteration 1 unless all todos are complete"
                    .to_string(),
            })
        );
        assert_eq!(
            controller.queue_idle_reminder(true, false, 125),
            Some(ContinuationDecision::LimitReached {
                limit: "max_wall_clock_ms",
                iteration: 1,
            })
        );
        assert!(controller.active().is_none());
    }
}
