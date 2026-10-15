//! Deployment-aware image caps shared by archive planning, rebuild, rescue and wire checks.
use harness_tokenizer::{classify, AnthropicTier, Family};

/// Count and byte caps for images sent to one deployment. Byte values count base64 text,
/// not decoded PNG bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageLimits {
    /// Source images, archive frames and inline imagery together.
    pub total_images: u32,
    /// Base64 bytes archive frames may take in one request.
    pub archive_bytes: u64,
    /// Base64 bytes every image may take in one request.
    pub per_request_bytes: u64,
}

/// Image allowance of a host nothing is known about.
const UNKNOWN_IMAGES: u32 = 5;
/// Archive base64 bytes when the deployment publishes no byte budget.
const DEFAULT_ARCHIVE_BYTES: u64 = 3_000_000;
/// Official inline-image byte budget of the Anthropic and coding endpoints.
const OFFICIAL_REQUEST_BYTES: u64 = 24_000_000;

/// Base64 length of the universal decoded request ceiling.
fn universal_request_bytes() -> u64 {
    (super::MAX_REQUEST_ATTACHMENT_BYTES as u64)
        .div_ceil(3)
        .saturating_mul(4)
}

/// Official API host of a provider id, or `None` when the provider has no official
/// image budget.
fn official_host(provider: &str) -> Option<&'static str> {
    match provider {
        "anthropic" | "anthropic-messages" => Some("api.anthropic.com"),
        "openai" | "openai-responses" | "openai-completions" => Some("api.openai.com"),
        "openai-codex" | "codex" | "openai-codex-responses" => Some("chatgpt.com"),
        _ => None,
    }
}

/// Whether the request reaches the provider's official endpoint. `None` means the
/// provider's built-in endpoint.
fn reaches_official(provider: &str, endpoint_host: Option<&str>) -> bool {
    let Some(official) = official_host(provider) else {
        return false;
    };
    endpoint_host.is_none_or(|host| {
        let host = host.trim().to_ascii_lowercase();
        host == official || host.ends_with(&format!(".{official}"))
    })
}

fn is_claude(model: &str) -> bool {
    matches!(
        classify("", model).family,
        Family::Anthropic { tier } if tier != AnthropicTier::Unknown
    ) || model.to_ascii_lowercase().contains("claude")
}

/// Whether an Anthropic model predates the 600-image official allowance: revision below
/// 4.6, or a collapsed two-digit revision from 10 through 45.
fn anthropic_legacy_count(model: &str) -> bool {
    classify("anthropic", model)
        .revision
        .is_some_and(|(major, minor, _)| {
            (10..46).contains(&major) || major < 4 || (major == 4 && minor < 6)
        })
}

/// Longest image edge a host accepts when it is smaller than the model's own, so archive
/// frames are sized to it. Keyed by provider id like the image allowances.
pub fn host_edge_limit(provider: &str) -> Option<u32> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "cursor" => Some(1024),
        _ => None,
    }
}

/// Image allowance and byte budgets of a deployment. `endpoint_host` is the host the
/// request will reach; `None` means the provider's built-in endpoint.
pub fn image_limits(provider: &str, model: &str, endpoint_host: Option<&str>) -> ImageLimits {
    let provider = provider.trim().to_ascii_lowercase();
    let official = reaches_official(&provider, endpoint_host);
    let total_images = match provider.as_str() {
        "anthropic" | "anthropic-messages" if official => {
            if anthropic_legacy_count(model) {
                100
            } else {
                600
            }
        }
        "anthropic" | "anthropic-messages" | "bedrock" | "amazon-bedrock" | "openrouter" => 90,
        "openai-codex" | "codex" | "openai-codex-responses" if official => 2000,
        "openai"
        | "openai-responses"
        | "openai-completions"
        | "openai-codex"
        | "codex"
        | "openai-codex-responses"
        | "google"
        | "google-gemini-cli"
        | "google-vertex" => 200,
        "cursor" | "google-antigravity" => {
            if is_claude(model) || model.eq_ignore_ascii_case("auto") {
                20
            } else {
                60
            }
        }
        "xai" | "xai-oauth" => 60,
        "umans" => 10,
        _ => UNKNOWN_IMAGES,
    };
    let (archive_bytes, per_request_bytes) = if official
        && matches!(
            provider.as_str(),
            "anthropic"
                | "anthropic-messages"
                | "openai-codex"
                | "codex"
                | "openai-codex-responses"
        ) {
        (OFFICIAL_REQUEST_BYTES / 2, OFFICIAL_REQUEST_BYTES)
    } else {
        (DEFAULT_ARCHIVE_BYTES, universal_request_bytes())
    };
    ImageLimits {
        total_images,
        archive_bytes,
        per_request_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_caps_follow_endpoint_and_model() {
        let case = |provider, model, host: Option<&str>| image_limits(provider, model, host);
        for (limits, images, archive) in [
            (case("anthropic", "claude-opus-5", None), 600, 12_000_000),
            (
                case("anthropic", "claude-opus-5", Some("proxy.example")),
                90,
                3_000_000,
            ),
            (case("anthropic", "claude-opus-4-5", None), 100, 12_000_000),
            (case("openai", "gpt-5.5", None), 200, 3_000_000),
            (case("openai-codex", "gpt-6-astra", None), 2000, 12_000_000),
            (case("cursor", "claude-opus-5", None), 20, 3_000_000),
            (case("cursor", "gpt-5.5", None), 60, 3_000_000),
            (case("unlisted", "model", None), 5, 3_000_000),
        ] {
            assert_eq!(
                (limits.total_images, limits.archive_bytes),
                (images, archive)
            );
        }
    }
}
