use regex::{Captures, Regex};
use serde_json::Value;
use std::sync::LazyLock;

static SECRETS: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?P<private>(?s:-----BEGIN [A-Z ]*PRIVATE KEY-----.*?(?:-----END [A-Z ]*PRIVATE KEY-----|\z)))",
        r"|(?P<cookie>(?i:\b(?:set-cookie|cookie):)[^\r\n]+)",
        r"|(?P<bearer>(?i:\bbearer\s+)[A-Za-z0-9._~+/=-]+)",
        r"|(?P<key>\bsk-[A-Za-z0-9._-]{10,}|AIza[0-9A-Za-z_-]{20,})",
        r"|(?P<aws>AKIA[0-9A-Z]{16})",
        r"|(?P<github>github_pat_[A-Za-z0-9_]{20,}|gh[pousr]_[A-Za-z0-9]{20,})",
        r"|(?P<url>(?i:https?://)[^/@\s]+@)",
        r#"|(?P<query>(?i:[?&](?:api[_-]?key|(?:access|refresh|id)[_-]?token|token|password|secret)=)[^&\s"'<>]+)"#,
        r#"|(?P<assignment>(?i:\b[A-Za-z0-9_-]*(?:api[_-]?key|(?:access|refresh|id)[_-]?token|password|passwd|client[_-]?secret))["']?[ \t]*[:=][ \t]*(?:"(?:[^"\\\r\n]|\\.)*"|'(?:[^'\\\r\n]|\\.)*'|[^\s,;"'}]+))"#,
    ))
});

pub trait Redactor {
    fn redact_text(&self, text: &str) -> String;
    /// Bytes that future fragments cannot turn into a redaction match.
    /// Custom redactors defer output until completion unless they implement this.
    fn streaming_prefix(&self, _text: &str) -> usize {
        0
    }
}
pub struct SecretRedactor {
    inner: std::sync::Arc<dyn Redactor + Send + Sync>,
    secrets: std::sync::Arc<SecretRegistry>,
}
impl SecretRedactor {
    pub fn new(
        inner: std::sync::Arc<dyn Redactor + Send + Sync>,
        secrets: std::sync::Arc<SecretRegistry>,
    ) -> Self {
        Self { inner, secrets }
    }
}

/// Credentials learned during a run remain redacted after rotation.
#[derive(Default)]
pub struct SecretRegistry(std::sync::RwLock<Vec<String>>);
impl SecretRegistry {
    pub fn register(&self, values: impl IntoIterator<Item = String>) -> Result<(), &'static str> {
        let mut secrets = self
            .0
            .write()
            .map_err(|_| "secret registry lock poisoned")?;
        let mut bytes: usize = secrets.iter().map(String::len).sum();
        let mut added = Vec::new();
        for value in values
            .into_iter()
            .flat_map(|value| {
                let token = value
                    .split_once(' ')
                    .filter(|(scheme, _)| {
                        scheme.eq_ignore_ascii_case("bearer")
                            || scheme.eq_ignore_ascii_case("basic")
                    })
                    .map(|(_, token)| token.to_owned());
                std::iter::once(value).chain(token)
            })
            .filter(|s| !s.is_empty())
        {
            if secrets.contains(&value) || added.contains(&value) {
                continue;
            }
            bytes = bytes.saturating_add(value.len());
            if secrets.len() + added.len() >= 1024 || bytes > 4 * 1024 * 1024 {
                return Err("secret registry exceeds 1024 values or 4 MiB; start a new run");
            }
            added.push(value);
        }
        secrets.extend(added);
        secrets.sort_unstable_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        Ok(())
    }
}
impl Redactor for SecretRedactor {
    fn redact_text(&self, text: &str) -> String {
        let mut text = self.inner.redact_text(text);
        let Ok(secrets) = self.secrets.0.read() else {
            return "[REDACTED]".into();
        };
        // ponytail: one scan per credential; use multi-pattern matching if this profiles hot.
        for secret in secrets.iter() {
            if text.contains(secret) {
                text = text.replace(secret, "[REDACTED]");
            }
        }
        text
    }
    fn streaming_prefix(&self, text: &str) -> usize {
        let limit = self.inner.streaming_prefix(text);
        let Ok(secrets) = self.secrets.0.read() else {
            return 0;
        };
        if limit == 0 || secrets.iter().any(|s| s.trim().is_empty()) {
            return 0;
        }
        word_prefix(text, |word| {
            secrets.iter().any(|s| {
                s.split_whitespace()
                    .next()
                    .is_some_and(|prefix| word.contains(prefix))
            })
        })
        .min(limit)
    }
}
#[derive(Debug, Default)]
pub struct DefaultRedactor {
    _private: (),
}

