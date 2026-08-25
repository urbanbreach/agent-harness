include!("common/resume_plan_fixtures.rs");

mod part_01_resume_plan_reconstructs_sequence_and_id_test {
    use super::*;
    include!("resume_plan/01_resume_plan_reconstructs_sequence_and_id_test.rs");
}

mod part_02_resume_plan_resolves_tool_identity_and_test {
    use super::*;
    include!("resume_plan/02_resume_plan_resolves_tool_identity_and_test.rs");
}

mod part_03_resume_plan_uses_latest_lifecycle_segment_test {
    use super::*;
    include!("resume_plan/03_resume_plan_uses_latest_lifecycle_segment_test.rs");
}

mod part_04_canonical_tool_pairing_test {
    include!("resume_plan/04_canonical_tool_pairing_test.rs");
}

mod part_05_semantic_history_restart_test {
    use super::*;
    include!("resume_plan/05_semantic_history_restart_test.rs");
}

mod part_06_canonical_provider_view_resume_test {
    include!("resume_plan/06_canonical_provider_view_resume_test.rs");
}

mod part_07_canonical_resume_recovery_test {
    use super::*;
    include!("resume_plan/07_canonical_resume_recovery_test.rs");
}
