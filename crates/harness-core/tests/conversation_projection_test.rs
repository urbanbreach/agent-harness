include!("common/conversation_projection_fixtures.rs");

mod part_01_provider_boundary_preserves_existing_message_shape_test {
    use super::*;
    include!(
        "conversation_projection/01_provider_boundary_preserves_existing_message_shape_test.rs"
    );
}

mod part_02_provider_boundary_sanitizes_unknown_historical_tool_test {
    use super::*;
    include!(
        "conversation_projection/02_provider_boundary_sanitizes_unknown_historical_tool_test.rs"
    );
}