impl Redactor for DefaultRedactor {
    fn redact_text(&self, text: &str) -> String {
        match SECRETS.as_ref() {
            Ok(regex) => regex.replace_all(text, replace_secret).into_owned(),
            Err(_) => "[REDACTED]".into(),
        }
    }
    fn streaming_prefix(&self, text: &str) -> usize {
        static PREFIX: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
            Regex::new(concat!(
                r"(?i:bearer|cookie|api[_-]?key|(?:access|refresh|id)[_-]?token|password|passwd|client[_-]?secret|[?&](?:token|secret)=|https?://)",
                r"|sk-|AIza|AKIA|github_pat_|gh[pousr]_|-----BEGIN"
            ))
        });
        PREFIX.as_ref().map_or(0, |pattern| {
            word_prefix(text, |word| pattern.is_match(word))
        })
    }
}

// Keep incomplete words and whitespace after the last emitted word. A stored
// credential can start with spaces, which must stay with the following word.
fn word_prefix(text: &str, suspect: impl Fn(&str) -> bool) -> usize {
    let (mut start, mut safe) = (0, 0);
    for (end, separator) in text.char_indices().filter(|(_, c)| c.is_whitespace()) {
        if end > start {
            if suspect(&text[start..end]) {
                break;
            }
            safe = end;
        }
        start = end + separator.len_utf8();
    }
    safe
}
impl DefaultRedactor {
    pub fn secret_finding_count(&self, text: &str) -> usize {
        SECRETS.as_ref().map_or(1, |re| {
            re.captures_iter(text)
                .filter(|c| replace_secret(c) != c[0])
                .count()
        })
    }
}

fn replace_secret(c: &Captures<'_>) -> String {
    if c.name("url").is_some() {
        return format!(
            "{}://[REDACTED]@",
            c[0].split("://").next().unwrap_or("https")
        );
    }
    if c.name("query").is_some() {
        return format!("{}=[REDACTED]", c[0].split('=').next().unwrap_or("secret"));
    }
    if c.name("assignment").is_some() {
        let Some(index) = c[0].find([':', '=']) else {
            return "[REDACTED]".into();
        };
        let (prefix, value) = c[0].split_at(index + 1);
        if value.trim().trim_matches(['\'', '"']) == "[REDACTED]" {
            return c[0].into();
        }
        return format!("{prefix}\"[REDACTED]\"");
    }
    for (group, replacement) in [
        ("private", "[REDACTED_PRIVATE_KEY]"),
        ("cookie", "Cookie: [REDACTED_COOKIE]"),
        ("bearer", "Bearer [REDACTED]"),
        ("key", "[REDACTED_API_KEY]"),
        ("aws", "[REDACTED_AWS_ACCESS_KEY]"),
        ("github", "[REDACTED_GITHUB_TOKEN]"),
    ] {
        if c.name(group).is_some() {
            return replacement.into();
        }
    }
    "[REDACTED]".into()
}

pub fn redact_value(redactor: &(impl Redactor + ?Sized), value: &Value) -> Value {
    let mut value = value.clone();
    redact_in_place(redactor, &mut value);
    value
}

/// Scan semantic strings and keys, so a JSON null credential is not a secret.
pub fn has_unredacted_secret(redactor: &(impl Redactor + ?Sized), value: &Value) -> bool {
    let scanner = DefaultRedactor::default();
    let secret =
        |text: &str| redactor.redact_text(text) != text || scanner.secret_finding_count(text) != 0;
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::String(text) if secret(text) => return true,
            Value::Array(values) => pending.extend(values),
            Value::Object(values) => {
                for (key, value) in values {
                    if secret(key) {
                        return true;
                    }
                    pending.push(value);
                }
            }
            _ => {}
        }
    }
    false
}

pub(crate) fn redact_event_payload(
    redactor: &(impl Redactor + ?Sized),
    payload: crate::event::EventV1,
) -> Result<crate::event::EventV1, serde_json::Error> {
    let mut value = serde_json::to_value(payload)?;
    redact_in_place(redactor, &mut value);
    serde_json::from_value(value)
}

pub fn redact_in_place(redactor: &(impl Redactor + ?Sized), value: &mut Value) {
    match value {
        Value::String(text) => *text = redactor.redact_text(text),
        Value::Array(items) => {
            for item in items {
                redact_in_place(redactor, item);
            }
        }
        Value::Object(map) => {
            for (key, value) in map {
                let normalized: String = key
                    .chars()
                    .filter(char::is_ascii_alphanumeric)
                    .flat_map(char::to_lowercase)
                    .collect();
                if [
                    "apikey",
                    "apisecret",
                    "accesstoken",
                    "refreshtoken",
                    "idtoken",
                    "password",
                    "passwd",
                    "authorization",
                    "cookie",
                    "secret",
                    "privatekey",
                ]
                .iter()
                .any(|suffix| normalized.ends_with(suffix))
                    || normalized == "token"
                {
                    if !value.is_null() {
                        *value = "[REDACTED]".into();
                    }
                } else {
                    redact_in_place(redactor, value);
                }
            }
        }
        _ => {}
    }
}

