include!("common/coord_fixtures.rs");

mod part_01_coord_title_generation_uses_isolated_hidden_test {
    use super::*;
    include!("coord/01_coord_title_generation_uses_isolated_hidden_test.rs");
}

mod part_02_coordinator_runs_parallel_child_sessions_under_test {
    use super::*;
    include!("coord/02_coordinator_runs_parallel_child_sessions_under_test.rs");
}

mod part_03_same_agent_turn_queues_even_when_test {
    use super::*;
    include!("coord/03_same_agent_turn_queues_even_when_test.rs");
}

mod part_04_running_agent_turn_cancellation_emits_single_test {
    use super::*;
    include!("coord/04_running_agent_turn_cancellation_emits_single_test.rs");
}

mod part_05_completed_tool_turn_preserves_tool_messages_test {
    use super::*;
    include!("coord/05_completed_tool_turn_preserves_tool_messages_test.rs");
}

mod part_06_no_tool_turn_appends_explicit_phase_test {
    use super::*;
    include!("coord/06_no_tool_turn_appends_explicit_phase_test.rs");
}

mod part_07_tool_results_project_in_assistant_source_test {
    use super::*;
    include!("coord/07_tool_results_project_in_assistant_source_test.rs");
}

mod part_08_cancelling_turn_waiting_for_permission_emits_test {
    use super::*;
    include!("coord/08_cancelling_turn_waiting_for_permission_emits_test.rs");
}

mod part_09_failed_turn_context_preserves_provider_error_test {
    use super::*;
    include!("coord/09_failed_turn_context_preserves_provider_error_test.rs");
}

mod part_10_aborted_response_compaction_preserves_abort_marker_test {
    use super::*;
    include!("coord/10_aborted_response_compaction_preserves_abort_marker_test.rs");
}

mod part_11_resume_existing_run_restores_sequence_and_test {
    use super::*;
    include!("coord/11_resume_existing_run_restores_sequence_and_test.rs");
}

mod part_12_resume_existing_run_persists_bindings_for_test {
    use super::*;
    include!("coord/12_resume_existing_run_persists_bindings_for_test.rs");
}

mod part_12b_resume_acceptance_restores_realistic_interrupted_session_test {
    use super::*;
    include!("coord/12b_resume_acceptance_restores_realistic_interrupted_session_test.rs");
}

mod part_13_overflow_retry_compacts_context_and_retries_test {
    use super::*;
    include!("coord/13_overflow_retry_compacts_context_and_retries_test.rs");
}

mod part_14_compaction_trigger_pre_prompt_runtime_uses_test {
    use super::*;
    include!("coord/14_compaction_trigger_pre_prompt_runtime_uses_test.rs");
}

mod part_15_manual_compaction_after_four_small_turns_test {
    use super::*;
    include!("coord/15_manual_compaction_after_four_small_turns_test.rs");
}

mod part_16_model_backed_overflow_split_uses_model_test {
    use super::*;
    include!("coord/16_model_backed_overflow_split_uses_model_test.rs");
}

mod part_17_tool_task_lifecycle_events_preserve_owner_test {
    use super::*;
    include!("coord/17_tool_task_lifecycle_events_preserve_owner_test.rs");
}

mod part_18_lifecycle_hooks_cover_provider_subagent_and_test {
    use super::*;
    include!("coord/18_lifecycle_hooks_cover_provider_subagent_and_test.rs");
}

mod part_19_replay_suppresses_hooks_but_preserves_hook_test {
    use super::*;
    include!("coord/19_replay_suppresses_hooks_but_preserves_hook_test.rs");
}

mod part_20_provider_cache_context_uses_run_session_test {
    use super::*;
    include!("coord/20_provider_cache_context_uses_run_session_test.rs");
}
