use std::path::PathBuf;

use harness_core::redact::{redact_value, DefaultRedactor, Redactor};
use serde_json::{json, Value};

use super::super::write_json_output;
use super::SessionExportBundle;

pub(super) fn write_redacted_export_output(
    export: &SessionExportBundle,
    output: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
    credential_values: &[String],
) -> i32 {
    let redactor = DefaultRedactor::default();
    write_redacted_export_output_with_redactor(
        export,
        output,
        stdout,
        stderr,
        &redactor,
        &redactor,
        credential_values,
    )
}

pub(in crate::sessions) fn write_redacted_export_output_with_redactor<R: Redactor + ?Sized>(
    export: &SessionExportBundle,
    output: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
    redactor: &R,
    scanner: &DefaultRedactor,
    credential_values: &[String],
) -> i32 {
    let value = match sanitized_support_export_value(export) {
        Ok(value) => value,
        Err(err) => {
            let _ = writeln!(stderr, "failed to serialize session export: {err}");
            return 1;
        }
    };
    let mut redacted = redact_value(redactor, &value);
    let redacted_body = serde_json::to_string(&redacted).unwrap_or_default();
    let secret_finding_count = scanner.secret_finding_count(&redacted_body)
        + credential_value_secret_finding_count(&redacted, credential_values);
    if secret_finding_count > 0 {
        let _ = writeln!(
            stderr,
            "failed to export session: redaction scanner found {secret_finding_count} unredacted secret marker(s)"
        );
        return 1;
    }
    let redacted_marker_count = redacted_body.matches("[REDACTED").count();
    if let Some(support) = redacted.get_mut("support").and_then(Value::as_object_mut) {
        support.insert(
            "redaction_manifest".to_string(),
            json!({
                "status": if secret_finding_count == 0 { "clean" } else { "failed" },
                "redactor": "harness-default-redactor",
                "redacted_marker_count": redacted_marker_count,
            }),
        );
        support.insert(
            "secret_scan_status".to_string(),
            json!({
                "status": if secret_finding_count == 0 { "clean" } else { "failed" },
                "scanner": "harness-session-export-secret-scan",
                "secret_finding_count": secret_finding_count,
            }),
        );
    }

    write_json_output(&redacted, output, stdout, stderr)
}

fn sanitized_support_export_value(export: &SessionExportBundle) -> serde_json::Result<Value> {
    let mut value = serde_json::to_value(export)?;
    let removed = remove_provider_reasoning_delta_events(&mut value);
    remove_provider_reasoning_delta_replay_counts(&mut value, removed);
    Ok(value)
}

fn remove_provider_reasoning_delta_events(value: &mut Value) -> u64 {
    let Some(events) = value.get_mut("events").and_then(Value::as_array_mut) else {
        return 0;
    };
    let before = events.len();
    events.retain(|event| {
        event
            .get("payload")
            .and_then(|payload| payload.get("event_type"))
            .and_then(Value::as_str)
            != Some("provider_reasoning_delta")
    });
    u64::try_from(before.saturating_sub(events.len())).unwrap_or(u64::MAX)
}

fn remove_provider_reasoning_delta_replay_counts(value: &mut Value, removed: u64) {
    let Some(replay) = value.get_mut("replay").and_then(Value::as_object_mut) else {
        return;
    };
    let reasoning_count = replay
        .get_mut("counts_by_type")
        .and_then(Value::as_object_mut)
        .and_then(|counts| counts.remove("provider_reasoning_delta"))
        .and_then(|count| count.as_u64())
        .unwrap_or(removed);
    if let Some(total_events) = replay.get_mut("total_events") {
        if let Some(total) = total_events.as_u64() {
            *total_events = Value::from(total.saturating_sub(reasoning_count));
        }
    }
}

fn credential_value_secret_finding_count(value: &Value, values: &[String]) -> usize {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => 0,
        Value::String(text) => text_credential_finding_count(text, values),
        Value::Array(items) => items
            .iter()
            .map(|item| credential_value_secret_finding_count(item, values))
            .sum(),
        Value::Object(object) => object
            .iter()
            .map(|(key, value)| {
                text_credential_finding_count(key, values)
                    + credential_value_secret_finding_count(value, values)
            })
            .sum(),
    }
}

fn text_credential_finding_count(text: &str, values: &[String]) -> usize {
    values
        .iter()
        .filter(|value| value.len() >= 8 && !value.contains("[REDACTED") && text.contains(*value))
        .count()
}
