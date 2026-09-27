use harness_tui::transcript_blocks::{RawDisclosure, RawPayload};

#[test]
fn raw_disclosure_redacts_provider_and_auth_secrets_without_dropping_shape() {
    // arrange
    let source = serde_json::json!({
        "model": "gpt",
        "api_key": "sk-1234567890abcdefghij",
        "authorization": "Bearer live-token-1234567890",
        "nested": [
            {"ok": true, "value": "AIza12345678901234567890"},
            null
        ]
    });

    let disclosure = RawDisclosure::from_json(&source);
    assert!(matches!(disclosure.payload, RawPayload::Json(_)));
    let redacted = match &disclosure.payload {
        RawPayload::Json(value) => value,
        RawPayload::Text(_) => return,
    };

    // act
    assert_eq!(redacted["model"], "gpt");
    assert_eq!(redacted["nested"][0]["ok"], true);
    assert_eq!(redacted["nested"].as_array().map(Vec::len), Some(2));
    assert_eq!(redacted["api_key"], "<redacted>");
    assert_eq!(redacted["authorization"], "Bearer <redacted>");
    assert_eq!(redacted["nested"][0]["value"], "<redacted>");
    assert!(!redacted.to_string().contains("sk-1234567890abcdefghij"));
    assert!(!redacted.to_string().contains("live-token-1234567890"));
    let text = RawDisclosure::from_text("Authorization: Bearer live-token-1234567890");
    assert_eq!(
        &text.payload,
        &RawPayload::Text("Authorization: Bearer <redacted>".to_string())
    );

    // assert
    insta::assert_json_snapshot!(redacted, @r###"
    {
      "api_key": "<redacted>",
      "authorization": "Bearer <redacted>",
      "model": "gpt",
      "nested": [
        {
          "ok": true,
          "value": "<redacted>"
        },
        null
      ]
    }
    "###);
}
