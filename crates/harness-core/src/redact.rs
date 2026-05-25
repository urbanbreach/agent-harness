use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

static API_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"sk-[A-Za-z0-9]{10,}").expect("valid api key regex"));
static BEARER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Bearer\s+[A-Za-z0-9._\-]+").expect("valid bearer regex"));

pub trait Redactor {
    fn redact_text(&self, s: &str) -> String;
}

#[derive(Debug, Default)]
pub struct DefaultRedactor;

impl Redactor for DefaultRedactor {
    fn redact_text(&self, s: &str) -> String {
        let without_keys = API_KEY_RE.replace_all(s, "[REDACTED_API_KEY]");
        BEARER_RE
            .replace_all(without_keys.as_ref(), "Bearer [REDACTED]")
            .into_owned()
    }
}

pub fn redact_value<R: Redactor + ?Sized>(redactor: &R, value: &Value) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(s) => Value::String(redactor.redact_text(s)),
        Value::Array(items) => {
            Value::Array(items.iter().map(|v| redact_value(redactor, v)).collect())
        }
        Value::Object(obj) => Value::Object(redact_map(redactor, obj)),
    }
}

pub fn redact_map<R: Redactor + ?Sized>(
    redactor: &R,
    map: &Map<String, Value>,
) -> Map<String, Value> {
    map.iter()
        .map(|(k, v)| (k.clone(), redact_value(redactor, v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{redact_map, redact_value, DefaultRedactor, Redactor};
    use serde_json::json;
    use std::fs;
    use std::path::Path;

    #[test]
    fn redacts_known_secret_patterns() {
        let redactor = DefaultRedactor::default();
        let input = "key=sk-AbCdEf0123456789 and Authorization: Bearer abc.def-ghi_123";

        let redacted = redactor.redact_text(input);

        assert!(!redacted.contains("sk-AbCdEf0123456789"));
        assert!(!redacted.contains("Bearer abc.def-ghi_123"));
        assert!(redacted.contains("[REDACTED_API_KEY]"));
        assert!(redacted.contains("Bearer [REDACTED]"));
    }

    #[test]
    fn leaves_non_secret_text_unchanged() {
        let redactor = DefaultRedactor::default();
        let input = "normal output with status=ok and no credentials";

        assert_eq!(redactor.redact_text(input), input);
    }

    #[test]
    fn redacts_structured_values_for_event_payloads() {
        let redactor = DefaultRedactor::default();
        let value = json!({
            "message": "token sk-ABCDE12345ABCDE and Bearer token.abc",
            "nested": {
                "arr": [
                    "sk-ABCDE12345ABCDE",
                    "Bearer another.token"
                ]
            }
        });

        let redacted = redact_value(&redactor, &value);
        let as_text = redacted.to_string();
        assert!(!as_text.contains("sk-ABCDE12345ABCDE"));
        assert!(!as_text.contains("Bearer another.token"));
        assert!(as_text.contains("[REDACTED_API_KEY]"));
        assert!(as_text.contains("Bearer [REDACTED]"));

        let map = value.as_object().expect("object input");
        let redacted_map = redact_map(&redactor, map);
        let as_map_text = serde_json::Value::Object(redacted_map).to_string();
        assert!(!as_map_text.contains("sk-ABCDE12345ABCDE"));
    }

    struct SecretScan;

    impl SecretScan {
        fn should_scan_file(path: &Path) -> bool {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                return false;
            };

            if path
                .components()
                .filter_map(|component| component.as_os_str().to_str())
                .any(|component| component == "snapshots")
            {
                return true;
            }

            if name.contains("snapshot") {
                return true;
            }

            matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("json" | "jsonl" | "txt" | "snap")
            )
        }

        fn assert_no_sk_in_file(path: &Path) {
            if !path.is_file() {
                return;
            }

            if !Self::should_scan_file(path) {
                return;
            }

            let text = fs::read_to_string(path).unwrap_or_default();
            assert!(
                !text.contains("sk-"),
                "secret-like token found in persisted file {}",
                path.display()
            );
        }

        fn assert_no_sk_in_dir(path: &Path) {
            if !path.exists() {
                return;
            }

            let entries = fs::read_dir(path).expect("read dir");
            for entry in entries {
                let entry = entry.expect("dir entry");
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    Self::assert_no_sk_in_dir(&entry_path);
                } else {
                    Self::assert_no_sk_in_file(&entry_path);
                }
            }
        }
    }

    #[test]
    fn secret_scan_helper_fails_when_jsonl_contains_sk_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("events.jsonl");
        fs::write(&file, "{\"line\":\"sk-should-not-be-here\"}").expect("write fixture");

        let panic = std::panic::catch_unwind(|| SecretScan::assert_no_sk_in_dir(dir.path()));
        assert!(panic.is_err(), "secret scan should fail for sk- leakage");
    }

    #[test]
    fn secret_scan_helper_allows_redacted_jsonl() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("events.jsonl");
        fs::write(&file, "{\"line\":\"[REDACTED_API_KEY]\"}").expect("write fixture");

        SecretScan::assert_no_sk_in_dir(dir.path());
    }

    #[test]
    fn secret_scan_helper_fails_when_artifact_contains_sk_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir
            .path()
            .join("artifacts")
            .join("toolcalls")
            .join("call_1")
            .join("result.redacted.json");
        fs::create_dir_all(file.parent().expect("artifact parent")).expect("create artifact dir");
        fs::write(&file, "{\"display_text\":\"sk-should-not-be-here\"}").expect("write fixture");

        let panic = std::panic::catch_unwind(|| SecretScan::assert_no_sk_in_dir(dir.path()));
        assert!(
            panic.is_err(),
            "secret scan should fail for sk- leakage in artifacts"
        );
    }

    #[test]
    fn secret_scan_helper_allows_redacted_artifact_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let result = dir
            .path()
            .join("artifacts")
            .join("toolcalls")
            .join("call_1")
            .join("result.redacted.json");
        let display = dir
            .path()
            .join("artifacts")
            .join("toolcalls")
            .join("call_1")
            .join("display.redacted.txt");
        fs::create_dir_all(result.parent().expect("artifact parent")).expect("create artifact dir");

        fs::write(&result, "{\"display_text\":\"[REDACTED_API_KEY]\"}")
            .expect("write redacted json");
        fs::write(&display, "token=[REDACTED_API_KEY]").expect("write redacted text");

        SecretScan::assert_no_sk_in_dir(dir.path());
    }

    #[test]
    fn secret_scan_helper_fails_when_snap_file_contains_sk_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir
            .path()
            .join("crates")
            .join("harness-tui")
            .join("tests")
            .join("snapshots")
            .join("pty_after_tool_call.snap");
        fs::create_dir_all(file.parent().expect("snapshot parent")).expect("create snapshot dir");
        fs::write(&file, "tool output: sk-should-not-be-here").expect("write fixture");

        let panic = std::panic::catch_unwind(|| SecretScan::assert_no_sk_in_dir(dir.path()));
        assert!(
            panic.is_err(),
            "secret scan should fail for sk- leakage in .snap files"
        );
    }

    #[test]
    fn secret_scan_helper_allows_redacted_snap_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir
            .path()
            .join("crates")
            .join("harness-tui")
            .join("tests")
            .join("snapshots")
            .join("pty_after_tool_call.snap");
        fs::create_dir_all(file.parent().expect("snapshot parent")).expect("create snapshot dir");
        fs::write(&file, "tool output: [REDACTED_API_KEY]").expect("write fixture");

        SecretScan::assert_no_sk_in_dir(dir.path());
    }
}
