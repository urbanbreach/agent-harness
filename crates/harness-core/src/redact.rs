use regex::Regex;
use serde_json::{Map, Value};

pub trait Redactor {
    fn redact_text(&self, s: &str) -> String;
}

#[derive(Debug)]
pub struct DefaultRedactor {
    api_key_re: Regex,
    google_api_key_re: Regex,
    aws_access_key_re: Regex,
    github_pat_re: Regex,
    github_token_re: Regex,
    bearer_re: Regex,
    cookie_header_re: Regex,
    pem_private_key_re: Regex,
    url_userinfo_re: Regex,
    sensitive_query_re: Regex,
}

use std::sync::LazyLock;

static GLOBAL_REDACTOR: LazyLock<DefaultRedactor> = LazyLock::new(|| {
    DefaultRedactor {
    api_key_re: Regex::new(r"(^|[^A-Za-z0-9])(sk-[A-Za-z0-9._-]{10,})")
        .expect("valid api key regex"),
    google_api_key_re: Regex::new(r"AIza[0-9A-Za-z_-]{20,}")
        .expect("valid google api key regex"),
    aws_access_key_re: Regex::new(r"AKIA[0-9A-Z]{16}")
        .expect("valid aws access key regex"),
    github_pat_re: Regex::new(r"github_pat_[A-Za-z0-9_]{20,}")
        .expect("valid github pat regex"),
    github_token_re: Regex::new(r"ghp_[A-Za-z0-9]{20,}")
        .expect("valid github token regex"),
    bearer_re: Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=-]+")
        .expect("valid bearer regex"),
    cookie_header_re: Regex::new(r"(?i)\b(?:Set-Cookie|Cookie):\s*[^\r\n]+")
        .expect("valid cookie header regex"),
    pem_private_key_re: Regex::new(
        r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----",
    )
    .expect("valid pem private key regex"),
    url_userinfo_re: Regex::new(r"(?i)(https?://)[^/@\s]+@")
        .expect("valid url userinfo regex"),
    sensitive_query_re: Regex::new(
        r"(?i)([?&](?:api[_-]?key|access[_-]?token|refresh[_-]?token|id[_-]?token|token|password|secret)=)[^&\s]+",
    )
    .expect("valid sensitive query regex"),
}
});

impl Default for DefaultRedactor {
    fn default() -> Self {
        Self {
            api_key_re: GLOBAL_REDACTOR.api_key_re.clone(),
            google_api_key_re: GLOBAL_REDACTOR.google_api_key_re.clone(),
            aws_access_key_re: GLOBAL_REDACTOR.aws_access_key_re.clone(),
            github_pat_re: GLOBAL_REDACTOR.github_pat_re.clone(),
            github_token_re: GLOBAL_REDACTOR.github_token_re.clone(),
            bearer_re: GLOBAL_REDACTOR.bearer_re.clone(),
            cookie_header_re: GLOBAL_REDACTOR.cookie_header_re.clone(),
            pem_private_key_re: GLOBAL_REDACTOR.pem_private_key_re.clone(),
            url_userinfo_re: GLOBAL_REDACTOR.url_userinfo_re.clone(),
            sensitive_query_re: GLOBAL_REDACTOR.sensitive_query_re.clone(),
        }
    }
}

impl DefaultRedactor {
    pub fn secret_finding_count(&self, s: &str) -> usize {
        non_redacted_match_count(&self.api_key_re, s)
            + non_redacted_match_count(&self.google_api_key_re, s)
            + non_redacted_match_count(&self.aws_access_key_re, s)
            + non_redacted_match_count(&self.github_pat_re, s)
            + non_redacted_match_count(&self.github_token_re, s)
            + non_redacted_match_count(&self.bearer_re, s)
            + non_redacted_match_count(&self.cookie_header_re, s)
            + non_redacted_match_count(&self.pem_private_key_re, s)
            + non_redacted_match_count(&self.url_userinfo_re, s)
            + non_redacted_match_count(&self.sensitive_query_re, s)
    }
}

fn non_redacted_match_count(regex: &Regex, s: &str) -> usize {
    regex
        .find_iter(s)
        .filter(|matched| !is_redacted_match_without_raw_prefix(matched.as_str()))
        .count()
}

