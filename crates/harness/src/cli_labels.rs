pub(crate) fn provider_model_label(provider: Option<&str>, model: Option<&str>) -> Option<String> {
    provider.or(model).map(|_| {
        format!(
            "{}/{}",
            provider.unwrap_or("<unavailable>"),
            model.unwrap_or("<unavailable>")
        )
    })
}
