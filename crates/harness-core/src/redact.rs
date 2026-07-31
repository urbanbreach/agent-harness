use crate::UnwrapOrAbort;
use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

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

static API_KEY_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(^|[^A-Za-z0-9])(sk-[A-Za-z0-9._-]{10,})"));
static GOOGLE_API_KEY_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"AIza[0-9A-Za-z_-]{20,}"));
static AWS_ACCESS_KEY_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"AKIA[0-9A-Z]{16}"));
static GITHUB_PAT_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"github_pat_[A-Za-z0-9_]{20,}"));
static GITHUB_TOKEN_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"ghp_[A-Za-z0-9]{20,}"));
static BEARER_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=-]+"));
static COOKIE_HEADER_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:Set-Cookie|Cookie):\s*[^\r\n]+"));
static PEM_PRIVATE_KEY_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----")
});
static URL_USERINFO_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?i)(https?://)[^/@\s]+@"));
static SENSITIVE_QUERY_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"(?i)([?&](?:api[_-]?key|access[_-]?token|refresh[_-]?token|id[_-]?token|token|password|secret)=)[^&\s]+",
    )
});

fn regex_or_fallback(result: Result<Regex, regex::Error>) -> Regex {
    match result {
        Ok(re) => re,
        Err(_) => Regex::new("$^").unwrap_or_abort(),
    }
}

impl Default for DefaultRedactor {
    fn default() -> Self {
        Self {
            api_key_re: regex_or_fallback(API_KEY_RE.clone()),
            google_api_key_re: regex_or_fallback(GOOGLE_API_KEY_RE.clone()),
            aws_access_key_re: regex_or_fallback(AWS_ACCESS_KEY_RE.clone()),
            github_pat_re: regex_or_fallback(GITHUB_PAT_RE.clone()),
            github_token_re: regex_or_fallback(GITHUB_TOKEN_RE.clone()),
            bearer_re: regex_or_fallback(BEARER_RE.clone()),
            cookie_header_re: regex_or_fallback(COOKIE_HEADER_RE.clone()),
            pem_private_key_re: regex_or_fallback(PEM_PRIVATE_KEY_RE.clone()),
            url_userinfo_re: regex_or_fallback(URL_USERINFO_RE.clone()),
            sensitive_query_re: regex_or_fallback(SENSITIVE_QUERY_RE.clone()),
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

    if normalized == "apikey"
        || normalized.ends_with("apikey")
        || adjacent_segments(key, "api", "key")
    {
        return Some("[REDACTED_API_KEY]");
    }
    if normalized == "auth" || normalized.contains("authorization") {
        return Some("Bearer [REDACTED]");
    }
    if normalized.contains("cookie") {
        return Some("[REDACTED_COOKIE]");
    }
    if normalized.contains("privatekey") || adjacent_segments(key, "private", "key") {
        return Some("[REDACTED_PRIVATE_KEY]");
    }
    if normalized.contains("password")
        || normalized.contains("passwd")
        || normalized.contains("secret")
        || normalized.contains("token")
        || normalized.contains("credential")
        || credential_key_segments(key)
    {
        return Some("[REDACTED_SECRET]");
    }

    None
}

fn segments_iter(key: &str) -> impl Iterator<Item = &str> {
    key.split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|segment| !segment.is_empty())
}

fn adjacent_segments(key: &str, left: &str, right: &str) -> bool {
    let mut prev = None;
    for segment in segments_iter(key) {
        if let Some(p) = prev {
            if p == left && segment.eq_ignore_ascii_case(right) {
                return true;
            }
        }
        if segment.eq_ignore_ascii_case(left) {
            prev = Some(left);
        } else {
            prev = None;
        }
    }
    false
}

fn key_segments_contain(key: &str, needle: &str) -> bool {
    segments_iter(key).any(|segment| segment.eq_ignore_ascii_case(needle))
}

fn credential_key_segments(key: &str) -> bool {
    if !key_segments_contain(key, "key") {
        return false;
    }
    segments_iter(key).any(|segment| {
        segment.eq_ignore_ascii_case("access")
            || segment.eq_ignore_ascii_case("api")
            || segment.eq_ignore_ascii_case("auth")
            || segment.eq_ignore_ascii_case("bearer")
            || segment.eq_ignore_ascii_case("client")
            || segment.eq_ignore_ascii_case("credential")
            || segment.eq_ignore_ascii_case("github")
            || segment.eq_ignore_ascii_case("google")
            || segment.eq_ignore_ascii_case("openai")
            || segment.eq_ignore_ascii_case("private")
            || segment.eq_ignore_ascii_case("provider")
            || segment.eq_ignore_ascii_case("secret")
            || segment.eq_ignore_ascii_case("token")
            || segment.eq_ignore_ascii_case("aws")
    })
}

#[cfg(test)]
mod tests {
    use super::{redact_map, redact_value, DefaultRedactor, Redactor};
    use crate::UnwrapOrAbort;
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