fn is_redacted_match_without_raw_prefix(text: &str) -> bool {
    let Some(marker_start) = text.find("[REDACTED") else {
        return false;
    };
    let prefix = text[..marker_start].trim_end().to_ascii_lowercase();
    if prefix.ends_with('=') || prefix.ends_with("://") {
        return true;
    }
    prefix.ends_with("cookie:") || prefix.ends_with("set-cookie:")
}

impl Redactor for DefaultRedactor {
    fn redact_text(&self, s: &str) -> String {
        let without_pems = self
            .pem_private_key_re
            .replace_all(s, "[REDACTED_PRIVATE_KEY]");
        let without_cookies = self
            .cookie_header_re
            .replace_all(without_pems.as_ref(), "Cookie: [REDACTED_COOKIE]");
        let without_bearers = self
            .bearer_re
            .replace_all(without_cookies.as_ref(), "Bearer [REDACTED]");
        let without_keys = self
            .api_key_re
            .replace_all(without_bearers.as_ref(), "${1}[REDACTED_API_KEY]");
        let without_google_keys = self
            .google_api_key_re
            .replace_all(without_keys.as_ref(), "[REDACTED_API_KEY]");
        let without_aws_keys = self
            .aws_access_key_re
            .replace_all(without_google_keys.as_ref(), "[REDACTED_AWS_ACCESS_KEY]");
        let without_github_pats = self
            .github_pat_re
            .replace_all(without_aws_keys.as_ref(), "[REDACTED_GITHUB_TOKEN]");
        let without_github_tokens = self
            .github_token_re
            .replace_all(without_github_pats.as_ref(), "[REDACTED_GITHUB_TOKEN]");
        let without_userinfo = self
            .url_userinfo_re
            .replace_all(without_github_tokens.as_ref(), "${1}[REDACTED]@");
        self.sensitive_query_re
            .replace_all(without_userinfo.as_ref(), "${1}[REDACTED]")
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
        .map(|(k, v)| {
            let key = redactor.redact_text(k);
            let value = if let Some(marker) = redaction_marker_for_sensitive_key(k) {
                match v {
                    Value::String(_) => Value::String(marker.to_string()),
                    _ => redact_value(redactor, v),
                }
            } else {
                redact_value(redactor, v)
            };
            (key, value)
        })
        .collect()
}

fn redaction_marker_for_sensitive_key(key: &str) -> Option<&'static str> {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect::<String>();
    if normalized == "credentials" {
        return None;
    }
    let segments = key
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_ascii_lowercase())
        .collect::<Vec<_>>();

    if normalized == "apikey"
        || normalized.ends_with("apikey")
        || adjacent_segments(&segments, "api", "key")
    {
        return Some("[REDACTED_API_KEY]");
    }
    if normalized == "auth" || normalized.contains("authorization") {
        return Some("Bearer [REDACTED]");
    }
    if normalized.contains("cookie") {
        return Some("[REDACTED_COOKIE]");
    }
    if normalized.contains("privatekey") || adjacent_segments(&segments, "private", "key") {
        return Some("[REDACTED_PRIVATE_KEY]");
    }
    if normalized.contains("password")
        || normalized.contains("passwd")
        || normalized.contains("secret")
        || normalized.contains("token")
        || normalized.contains("credential")
        || credential_key_segments(&segments)
    {
        return Some("[REDACTED_SECRET]");
    }

    None
}

fn adjacent_segments(segments: &[String], left: &str, right: &str) -> bool {
    segments
        .windows(2)
        .any(|window| window[0] == left && window[1] == right)
}

fn key_segments_contain(segments: &[String], needle: &str) -> bool {
    segments.iter().any(|segment| segment == needle)
}