pub fn redact_map(
    redactor: &(impl Redactor + ?Sized),
    map: &serde_json::Map<String, Value>,
) -> serde_json::Map<String, Value> {
    match redact_value(redactor, &Value::Object(map.clone())) {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    }
}

pub fn redact_artifact_text(text: &str) -> String {
    let redactor = DefaultRedactor::default();
    if let Ok(value) = json5::from_str::<Value>(text) {
        let safe = redact_value(&redactor, &value);
        if safe != value {
            return safe.to_string();
        }
    }
    redactor.redact_text(text)
}

#[derive(Default)]
pub struct LineRedactor {
    in_private_key: bool,
    redactor: DefaultRedactor,
}

impl LineRedactor {
    pub fn redact_line(&mut self, line: &str) -> String {
        if line.contains("-----BEGIN ") && line.contains("PRIVATE KEY-----") {
            self.in_private_key = true;
        }
        if self.in_private_key {
            if line.contains("-----END ") && line.contains("PRIVATE KEY-----") {
                self.in_private_key = false;
            }
            "[REDACTED_PRIVATE_KEY]".into()
        } else {
            self.redactor.redact_text(line)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn streamed_prefix_stays_safe_across_character_boundaries() -> Result<(), &'static str> {
        let registry = std::sync::Arc::new(SecretRegistry::default());
        registry.register([
            "opaque two-word secret".into(),
            "  leading-space-token".into(),
        ])?;
        let redactor =
            SecretRedactor::new(std::sync::Arc::new(DefaultRedactor::default()), registry);
        for text in [
            "Visible text with Unicode λ and more words. ",
            "Visible Bearer\nabc.def tail ",
            "Visible -----BEGIN PRIVATE KEY-----\nbody\n-----END PRIVATE KEY----- tail ",
            "Visible CLIENTſECRET = 'hidden value' tail ",
            "Visible coo\u{212a}ie: session=hidden\r\n tail ",
            "Visible https://name:password@example.test/?token=private tail ",
            "Visible sk-1234567890123456 tail ",
            "Visible opaque two-word secret tail ",
            "Visible  leading-space-token tail ",
        ] {
            let expected = redactor.redact_text(text);
            let (mut pending, mut emitted) = (String::new(), String::new());
            for character in text.chars() {
                pending.push(character);
                let count = redactor.streaming_prefix(&pending);
                emitted.extend(pending.drain(..count));
                assert!(
                    expected.starts_with(&emitted),
                    "streamed prefix changed after completion"
                );
            }
            assert!(emitted.starts_with("Visible"));
        }
        Ok(())
    }

    #[test]
    fn redaction_removes_structured_credentials_and_is_idempotent() {
        let redactor = DefaultRedactor::default();
        let value = json!({"OPENAI_API_KEY":"opaque-credential", "nested":[{"password":"private-value"}],
            "prompt_tokens":123, "text":"Bearer abc.def.ghi https://alice:password@example.test/?token=private-query"});
        let safe = redact_value(&redactor, &value);
        for secret in [
            "opaque-credential",
            "private-value",
            "abc.def.ghi",
            "alice:password",
            "private-query",
        ] {
            assert!(!safe.to_string().contains(secret));
        }
        assert_eq!(safe["prompt_tokens"], 123);
        assert_eq!(redact_value(&redactor, &safe), safe);
        let text = "sk-1234567890abcdefgh\nCookie: session=hidden\n-----BEGIN PRIVATE KEY-----\nsecret-body";
        assert!(redactor.secret_finding_count(text) > 0);
        let safe = redactor.redact_text(text);
        assert_eq!(redactor.secret_finding_count(&safe), 0);
        assert_eq!(redactor.redact_text(&safe), safe);
        assert!(!safe.contains("secret-body"));
        for text in [
            "API_KEY=\"private-quoted\"",
            "PASSWORD='private-quoted'",
            "+  \"client_secret\": \"private-quoted\",",
        ] {
            let safe = redactor.redact_text(text);
            assert!(
                !safe.contains("private-quoted"),
                "unredacted assignment: {safe}"
            );
            assert_eq!(redactor.redact_text(&safe), safe);
        }
    }
}
