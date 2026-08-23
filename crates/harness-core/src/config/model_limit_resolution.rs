use super::{
    MaxInputSemantics, ModelConfig, ModelLimitProvenance, ModelVariantConfig, ResolvedModelLimit,
    ResolvedModelLimits,
};

pub(super) struct ModelLimitIdentity<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub variant: Option<&'a str>,
}

pub(super) fn resolve_model_limits(
    identity: ModelLimitIdentity<'_>,
    model: &ModelConfig,
    variant: Option<&ModelVariantConfig>,
) -> ResolvedModelLimits {
    let model_detail = format!("model `{}:{}`", identity.provider, identity.model);
    let variant_detail = identity
        .variant
        .map(|name| format!("{model_detail} variant `{name}`"))
        .unwrap_or_else(|| model_detail.clone());
    let base = &model.limit_provenance;

    let context = variant
        .and_then(|config| config.context_window_tokens.or(config.limit.context))
        .map(|tokens| {
            (
                tokens,
                ModelLimitProvenance::explicit(variant_detail.clone()),
            )
        })
        .or_else(|| {
            model
                .metadata
                .context_window_tokens
                .or(model.limit.context)
                .map(|tokens| (tokens, base.clone()))
        });
    let input = variant
        .and_then(|config| config.max_input_tokens.or(config.limit.input))
        .map(|tokens| {
            (
                tokens,
                ModelLimitProvenance::explicit(variant_detail.clone()),
            )
        })
        .or_else(|| {
            model
                .max_input_tokens
                .or(model.limit.input)
                .map(|tokens| (tokens, base.clone()))
        });
    let output = variant
        .and_then(|config| config.max_output_tokens.or(config.limit.output))
        .map(|tokens| (tokens, ModelLimitProvenance::explicit(variant_detail)))
        .or_else(|| {
            model
                .max_output_tokens
                .or(model.limit.output)
                .map(|tokens| (tokens, base.clone()))
        });

    ResolvedModelLimits {
        context_window: resolved_field(context, &model_detail, "context window"),
        max_input: resolved_field(input, &model_detail, "max input"),
        max_output: resolved_field(output, &model_detail, "max output"),
        max_input_semantics: MaxInputSemantics::ProviderVisibleInputTokens,
    }
}

fn resolved_field(
    resolved: Option<(u32, ModelLimitProvenance)>,
    model_detail: &str,
    field: &str,
) -> ResolvedModelLimit {
    match resolved {
        Some((tokens, provenance)) => ResolvedModelLimit {
            tokens: Some(tokens),
            provenance,
        },
        None => ResolvedModelLimit {
            tokens: None,
            provenance: ModelLimitProvenance::unknown(format!(
                "{model_detail} has no authoritative {field}"
            )),
        },
    }
}
