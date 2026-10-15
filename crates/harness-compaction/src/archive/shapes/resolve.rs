//! Model identity and deployment API to archive geometry and billing lineage.
use super::{lookup, variant_index, ArchiveShape, SILVER};
use harness_providers::attachment_protocol::host_edge_limit;
use harness_tokenizer::{classify, AnthropicTier, Family, ModelIdentity, ReaderApi};

/// The wire family of a provider or API name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Api {
    Anthropic,
    OpenAi,
    Google,
    /// Gateways and adapters whose images are billed like Anthropic's high tier.
    AnthropicBilled,
    Other,
}

/// Facts about the reader's deployment beyond the model's name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShapeHost {
    /// The wire the request will use, when the provider knows it. It decides the family of
    /// providers the catalog does not list.
    pub api: Option<ReaderApi>,
}

/// Wire family of a provider id or API name: exact catalog entries first, then the wire
/// the provider reported, else unknown.
fn api_of(provider: &str, host: ShapeHost) -> Api {
    match provider.trim().to_ascii_lowercase().as_str() {
        "anthropic"
        | "anthropic-messages"
        | "bedrock"
        | "amazon-bedrock"
        | "bedrock-converse-stream" => Api::Anthropic,
        "openai"
        | "openai-completions"
        | "openai-responses"
        | "openai-codex"
        | "openai-codex-responses"
        | "codex"
        | "azure"
        | "azure-openai-responses" => Api::OpenAi,
        "google"
        | "google-generative-ai"
        | "google-gemini-cli"
        | "google-vertex"
        | "vertex"
        | "google-antigravity" => Api::Google,
        "openrouter" | "ollama" | "cursor" | "apple" | "deployment-agent" => Api::AnthropicBilled,
        _ => match host.api {
            Some(ReaderApi::Anthropic) => Api::Anthropic,
            Some(ReaderApi::OpenAiChat | ReaderApi::OpenAiResponses) => Api::OpenAi,
            _ => Api::Other,
        },
    }
}

/// Revision compared structurally; a collapsed two-digit revision (`47` for 4.7) expands.
fn revision(identity: &ModelIdentity) -> Option<(u32, u32)> {
    identity.revision.map(|(major, minor, _)| {
        if major >= 10 && minor == 0 {
            (major / 10, major % 10)
        } else {
            (major, minor)
        }
    })
}

/// Cell of the shape table a model lands on before any explicit variant.
struct Ideal {
    variant: usize,
    /// Index into the edge table: 0 own, 1 high-resolution, 2 Gemini.
    edge: usize,
}

fn claude_high_text(id: &str) -> bool {
    id.find("claude")
        .is_some_and(|at| id[at..].contains("fable") || id[at..].contains("mythos"))
}

fn is_high(identity: &ModelIdentity, id: &str) -> bool {
    let rev = revision(identity);
    match identity.family {
        Family::Anthropic { tier } => {
            matches!(tier, AnthropicTier::Fable | AnthropicTier::Mythos)
                || (tier == AnthropicTier::Opus && rev.is_some_and(|r| r >= (4, 7)))
                || claude_high_text(id)
        }
        _ => claude_high_text(id),
    }
}

fn ideal(identity: &ModelIdentity, id: &str, api: Api) -> Ideal {
    let has = |needle: &str| id.contains(needle);
    let (variant, edge) = if is_high(identity, id) {
        (12, 1)
    } else if matches!(identity.family, Family::Anthropic { .. }) || has("claude") {
        (12, 0)
    } else if identity.family == Family::Gemini || has("gemini") {
        (11, 2)
    } else if identity.family == Family::OpenAi || has("gpt") || has("codex") {
        (11, 0)
    } else if identity.family == Family::Kimi || has("kimi") {
        (11, 0)
    } else if identity.family == Family::Glm || has("glm") {
        (10, 0)
    } else if api == Api::Anthropic {
        (12, 0)
    } else {
        (11, 0)
    };
    Ideal { variant, edge }
}

/// Billing lineage (index into the pricing table) and whether the rule is verified.
fn billing(identity: &ModelIdentity, id: &str, api: Api) -> (usize, bool) {
    let rev = revision(identity);
    match identity.family {
        Family::Anthropic { tier } => {
            let older = matches!(
                tier,
                AnthropicTier::Opus | AnthropicTier::Sonnet | AnthropicTier::Haiku
            ) && rev.is_some_and(|r| r < (4, 7));
            if older {
                (2, tier == AnthropicTier::Sonnet && rev == Some((4, 6)))
            } else {
                (1, is_high(identity, id) || rev.is_some_and(|r| r >= (4, 7)))
            }
        }
        Family::Gemini => (3, false),
        Family::OpenAi => {
            let r = rev.unwrap_or((0, 0));
            let pricing = if r >= (5, 6) {
                6
            } else if r >= (5, 4) {
                4
            } else {
                5
            };
            (
                pricing,
                ((5, 5)..(6, 2)).contains(&r) || id.contains("gpt-6-astra"),
            )
        }
        Family::Xai => (7, rev.is_some_and(|r| ((4, 6)..=(4, 20)).contains(&r))),
        _ => match api {
            Api::Anthropic | Api::AnthropicBilled => (1, false),
            Api::Google => (3, false),
            Api::OpenAi => (4, false),
            Api::Other => (0, false),
        },
    }
}

/// Whether `explicit` names a concrete variant that automatic selection must not change.
pub(in crate::archive) fn is_explicit(explicit: Option<&str>) -> bool {
    explicit
        .filter(|v| *v != "auto")
        .is_some_and(|v| variant_index(v).is_some())
}

/// Model lineage takes priority over transport; an explicit variant retains its geometry.
pub fn resolve_shape(
    model_id: &str,
    provider_family: &str,
    explicit: Option<&str>,
) -> &'static ArchiveShape {
    resolve_shape_on(model_id, provider_family, explicit, ShapeHost::default())
}

/// [`resolve_shape`] with the deployment facts the provider reported.
pub fn resolve_shape_on(
    model_id: &str,
    provider_family: &str,
    explicit: Option<&str>,
    host: ShapeHost,
) -> &'static ArchiveShape {
    resolve(model_id, provider_family, explicit, false, host)
}

/// The shape automatic selection picks for text Silver renders better: Silver geometry
/// with the model's own edge and billing lineage.
pub(in crate::archive) fn resolve_silver(
    model_id: &str,
    provider_family: &str,
    host: ShapeHost,
) -> &'static ArchiveShape {
    resolve(model_id, provider_family, None, true, host)
}

fn resolve(
    model_id: &str,
    provider_family: &str,
    explicit: Option<&str>,
    silver: bool,
    host: ShapeHost,
) -> &'static ArchiveShape {
    let id = model_id.to_ascii_lowercase();
    let api = api_of(provider_family, host);
    let identity = classify(provider_family, model_id);
    let ideal = ideal(&identity, &id, api);
    let selected = explicit
        .filter(|v| *v != "auto" && !silver)
        .and_then(variant_index);
    let variant = if silver {
        SILVER
    } else {
        selected.unwrap_or(ideal.variant)
    };
    // A host cannot take larger frames than its own edge limit (the 1024 table slot).
    let edge = if host_edge_limit(provider_family) == Some(1024) {
        3
    } else if selected.is_some() {
        0
    } else {
        ideal.edge
    };
    let (pricing, verified) = billing(&identity, &id, api);
    lookup(variant, edge, pricing, verified, api == Api::OpenAi)
}
