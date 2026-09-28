#[test]
fn frozen_runtime_projection_matches_replacement() {
    use crate::view_model as candidate;
    use candidate::{RuntimeStateInput as CandidateInput, PermissionRuntimeInput as CandidatePermission};
    use harness_core::event::{RunFailedEvent, RunFinishedEvent, TaskCancelledEvent, TaskTerminalScope};
    let mut activities = Vec::new();
    for status in [ToolCallDisplayStatus::PendingPermission, ToolCallDisplayStatus::Queued, ToolCallDisplayStatus::Running, ToolCallDisplayStatus::Succeeded, ToolCallDisplayStatus::Failed] {
        let mut entry = runtime_tool_identity_fixture(status);
        activities.push(entry.clone());
        entry.tool_calls[0].output_summary = Some(" \n failure request_digest=hidden ".to_string());
        entry.tool_calls[0].truncated_output = Some(" \n result ".to_string());
        entry.transcript_text = "response".to_string();
        activities.push(entry);
    }
    for status in [ActivityStatus::Queued, ActivityStatus::Streaming, ActivityStatus::Done, ActivityStatus::Error] {
        let mut entry = runtime_tool_identity_fixture(ToolCallDisplayStatus::PendingPermission);
        entry.status = status;
        entry.tool_calls.clear();
        entry.error_message = Some(" \n error detail ".to_string());
        activities.push(entry.clone());
        entry.transcript_text = "stream".to_string();
        entry.error_message = None;
        activities.push(entry);
    }
    let mut events = Vec::new();
    for reason in ["", " \n", "  stopped  ", "REQUEST_DIGEST=hidden"] {
        events.push(EventV1::RunFailed(RunFailedEvent { error: reason.to_string() }));
        events.push(EventV1::RunFinished(RunFinishedEvent { summary: reason.to_string() }));
        for task_scope in [None, Some(TaskTerminalScope::AgentTurn), Some(TaskTerminalScope::ToolCall)] {
            events.push(EventV1::TaskCancelled(TaskCancelledEvent {failure: false, task_id: "task".into(), reason: reason.to_string(), task_scope}));
        }
    }
    let banners = [None, Some("idle"), Some(""), Some("DISCONNECTED error"), Some("lagged"), Some("replaying"), Some("failed"), Some("no session path"), Some("request_digest=hidden"), Some("error request_digest=hidden")];
    let mut cases = 0;
    for lifecycle_shell_state in [LifecycleShellState::None, LifecycleShellState::Startup, LifecycleShellState::PostRun] {
      for replay_mode in [false, true] {
       for status_banner in banners {
        for last_event in std::iter::once(None).chain(events.iter().map(Some)) {
         for latest_activity in std::iter::once(None).chain(activities.iter().map(Some)) {
          for pending in [None, Some(false), Some(true)] {
           for continue_disabled_banner in [None, Some(" unavailable ")] {
            macro_rules! input {
                ($input:ident, $permission:ident) => {
                    $input { replay_mode, lifecycle_shell_state, continue_disabled_banner, status_banner,
                        event_count: if cases % 2 == 0 {0} else {17}, last_event, latest_activity,
                        activity_count: 3,
                        active_permission: pending.map(|submission_pending| $permission {summary: "  decision  ".to_string(), submission_pending}),
                    }
                }
            }
            let expected = runtime_state(input!(RuntimeStateInput, PermissionRuntimeInput));
            let actual: RuntimeState = candidate::runtime_state(input!(CandidateInput, CandidatePermission)).into();
            assert_eq!(expected, actual, "case {cases}");
            cases += 1;
           }
          }
         }
        }
       }
      }
    }
    println!("Compared {cases} complete runtime state values against frozen predecessor.");
}
