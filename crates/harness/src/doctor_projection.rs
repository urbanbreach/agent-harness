use std::path::Path;

use harness_core::event::EventEnvelopeV1;
use harness_core::proj::{project_team_state, TeamRunStatus};
use serde_json::{json, Value};

pub(super) fn active_team_projection_summary(session_dir: &Path) -> Value {
    if !session_dir.exists() {
        return json!({
            "available": false,
            "active_team_count": 0,
            "sessions_scanned": 0,
            "parse_error_count": 0,
            "reason": "session_dir_missing",
            "source": "event_replay",
            "no_network_probes": true,
        });
    }

    let mut active_team_count = 0usize;
    let mut sessions_scanned = 0usize;
    let mut parse_error_count = 0usize;
    if let Ok(entries) = std::fs::read_dir(session_dir) {
        for entry in entries.filter_map(Result::ok) {
            let events_path = entry.path().join("events.jsonl");
            if !events_path.is_file() {
                continue;
            }
            let Ok(body) = std::fs::read_to_string(&events_path) else {
                parse_error_count = parse_error_count.saturating_add(1);
                continue;
            };
            let mut events = Vec::new();
            for line in body.lines().filter(|line| !line.trim().is_empty()) {
                match serde_json::from_str::<EventEnvelopeV1>(line) {
                    Ok(event) => events.push(event),
                    Err(_) => parse_error_count = parse_error_count.saturating_add(1),
                }
            }
            sessions_scanned = sessions_scanned.saturating_add(1);
            if let Ok(projection) = project_team_state(events.iter()) {
                active_team_count = active_team_count.saturating_add(
                    projection
                        .teams
                        .values()
                        .filter(|team| team.status == TeamRunStatus::Active)
                        .count(),
                );
            }
        }
    }

    json!({
        "available": true,
        "active_team_count": active_team_count,
        "sessions_scanned": sessions_scanned,
        "parse_error_count": parse_error_count,
        "source": "event_replay",
        "no_network_probes": true,
    })
}
