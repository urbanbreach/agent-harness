use serde_json::Value;

pub(crate) fn provider_tool_arguments_json(args_summary: &str) -> String {
    if serde_json::from_str::<Value>(args_summary).is_ok() {
        args_summary.to_string()
    } else {
        "{}".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::provider_tool_arguments_json;

    #[test]
    fn provider_tool_arguments_preserves_valid_json_and_falls_back_for_text() {
        // arrange
        // act
        // assert
        assert_eq!(
            provider_tool_arguments_json(r#"{"path":"Cargo.toml"}"#),
            r#"{"path":"Cargo.toml"}"#
        );
        assert_eq!(provider_tool_arguments_json("read Cargo.toml"), "{}");
    }
}
