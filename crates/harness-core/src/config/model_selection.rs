use super::*;

fn parse_model_ref(model_ref: &str) -> Option<(&str, &str)> {
    let (provider_name, model_name) = model_ref
        .split_once(':')
        .or_else(|| model_ref.split_once('/'))?;
    if provider_name.is_empty() || model_name.is_empty() {
        return None;
    }
    Some((provider_name, model_name))
}

fn is_direct_model_ref(model_ref: &str) -> bool {
    model_ref.contains(':') || model_ref.contains('/')
}

fn normalize_model_ref(provider: &str, model: &str) -> String {
    format!("{provider}:{model}")
}

pub fn resolve_model_selection(
    cfg: &HarnessConfig,
    selector: &str,
    variant_override: Option<&str>,
) -> Result<ResolvedModelSelection, ConfigError> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err(ConfigError::InvalidReference(
            "model selector must not be empty; use `<provider>:<model>` or a configured `model_profile` name"
                .to_string(),
        ));
    }

    if is_direct_model_ref(selector) {
        return resolve_direct_model_target(cfg, selector, variant_override, "model selector").map(
            |primary| ResolvedModelSelection {
                selector: selector.to_string(),
                profile: None,
                primary,
                fallback: Vec::new(),
            },
        );
    }

    if cfg.model_profiles.contains_key(selector) {
        return resolve_named_model_profile(cfg, selector, variant_override);
    }

    Err(ConfigError::InvalidReference(format!(
        "unknown model profile `{selector}`; unqualified model selectors must match `model_profile` names; available profiles: {}",
        format_name_list(cfg.model_profiles.keys().map(|name| name.as_str()))
    )))
}

pub(super) fn resolve_agent_model_selection(
    cfg: &HarnessConfig,
    agent_name: &str,
    agent: &ProfileConfig,
) -> Result<ResolvedModelSelection, ConfigError> {
    resolve_model_selection(cfg, &agent.model_ref, agent.variant.as_deref()).map_err(|err| {
        ConfigError::InvalidReference(format!(
            "agent `{agent_name}` has invalid model selection `{}`: {err}",
            agent.model_ref
        ))
    })
}

pub(super) fn resolve_named_model_profile(
    cfg: &HarnessConfig,
    profile_name: &str,
    variant_override: Option<&str>,
) -> Result<ResolvedModelSelection, ConfigError> {
    let profile = cfg.model_profiles.get(profile_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "unknown model profile `{profile_name}`; available profiles: {}",
            format_name_list(cfg.model_profiles.keys().map(|name| name.as_str()))
        ))
    })?;

    let primary = resolve_model_profile_target(
        cfg,
        &ModelProfileTargetConfig {
            model: profile.model.clone(),
            variant: profile.variant.clone(),
        },
        variant_override,
        &format!("model_profile `{profile_name}`"),
    )?;
    let fallback = profile
        .fallback
        .iter()
        .enumerate()
        .map(|(index, target)| {
            resolve_model_profile_target(
                cfg,
                target,
                None,
                &format!("model_profile `{profile_name}` fallback[{index}]"),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ResolvedModelSelection {
        selector: profile_name.to_string(),
        profile: Some(profile_name.to_string()),
        primary,
        fallback,
    })
}

fn resolve_model_profile_target(
    cfg: &HarnessConfig,
    target: &ModelProfileTargetConfig,
    variant_override: Option<&str>,
    context: &str,
) -> Result<ResolvedModelTarget, ConfigError> {
    if !is_direct_model_ref(&target.model) {
        return Err(ConfigError::InvalidReference(format!(
            "{context} references `{}`; model profile targets must use direct refs like `<provider>:<model>` or `<provider>/<model>`",
            target.model
        )));
    }

    resolve_direct_model_target(
        cfg,
        &target.model,
        variant_override.or(target.variant.as_deref()),
        context,
    )
}

fn resolve_direct_model_target(
    cfg: &HarnessConfig,
    model_ref: &str,
    variant_name: Option<&str>,
    context: &str,
) -> Result<ResolvedModelTarget, ConfigError> {
    let Some((provider_name, model_name)) = parse_model_ref(model_ref) else {
        return Err(ConfigError::InvalidReference(format!(
            "{context} has invalid model ref `{model_ref}`; use `<provider>:<model>` or `<provider>/<model>`"
        )));
    };

    let resolved = resolve_configured_model_metadata(cfg, provider_name, model_name, variant_name)
        .map_err(|err| ConfigError::InvalidReference(format!("{context}: {err}")))?;
    Ok(ResolvedModelTarget {
        model_ref: normalize_model_ref(&resolved.provider, &resolved.model),
        provider: resolved.provider,
        model: resolved.model,
        variant: resolved.variant,
        reasoning_effort: resolved.reasoning_effort.clone(),
        text_verbosity: resolved.text_verbosity,
        reasoning_summary: if resolved
            .resolution
            .capabilities
            .supports_reasoning_summaries
            && resolved.reasoning_effort.is_some()
        {
            Some("auto".to_string())
        } else {
            None
        },
        thinking: resolved.thinking,
        resolution: resolved.resolution,
    })
}
