pub(crate) fn provider_model_label(provider: Option<&str>, model: Option<&str>) -> Option<String> {
    match (provider, model) {
        (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
        (Some(provider), None) => Some(format!("{provider}/<unavailable>")),
        (None, Some(model)) => Some(format!("<unavailable>/{model}")),
        (None, None) => None,
    }
}
