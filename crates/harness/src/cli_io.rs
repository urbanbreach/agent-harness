use crate::UnwrapOrAbort;
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
                if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id) =>
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
            EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == tool_call_id => {
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
    use crate::UnwrapOrAbort;
    use harness_core::event::{ActorKind, EventActor, EventV1, RunFinishedEvent, SCHEMA_VERSION};
    use tempfile::TempDir;

    fn event(event_id: &str, seq: u64) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: event_id.to_string(),
            seq,
            run_id: "run1".into(),
            mono_ms: seq * 100,
            ts: None,
            actor: EventActor::new(ActorKind::System, None),
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload: EventV1::RunFinished(RunFinishedEvent {
                summary: "test".to_string(),
            }),
        }
    }

    fn write_events(path: &Path, events: &[EventEnvelopeV1]) {
        let content = events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap_or_abort())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(path, format!("{content}\n")).unwrap_or_abort();
    }

    #[test]
    fn test_copy_events_file_success() {
        // arrange
        // act
        // assert
        let dir = TempDir::new().unwrap();
        let from = dir.path().join("from.jsonl");
        let to = dir.path().join("to.jsonl");
        fs::write(&from, "test\n").unwrap();

        copy_events_file(&from, &to).unwrap_or_abort();

        assert_eq!(fs::read_to_string(&to).unwrap(), "test\n");
    }

    #[test]
    fn test_copy_events_file_create_parents() {
        // arrange
        // act
        // assert
        let dir = TempDir::new().unwrap();
        let from = dir.path().join("from.jsonl");
        let to = dir.path().join("nested/dir/to.jsonl");
        fs::write(&from, "test\n").unwrap();

        copy_events_file(&from, &to).unwrap_or_abort();

        assert_eq!(fs::read_to_string(&to).unwrap(), "test\n");
    }

    #[test]
    fn test_copy_events_file_source_missing_does_not_create_destination() {
        // arrange
        // act
        // assert
        let dir = TempDir::new().unwrap();
        let from = dir.path().join("from.jsonl");
        let to = dir.path().join("to.jsonl");

        let err = copy_events_file(&from, &to).unwrap_err();

        assert!(err.contains("failed to copy events file"));
        assert!(!to.exists());
    }

    #[test]
    fn test_load_events_file_success() {
        // arrange
        // act
        // assert
        let dir = TempDir::new().unwrap();
        let events_path = dir.path().join(EVENTS_FILE_NAME);
        write_events(&events_path, &[event("evt1", 1), event("evt2", 2)]);

        let events = load_events_file(&events_path).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_id, "evt1");
        assert_eq!(events[1].event_id, "evt2");
    }

    #[test]
    fn test_load_events_file_not_found() {
        // arrange
        // act
        // assert
        let dir = TempDir::new().unwrap();
        let events_path = dir.path().join("non_existent.jsonl");

        let err = load_events_file(&events_path).unwrap_err();

        assert!(err.contains("failed to read events file"));
    }

    #[test]
    fn test_load_events_file_invalid_json() {
        // arrange
        // act
        // assert
        let dir = TempDir::new().unwrap();
        let events_path = dir.path().join(EVENTS_FILE_NAME);
        fs::write(&events_path, "invalid json\n").unwrap();

        let result = load_events_file(&events_path);

        assert!(result.is_err());
    }

    #[test]
    fn test_load_events_from_run_dir_success() {
        // arrange
        // act
        // assert
        let dir = TempDir::new().unwrap();
        let events_path = dir.path().join(EVENTS_FILE_NAME);
        write_events(&events_path, &[event("evt1", 1), event("evt2", 2)]);

        let events = load_events_from_run_dir(dir.path()).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_id, "evt1");
        assert_eq!(events[1].event_id, "evt2");
    }

    #[test]
    fn test_load_events_from_run_dir_not_found() {
        // arrange
        // act
        // assert
        let dir = TempDir::new().unwrap();

        let err = load_events_from_run_dir(dir.path()).unwrap_err();

        assert!(err.contains("failed to read events file"));
    }

    #[test]
    fn test_load_events_from_run_dir_invalid_json() {
        // arrange
        // act
        // assert
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(EVENTS_FILE_NAME), "invalid json\n").unwrap();

        let result = load_events_from_run_dir(dir.path());

        assert!(result.is_err());
    }
}
