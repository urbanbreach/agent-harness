//! Native compaction endpoints, derived from an endpoint or base URL the way the lane
//! needs them. Pure string rules; the transport adds nothing.

const OPENAI_BASE: &str = "https://api.openai.com/v1";
const CODEX_BASE: &str = "https://chatgpt.com/backend-api";

/// Inputs of [`resolve_native_endpoints`].
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeEndpointInput<'a> {
    /// Base URL, or the provider's full Responses endpoint. `None` selects the default host.
    pub base: Option<&'a str>,
    /// The Codex subscription route.
    pub codex: bool,
    /// Explicit compact endpoint for lane V1; used unchanged when non-empty.
    pub endpoint: Option<&'a str>,
    /// Explicit streaming endpoint for lane V2.
    pub v2_endpoint: Option<&'a str>,
    /// Older name of `v2_endpoint`, consulted only when `v2_endpoint` is absent.
    pub legacy_v2_endpoint: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeEndpoints {
    pub v1: String,
    pub v2: String,
}

pub fn resolve_native_endpoints(input: &NativeEndpointInput<'_>) -> NativeEndpoints {
    let base = input
        .base
        .filter(|base| !base.is_empty())
        .unwrap_or(if input.codex { CODEX_BASE } else { OPENAI_BASE });
    let v1 = match input.endpoint.filter(|endpoint| !endpoint.is_empty()) {
        Some(endpoint) => endpoint.to_owned(),
        None => v1_from_base(base, input.codex),
    };
    // Nullish precedence: an empty first field is still "present" and falls to the derived URL.
    let v2 = match input.v2_endpoint.or(input.legacy_v2_endpoint) {
        Some(endpoint) if !endpoint.is_empty() => endpoint.to_owned(),
        _ => v2_from_base(base, input.codex),
    };
    NativeEndpoints { v1, v2 }
}

fn v1_from_base(base: &str, codex: bool) -> String {
    let base = base.strip_suffix('/').unwrap_or(base);
    // A full Responses endpoint is accepted as the base; the spec's rules would append twice.
    if base.ends_with("/responses/compact") {
        return base.to_owned();
    }
    if let Some(responses) = base.strip_suffix("/responses") {
        return format!("{responses}/responses/compact");
    }
    let suffix = if codex {
        if codex_suffix(base) {
            "/responses/compact"
        } else {
            "/codex/responses/compact"
        }
    } else if base.ends_with("/v1") {
        "/responses/compact"
    } else {
        "/v1/responses/compact"
    };
    format!("{base}{suffix}")
}

fn v2_from_base(base: &str, codex: bool) -> String {
    let base = base.trim_end_matches('/');
    let suffix = if codex {
        if base.ends_with("/codex/responses") {
            ""
        } else if base.ends_with("/codex") {
            "/responses"
        } else {
            "/codex/responses"
        }
    } else if base.ends_with("/responses") {
        ""
    } else if base.ends_with("/v1") {
        "/responses"
    } else {
        "/v1/responses"
    };
    format!("{base}{suffix}")
}

/// `/codex` or `/codex/v<digits>` at the end of the path.
fn codex_suffix(base: &str) -> bool {
    let tail = base.rsplit_once("/codex");
    tail.is_some_and(|(_, rest)| {
        rest.is_empty()
            || rest
                .strip_prefix("/v")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_follow_the_documented_rules() {
        let resolve = |base, codex, endpoint, v2, legacy| {
            resolve_native_endpoints(&NativeEndpointInput {
                base,
                codex,
                endpoint,
                v2_endpoint: v2,
                legacy_v2_endpoint: legacy,
            })
        };
        // (base, codex, v1 endpoint, v2 endpoint, legacy) -> (v1, v2)
        let cases = [
            (
                None,
                false,
                None,
                None,
                None,
                "https://api.openai.com/v1/responses/compact",
                "https://api.openai.com/v1/responses",
            ),
            (
                Some("https://gw.example/"),
                false,
                None,
                None,
                None,
                "https://gw.example/v1/responses/compact",
                "https://gw.example/v1/responses",
            ),
            (
                Some("https://gw.example/v1//"),
                false,
                None,
                None,
                None,
                "https://gw.example/v1//v1/responses/compact",
                "https://gw.example/v1/responses",
            ),
            (
                Some("https://gw.example/v1/responses"),
                false,
                None,
                None,
                None,
                "https://gw.example/v1/responses/compact",
                "https://gw.example/v1/responses",
            ),
            (
                None,
                true,
                None,
                None,
                None,
                "https://chatgpt.com/backend-api/codex/responses/compact",
                "https://chatgpt.com/backend-api/codex/responses",
            ),
            (
                Some("https://h.example/codex/v2"),
                true,
                None,
                None,
                None,
                "https://h.example/codex/v2/responses/compact",
                "https://h.example/codex/v2/codex/responses",
            ),
            (
                Some("https://h.example/codex/"),
                true,
                None,
                None,
                None,
                "https://h.example/codex/responses/compact",
                "https://h.example/codex/responses",
            ),
            (
                None,
                false,
                Some("https://proxy/x"),
                Some("https://stream/y"),
                Some("https://old/z"),
                "https://proxy/x",
                "https://stream/y",
            ),
            (
                None,
                false,
                Some(""),
                Some(""),
                Some("https://old/z"),
                "https://api.openai.com/v1/responses/compact",
                "https://api.openai.com/v1/responses",
            ),
            (
                None,
                false,
                None,
                None,
                Some("https://old/z"),
                "https://api.openai.com/v1/responses/compact",
                "https://old/z",
            ),
        ];
        for (base, codex, endpoint, v2, legacy, want_v1, want_v2) in cases {
            let got = resolve(base, codex, endpoint, v2, legacy);
            assert_eq!(
                (got.v1.as_str(), got.v2.as_str()),
                (want_v1, want_v2),
                "{base:?}"
            );
        }
    }
}
