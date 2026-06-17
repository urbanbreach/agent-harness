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

static API_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(^|[^A-Za-z0-9])(sk-[A-Za-z0-9._-]{10,})").expect("valid api key regex")
});
static GOOGLE_API_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"AIza[0-9A-Za-z_-]{20,}").expect("valid google api key regex"));
static AWS_ACCESS_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"AKIA[0-9A-Z]{16}").expect("valid aws access key regex"));
static GITHUB_PAT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"github_pat_[A-Za-z0-9_]{20,}").expect("valid github pat regex"));
static GITHUB_TOKEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"ghp_[A-Za-z0-9]{20,}").expect("valid github token regex"));
static BEARER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=-]+").expect("valid bearer regex")
});
static COOKIE_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:Set-Cookie|Cookie):\s*[^\r\n]+").expect("valid cookie header regex")
});
static PEM_PRIVATE_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----")
        .expect("valid pem private key regex")
});
static URL_USERINFO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(https?://)[^/@\s]+@").expect("valid url userinfo regex"));
static SENSITIVE_QUERY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)([?&](?:api[_-]?key|access[_-]?token|refresh[_-]?token|id[_-]?token|token|password|secret)=)[^&\s]+",
    )
    .expect("valid sensitive query regex")
});

impl Default for DefaultRedactor {
    fn default() -> Self {
        Self {
            api_key_re: API_KEY_RE.clone(),
            google_api_key_re: GOOGLE_API_KEY_RE.clone(),
            aws_access_key_re: AWS_ACCESS_KEY_RE.clone(),
            github_pat_re: GITHUB_PAT_RE.clone(),
            github_token_re: GITHUB_TOKEN_RE.clone(),
            bearer_re: BEARER_RE.clone(),
            cookie_header_re: COOKIE_HEADER_RE.clone(),
            pem_private_key_re: PEM_PRIVATE_KEY_RE.clone(),
            url_userinfo_re: URL_USERINFO_RE.clone(),
            sensitive_query_re: SENSITIVE_QUERY_RE.clone(),
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

fn contains_normalized(key: &str, target: &str) -> bool {
    let target_bytes = target.as_bytes();
    if target_bytes.is_empty() {
        return true;
    }
    let mut match_idx = 0;
    for c in key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
    {
        if c as u8 == target_bytes[match_idx] {
            match_idx += 1;
            if match_idx == target_bytes.len() {
                return true;
            }
        } else {
            if c as u8 == target_bytes[0] {
                match_idx = 1;
            } else {
                match_idx = 0;
            }
        }
    }
    false
}

fn credential_key_segments(key: &str) -> bool {
    let mut has_key = false;
    let mut has_other = false;

    for segment in key
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
    {
        if segment.eq_ignore_ascii_case("key") {
            has_key = true;
        } else if segment.eq_ignore_ascii_case("access")
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
        {
            has_other = true;
        }
    }

    has_key && has_other
}

fn redaction_marker_for_sensitive_key(key: &str) -> Option<&'static str> {
    // fast path exactly "credentials"
    let mut is_credentials = true;
    let mut len = 0;
    for c in key.chars().filter(|c| c.is_ascii_alphanumeric()) {
        if len >= 11 || c.to_ascii_lowercase() as u8 != b"credentials"[len] {
            is_credentials = false;
        }
        len += 1;
    }
    if is_credentials && len == 11 {
        return None;
    }

    let mut has_api_key_adj = false;
    let mut has_private_key_adj = false;
    let mut prev_seg: Option<&str> = None;

    for segment in key
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
    {
        if let Some(prev) = prev_seg {
            if prev.eq_ignore_ascii_case("api") && segment.eq_ignore_ascii_case("key") {
                has_api_key_adj = true;
            }
            if prev.eq_ignore_ascii_case("private") && segment.eq_ignore_ascii_case("key") {
                has_private_key_adj = true;
            }
        }
        prev_seg = Some(segment);
    }

    // Check apikey and auth
    let mut is_apikey = true;
    let mut is_auth = true;
    let mut len_apikey = 0;
    let mut len_auth = 0;

    for c in key.chars().filter(|c| c.is_ascii_alphanumeric()) {
        let l = c.to_ascii_lowercase() as u8;
        if len_apikey >= 6 || l != b"apikey"[len_apikey] {
            is_apikey = false;
        }
        len_apikey += 1;

        if len_auth >= 4 || l != b"auth"[len_auth] {
            is_auth = false;
        }
        len_auth += 1;
    }
    if is_apikey && len_apikey == 6 {
        return Some("[REDACTED_API_KEY]");
    }

    // Check ends with apikey
    let mut ends_with_apikey = true;
    let mut rev_len = 0;
    for c in key.chars().rev().filter(|c| c.is_ascii_alphanumeric()) {
        let l = c.to_ascii_lowercase() as u8;
        if rev_len >= 6 || l != b"yekipa"[rev_len] {
            ends_with_apikey = false;
        }
        rev_len += 1;
        if rev_len == 6 {
            break;
        }
    }
    ends_with_apikey = ends_with_apikey && rev_len >= 6;

    if ends_with_apikey || has_api_key_adj {
        return Some("[REDACTED_API_KEY]");
    }

    if (is_auth && len_auth == 4) || contains_normalized(key, "authorization") {
        return Some("Bearer [REDACTED]");
    }

    if contains_normalized(key, "cookie") {
        return Some("[REDACTED_COOKIE]");
    }

    if contains_normalized(key, "privatekey") || has_private_key_adj {
        return Some("[REDACTED_PRIVATE_KEY]");
    }

    if contains_normalized(key, "password")
        || contains_normalized(key, "passwd")
        || contains_normalized(key, "secret")
        || contains_normalized(key, "token")
        || contains_normalized(key, "credential")
        || credential_key_segments(key)
    {
        return Some("[REDACTED_SECRET]");
    }

    None
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