        let map = value.as_object().unwrap_or_abort();
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

            let entries = fs::read_dir(path).unwrap_or_abort();
            for entry in entries {
                let entry = entry.unwrap_or_abort();
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
        let dir = tempfile::tempdir().unwrap_or_abort();
        let file = dir.path().join("events.jsonl");
        fs::write(&file, "{\"line\":\"sk-should-not-be-here\"}").unwrap_or_abort();

        let panic = std::panic::catch_unwind(|| SecretScan::assert_no_sk_in_dir(dir.path()));
        assert!(panic.is_err(), "secret scan should fail for sk- leakage");
    }

    #[test]
    fn secret_scan_helper_allows_redacted_jsonl() {
        let dir = tempfile::tempdir().unwrap_or_abort();
        let file = dir.path().join("events.jsonl");
        fs::write(&file, "{\"line\":\"[REDACTED_API_KEY]\"}").unwrap_or_abort();

        SecretScan::assert_no_sk_in_dir(dir.path());
    }

    #[test]
    fn secret_scan_helper_fails_when_artifact_contains_sk_prefix() {
        let dir = tempfile::tempdir().unwrap_or_abort();
        let file = dir
            .path()
            .join("artifacts")
            .join("toolcalls")
            .join("call_1")
            .join("result.redacted.json");
        fs::create_dir_all(file.parent().unwrap_or_abort()).unwrap_or_abort();
        fs::write(&file, "{\"display_text\":\"sk-should-not-be-here\"}").unwrap_or_abort();

        let panic = std::panic::catch_unwind(|| SecretScan::assert_no_sk_in_dir(dir.path()));
        assert!(
            panic.is_err(),
            "secret scan should fail for sk- leakage in artifacts"
        );
    }

    #[test]
    fn secret_scan_helper_allows_redacted_artifact_files() {
        let dir = tempfile::tempdir().unwrap_or_abort();
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
        fs::create_dir_all(result.parent().unwrap_or_abort()).unwrap_or_abort();

        fs::write(&result, "{\"display_text\":\"[REDACTED_API_KEY]\"}").unwrap_or_abort();
        fs::write(&display, "token=[REDACTED_API_KEY]").unwrap_or_abort();

        SecretScan::assert_no_sk_in_dir(dir.path());
    }

    #[test]
    fn secret_scan_helper_fails_when_snap_file_contains_sk_prefix() {
        let dir = tempfile::tempdir().unwrap_or_abort();
        let file = dir
            .path()
            .join("crates")
            .join("harness-tui")
            .join("tests")
            .join("snapshots")
            .join("pty_after_tool_call.snap");
        fs::create_dir_all(file.parent().unwrap_or_abort()).unwrap_or_abort();
        fs::write(&file, "tool output: sk-should-not-be-here").unwrap_or_abort();

        let panic = std::panic::catch_unwind(|| SecretScan::assert_no_sk_in_dir(dir.path()));
        assert!(
            panic.is_err(),
            "secret scan should fail for sk- leakage in .snap files"
        );
    }

    #[test]
    fn secret_scan_helper_allows_redacted_snap_file() {
        let dir = tempfile::tempdir().unwrap_or_abort();
        let file = dir
            .path()
            .join("crates")
            .join("harness-tui")
            .join("tests")
            .join("snapshots")
            .join("pty_after_tool_call.snap");
        fs::create_dir_all(file.parent().unwrap_or_abort()).unwrap_or_abort();
        fs::write(&file, "tool output: [REDACTED_API_KEY]").unwrap_or_abort();

        SecretScan::assert_no_sk_in_dir(dir.path());
    }
}
