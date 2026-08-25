include!("common/session_lineage_materialization_fixtures.rs");

mod part_01_session_lineage_materializes_child_atomically_test {
    use super::*;
    include!(
        "session_lineage_materialization/01_session_lineage_materializes_child_atomically_test.rs"
    );
}

mod part_02_canonical_root_child_isolation_test {
    include!("session_lineage_materialization/02_canonical_root_child_isolation_test.rs");
}

mod part_03_provider_view_selected_branch_test {
    include!("session_lineage_materialization/03_provider_view_selected_branch_test.rs");
}
