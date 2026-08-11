use super::*;
use crate::UnwrapOrAbort;

#[test]
fn legacy_provider_name_and_options_normalize_to_runtime_shape() {
    // Given
    let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              name: "CLIProxyAPI",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key"
              },
              models: {
                "gpt-4o-mini": { name: "GPT-4o mini" }
              }
            }
          },
          model: "default/gpt-4o-mini",
          permission: "allow"
        }
        "#;

    // When
    let parsed = load_config_from_str(cfg).unwrap_or_abort();

    // Then
    let ProviderConfig::OpenAiCompatible(provider) = &parsed.providers["default"] else {
        panic!("expected OpenAiCompatible");
    };
    assert_eq!(provider.name.as_deref(), Some("CLIProxyAPI"));
    assert_eq!(provider.base_url, "http://127.0.0.1:8317/v1");
    assert_eq!(provider.api_key, "test-key");
    assert_eq!(provider.models["gpt-4o-mini"].display_name, "GPT-4o mini");
    let metadata = resolve_profile_model_metadata(&parsed, "default").unwrap_or_abort();
    assert_eq!(metadata.provider_display_label, "CLIProxyAPI");
}
