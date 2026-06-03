use std::collections::BTreeMap;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use super::OpenAiCompatibleProviderError;

pub(super) fn parse_headers(
    headers: &BTreeMap<String, String>,
) -> Result<HeaderMap, OpenAiCompatibleProviderError> {
    let mut parsed = HeaderMap::new();

    for (name, value) in headers {
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|source| {
            OpenAiCompatibleProviderError::InvalidHeaderName {
                header: name.clone(),
                source,
            }
        })?;

        let header_value = HeaderValue::from_str(value).map_err(|source| {
            OpenAiCompatibleProviderError::InvalidHeaderValue {
                header: name.clone(),
                source,
            }
        })?;

        parsed.insert(header_name, header_value);
    }

    Ok(parsed)
}

pub(super) fn insert_static_header(
    headers: &mut HeaderMap,
    name: &'static str,
    value: &str,
) -> Result<(), String> {
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|err| format!("invalid openai_compatible `{name}` header name: {err}"))?;
    let value = HeaderValue::from_str(value)
        .map_err(|err| format!("invalid openai_compatible `{name}` header value: {err}"))?;
    headers.insert(name, value);
    Ok(())
}

pub(super) fn remove_header_case_insensitive(headers: &mut HeaderMap, name: &str) {
    let names = headers
        .keys()
        .filter(|candidate| candidate.as_str().eq_ignore_ascii_case(name))
        .cloned()
        .collect::<Vec<_>>();
    for name in names {
        headers.remove(name);
    }
}
