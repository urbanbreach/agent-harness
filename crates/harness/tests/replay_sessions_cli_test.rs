include!("common/replay_sessions_cli_fixtures.rs");

mod part_01_replay_cli_prints_json_summary_test {
    use super::*;
    include!("replay_sessions_cli/01_replay_cli_prints_json_summary_test.rs");
}

mod part_02_replay_cli_merges_on_disk_artifact_test {
    use super::*;
    include!("replay_sessions_cli/02_replay_cli_merges_on_disk_artifact_test.rs");
}

mod part_03_session_history_entries_sort_by_recency_test {
    use super::*;
    include!("replay_sessions_cli/03_session_history_entries_sort_by_recency_test.rs");
}

mod part_04_sessions_list_cli_prints_json_entries_test {
    use super::*;
    include!("replay_sessions_cli/04_sessions_list_cli_prints_json_entries_test.rs");
}

mod part_05_sessions_list_cli_filters_machine_readable_test {
    use super::*;
    include!("replay_sessions_cli/05_sessions_list_cli_filters_machine_readable_test.rs");
}

mod part_06_sessions_fork_clone_test {
    use super::*;
    include!("replay_sessions_cli/06_sessions_fork_clone_test.rs");
}

mod part_07_sessions_fork_rejects_invalid_cutoff_test {
    use super::*;
    include!("replay_sessions_cli/07_sessions_fork_rejects_invalid_cutoff_test.rs");
}

mod part_08_sessions_export_test {
    use super::*;
    include!("replay_sessions_cli/08_sessions_export_test.rs");
}

mod part_09_sessions_export_support_scan_test {
    use super::*;
    include!("replay_sessions_cli/09_sessions_export_support_scan_test.rs");
}
