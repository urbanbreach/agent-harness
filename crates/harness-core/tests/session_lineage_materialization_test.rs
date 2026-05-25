include!("common/session_lineage_materialization_fixtures.rs");

mod part_01_session_lineage_materializes_child_atomically_test {
    use super::*;
    include!(
        "session_lineage_materialization/01_session_lineage_materializes_child_atomically_test.rs"
    );
}
