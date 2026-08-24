include!("common/transcript_projection_fixtures.rs");

mod part_01_projects_user_assistant_text_and_reasoning_test {
    use super::*;
    include!("transcript_projection/01_projects_user_assistant_text_and_reasoning_test.rs");
}

mod part_02_projects_task_lineage_and_child_session_test {
    use super::*;
    include!("transcript_projection/02_projects_task_lineage_and_child_session_test.rs");
}

mod part_03_semantic_assistant_commit_test {
    use super::*;
    include!("transcript_projection/03_semantic_assistant_commit_test.rs");
}