fn credential_key_segments(segments: &[String]) -> bool {
    if !key_segments_contain(segments, "key") {
        return false;
    }
    segments.iter().any(|segment| {
        matches!(
            segment.as_str(),
            "access"
                | "api"
                | "auth"
                | "bearer"
                | "client"
                | "credential"
                | "github"
                | "google"
                | "openai"
                | "private"
                | "provider"
                | "secret"
                | "token"
                | "aws"
        )
    })
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

    #[test]
    fn redacts_support_bundle_secret_shapes() {
        // arrange
        let redactor = DefaultRedactor::default();
        let value = json!({
            "base_url": "https://user:pass@example.test/v1?api_key=AIzaSyA1234567890abcdefghi",
            "summary": "sk-proj-output_secret_0123456789abcdef Cookie: sid=sessionid-abc123\n-----BEGIN PRIVATE KEY-----\nprivate-key-material\n-----END PRIVATE KEY-----\nAKIA1234567890ABCDEF",
            "authorization": "Bearer abc.def-ghi_123",
            "mixed_header": "Authorization: bearer abc+/def==~ ghp_1234567890ABCDEFGHIJ github_pat_1234567890ABCDEFGHIJ",
            "token": "plain-token-value",
            "password": "hunter2",
            "sk-proj-key_name_0123456789abcdef": "secret in key name"
        });

        // act
        let redacted = redact_value(&redactor, &value);
        let text = redacted.to_string();

        // assert
        for forbidden in [
            "user:pass@",
            "api_key=AIzaSyA1234567890abcdefghi",
            "sk-proj-output_secret_0123456789abcdef",
            "sessionid-abc123",
            "BEGIN PRIVATE KEY",
            "private-key-material",
            "AKIA1234567890ABCDEF",
            "Bearer abc.def-ghi_123",
            "bearer abc+/def==~",
            "abc+/def==~",
            "ghp_1234567890ABCDEFGHIJ",
            "github_pat_1234567890ABCDEFGHIJ",
            "plain-token-value",
            "hunter2",
            "sk-proj-key_name_0123456789abcdef",
        ] {
            assert!(
                !text.contains(forbidden),
                "redacted value leaked {forbidden}"
            );
        }
        assert_eq!(redactor.secret_finding_count(&text), 0);
        assert!(text.contains("[REDACTED_API_KEY]"));
        assert!(text.contains("[REDACTED_PRIVATE_KEY]"));
        assert!(text.contains("[REDACTED_GITHUB_TOKEN]"));
        assert!(text.contains("[REDACTED_SECRET]"));
    }

    #[test]
    fn redacts_composite_sensitive_key_names() {
        // arrange
        let redactor = DefaultRedactor::default();
        let value = json!({
            "client_secret": "plain-client-secret-value",
            "github_token": "plain-github-token-value",
            "x-api-key": "plain-api-key-value",
            "openai_api_key": "plain-openai-key-value",
            "secret_access_key": "plain-access-key-value",
            "password_hash": "plain-password-hash-value",
            "metadata": "keep me"
        });

        // act
        let redacted = redact_value(&redactor, &value);
        let text = redacted.to_string();

        // assert
        for forbidden in [
            "plain-client-secret-value",
            "plain-github-token-value",
            "plain-api-key-value",
            "plain-openai-key-value",
            "plain-access-key-value",
            "plain-password-hash-value",
        ] {
            assert!(
                !text.contains(forbidden),
                "redacted value leaked {forbidden}"
            );
        }
        assert!(text.contains("keep me"));
    }

    #[test]
    fn preserves_typed_token_count_fields() {
        // arrange
        let redactor = DefaultRedactor::default();
        let value = json!({
            "usage": {
                "input_tokens": 123,
                "output_tokens": 45,
                "token": "plain-token-secret"
            }
        });

        // act
        let redacted = redact_value(&redactor, &value);

        // assert
        assert_eq!(redacted["usage"]["input_tokens"], 123);
        assert_eq!(redacted["usage"]["output_tokens"], 45);
        assert_eq!(redacted["usage"]["token"], "[REDACTED_SECRET]");
    }

    #[test]
    fn scanner_counts_raw_cookie_even_when_later_marker_exists() {
        // arrange
        let redactor = DefaultRedactor::default();
        let text = r#"{"summary":"Cookie: sid=raw-session","other":"[REDACTED_API_KEY]"}"#;

        // act
        let finding_count = redactor.secret_finding_count(text);

        // assert
        assert_eq!(finding_count, 1);
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
