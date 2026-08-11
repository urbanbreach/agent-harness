use super::RawDisclosureError;
use serde_json::{Map, Value};

const REDACTED: &str = "<redacted>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawPayload {
    Text(String),
    Json(Value),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactionReason {
    SensitiveField,
    Authorization,
    ProviderToken,
    PrivateKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redaction {
    pub path: String,
    pub reason: RedactionReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDisclosure {
    pub payload: RawPayload,
    redactions: Vec<Redaction>,
}

impl RawDisclosure {
    pub fn from_text(source: &str) -> Self {
        let (text, count) = redact_text(source);
        let redactions = (0..count)
            .map(|_| Redaction {
                path: "$".to_string(),
                reason: RedactionReason::ProviderToken,
            })
            .collect();
        Self {
            payload: RawPayload::Text(text),
            redactions,
        }
    }

    pub fn from_json(source: &Value) -> Self {
        let mut redactions = Vec::new();
        let payload = RawPayload::Json(redact_value(source, "$", &mut redactions));
        Self {
            payload,
            redactions,
        }
    }

    pub fn from_json_text(source: &str) -> Result<Self, RawDisclosureError> {
        let value = serde_json::from_str(source)
            .map_err(|error| RawDisclosureError::InvalidJson(error.to_string()))?;
        Ok(Self::from_json(&value))
    }
}

fn redact_value(value: &Value, path: &str, redactions: &mut Vec<Redaction>) -> Value {
    match value {
        Value::Object(object) => Value::Object(redact_object(object, path, redactions)),
        Value::Array(array) => Value::Array(
            array
                .iter()
                .enumerate()
                .map(|(index, item)| redact_value(item, &format!("{path}[{index}]"), redactions))
                .collect(),
        ),
        Value::String(text) => {
            let (redacted, count) = redact_text(text);
            for _ in 0..count {
                redactions.push(Redaction {
                    path: path.to_string(),
                    reason: RedactionReason::ProviderToken,
                });
            }
            Value::String(redacted)
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
    }
}

fn redact_object(
    object: &Map<String, Value>,
    path: &str,
    redactions: &mut Vec<Redaction>,
) -> Map<String, Value> {
    object
        .iter()
        .map(|(key, value)| {
            let child_path = format!("{path}.{key}");
            let value = if is_sensitive_key(key) {
                redact_sensitive_value(key, value, &child_path, redactions)
            } else {
                redact_value(value, &child_path, redactions)
            };
            (key.clone(), value)
        })
        .collect()
}

fn redact_sensitive_value(
    key: &str,
    value: &Value,
    path: &str,
    redactions: &mut Vec<Redaction>,
) -> Value {
    match value {
        Value::String(text) => {
            redactions.push(Redaction {
                path: path.to_string(),
                reason: if key.eq_ignore_ascii_case("authorization") {
                    RedactionReason::Authorization
                } else if key.to_ascii_lowercase().contains("private") {
                    RedactionReason::PrivateKey
                } else {
                    RedactionReason::SensitiveField
                },
            });
            if key.eq_ignore_ascii_case("authorization")
                && text
                    .get(..7)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer "))
            {
                Value::String("Bearer <redacted>".to_string())
            } else {
                Value::String(REDACTED.to_string())
            }
        }
        _ => redact_value(value, path, redactions),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key == "authorization"
        || key == "api_key"
        || key == "access_token"
        || key == "refresh_token"
        || key == "token"
        || key == "secret"
        || key == "password"
        || key == "private_key"
        || key.contains("credential")
}

fn redact_text(source: &str) -> (String, usize) {
    let (source, bearer_count) = redact_prefixed(
        source,
        "bearer ",
        8,
        |character| character.is_ascii_alphanumeric() || ".~+/=-".contains(character),
        "Bearer <redacted>",
        true,
    );
    let (source, pem_count) = redact_pem(&source);
    let (source, anthropic_count) = prefixed_token(&source, "sk-ant-", 10);
    let (source, openai_count) = prefixed_token(&source, "sk-", 10);
    let (source, google_count) = prefixed_token(&source, "AIza", 20);
    let (source, aws_count) = redact_prefixed(
        &source,
        "AKIA",
        16,
        |character| character.is_ascii_uppercase() || character.is_ascii_digit(),
        REDACTED,
        false,
    );
    let (source, github_pat_count) = prefixed_token(&source, "github_pat_", 20);
    let (source, github_count) = redact_prefixed(
        &source,
        "ghp_",
        20,
        |character| character.is_ascii_alphanumeric(),
        REDACTED,
        false,
    );
    (
        source,
        bearer_count
            + pem_count
            + anthropic_count
            + openai_count
            + google_count
            + aws_count
            + github_pat_count
            + github_count,
    )
}

fn prefixed_token(source: &str, prefix: &str, minimum: usize) -> (String, usize) {
    redact_prefixed(source, prefix, minimum, is_token_character, REDACTED, false)
}

fn redact_prefixed(
    source: &str,
    prefix: &str,
    minimum: usize,
    valid: fn(char) -> bool,
    replacement: &str,
    ignore_case: bool,
) -> (String, usize) {
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    let mut count = 0;
    while cursor < source.len() {
        let relative = if ignore_case {
            source[cursor..].char_indices().find_map(|(index, _)| {
                source
                    .get(cursor + index..)
                    .filter(|tail| {
                        tail.get(..prefix.len())
                            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
                    })
                    .map(|_| index)
            })
        } else {
            source[cursor..].find(prefix)
        };
        let Some(relative) = relative else {
            break;
        };
        let start = cursor + relative;
        let token_start = start + prefix.len();
        let token_end = token_end(source, token_start, valid);
        if token_end - token_start < minimum {
            output.push_str(&source[cursor..token_start]);
            cursor = token_start;
            continue;
        }
        output.push_str(&source[cursor..start]);
        output.push_str(replacement);
        cursor = token_end;
        count += 1;
    }
    output.push_str(&source[cursor..]);
    (output, count)
}

fn redact_pem(source: &str) -> (String, usize) {
    let Some(start) = source.find("-----BEGIN ") else {
        return (source.to_string(), 0);
    };
    let Some(relative_end) = source[start..].find("PRIVATE KEY-----") else {
        return (source.to_string(), 0);
    };
    let end = start + relative_end + "PRIVATE KEY-----".len();
    let mut output = String::with_capacity(source.len());
    output.push_str(&source[..start]);
    output.push_str(REDACTED);
    output.push_str(&source[end..]);
    (output, 1)
}

fn token_end(source: &str, start: usize, valid: fn(char) -> bool) -> usize {
    source[start..]
        .char_indices()
        .take_while(|(_, character)| valid(*character))
        .last()
        .map_or(start, |(index, character)| {
            start + index + character.len_utf8()
        })
}

fn is_token_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{RawDisclosure, RawPayload};

    #[test]
    fn from_json_preserves_multibyte_text_without_credentials() {
        // Given
        let source = json!({"summary": "gpt-5 · high"});

        // When
        let disclosure = RawDisclosure::from_json(&source);

        // Then
        assert_eq!(disclosure.payload, RawPayload::Json(source));
    }
}
