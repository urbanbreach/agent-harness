use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use harness_core::event::{EventEnvelopeV1, EventV1, ToolCallStatus};
pub use harness_core::proj::load_run_metadata;

pub(crate) const DEFAULT_EVENT_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const EVENTS_FILE_NAME: &str = "events.jsonl";
pub(crate) const META_FILE_NAME: &str = "meta.json";
const EVENT_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy)]
pub(crate) enum ToolFinishTerminalEvents {
    Ignore,
    Error,
}

pub fn copy_events_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create output directory {}: {err}",
                parent.display()
            )
        })?;
    }

    fs::copy(from, to).map_err(|err| {
        format!(
            "failed to copy events file from {} to {}: {err}",
            from.display(),
            to.display()
        )
    })?;

    Ok(())
}

pub fn load_events_file(path: &Path) -> Result<Vec<EventEnvelopeV1>, String> {
    let body = fs::read_to_string(path)
        .map_err(|err| format!("failed to read events file {}: {err}", path.display()))?;
    body.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).map_err(|err| err.to_string()))
        .collect()
}

pub fn load_events_from_run_dir(run_dir: &Path) -> Result<Vec<EventEnvelopeV1>, String> {
    load_events_file(&run_dir.join(EVENTS_FILE_NAME))
}

pub async fn wait_for_permission_id(
    events_path: &Path,
    tool_call_id: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let events = load_events_file(events_path)?;
        if let Some(permission_id) = events.into_iter().find_map(|event| match event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(tool_call_id) =>
            {
                Some(data.permission_id)
            }
            _ => None,
        }) {
            return Ok(permission_id);
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for PermissionRequested for {tool_call_id}"
            ));
        }

        tokio::time::sleep(EVENT_WAIT_POLL_INTERVAL).await;
    }
}

pub(crate) async fn wait_for_tool_finished(
    events_path: &Path,
    tool_call_id: &str,
    timeout: Option<Duration>,
    terminal_events: ToolFinishTerminalEvents,
) -> Result<ToolCallStatus, String> {
    let deadline = timeout.map(|wait| Instant::now() + wait);
    loop {
        let events = load_events_file(events_path)?;

        if let Some(status) = events.iter().find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data) if data.tool_call_id == tool_call_id => {
                Some(data.status)
            }
            _ => None,
        }) {
            return Ok(status);
        }

        if matches!(terminal_events, ToolFinishTerminalEvents::Error) {
            if let Some(run_error) = events.iter().find_map(|event| match &event.payload {
                EventV1::RunFailed(data) => Some(data.error.clone()),
                _ => None,
            }) {
                return Err(format!(
                    "run failed before ToolCallFinished for {tool_call_id}: {run_error}"
                ));
            }

            if events
                .iter()
                .any(|event| matches!(&event.payload, EventV1::RunFinished(_)))
            {
                return Err(format!(
                    "run finished before ToolCallFinished for {tool_call_id}"
                ));
            }
        }

        if let Some(deadline) = deadline {
            if Instant::now() >= deadline {
                return Err(format!(
                    "timed out waiting for ToolCallFinished for {tool_call_id}"
                ));
            }
        }

        tokio::time::sleep(EVENT_WAIT_POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::event::{ActorKind, EventActor, EventV1, RunFinishedEvent};
    use tempfile::TempDir;

    #[test]
    fn test_load_events_file_success() {
        let dir = TempDir::new().unwrap();
        let events_path = dir.path().join("events.jsonl");

        let event1 = EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt1".to_string(),
            seq: 1,
            run_id: "run1".to_string(),
            mono_ms: 100,
            ts: None,
            actor: EventActor { kind: ActorKind::System, agent_id: None },
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload: EventV1::RunFinished(RunFinishedEvent { summary: "test".to_string() }),
        };

        let event2 = EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt2".to_string(),
            seq: 2,
            run_id: "run1".to_string(),
            mono_ms: 200,
            ts: None,
            actor: EventActor { kind: ActorKind::System, agent_id: None },
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload: EventV1::RunFinished(RunFinishedEvent { summary: "test".to_string() }),
        };

        let content = format!(
            "{}\n{}\n",
            serde_json::to_string(&event1).unwrap(),
            serde_json::to_string(&event2).unwrap()
        );

        fs::write(&events_path, content).unwrap();

        let events = load_events_file(&events_path).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_id, "evt1");
        assert_eq!(events[1].event_id, "evt2");
    }

    #[test]
    fn test_load_events_file_not_found() {
        let dir = TempDir::new().unwrap();
        let events_path = dir.path().join("non_existent.jsonl");

        let result = load_events_file(&events_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("failed to read events file"));
    }

    #[test]
    fn test_load_events_file_invalid_json() {
        let dir = TempDir::new().unwrap();
        let events_path = dir.path().join("events.jsonl");

        fs::write(&events_path, "invalid json\n").unwrap();

        let result = load_events_file(&events_path);
        assert!(result.is_err());
    }
}
