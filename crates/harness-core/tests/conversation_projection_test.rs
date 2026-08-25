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

mod part_03_canonical_session_contract_test {
    include!("conversation_projection/03_canonical_session_contract_test.rs");
}

mod part_04_canonical_session_behavior_test {
    include!("conversation_projection/04_canonical_session_behavior_test.rs");
}

mod part_05_semantic_assistant_commit_test {
    use super::*;
    include!("conversation_projection/05_semantic_assistant_commit_test.rs");
}

mod part_06_compaction_v2_protocol_test {
    use super::*;
    include!("conversation_projection/06_compaction_v2_protocol_test.rs");
}
