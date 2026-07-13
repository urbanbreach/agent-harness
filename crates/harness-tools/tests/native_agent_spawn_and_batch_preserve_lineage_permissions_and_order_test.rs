include!("common/native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_fixtures.rs");

mod part_01_foreground_task_waits_for_child_agent_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/01_foreground_task_waits_for_child_agent_test.rs");
}

mod part_02_native_plan_enter_decline_leaves_build_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/02_native_plan_enter_decline_leaves_build_test.rs");
}

mod part_02b_background_output_completed_child_result_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/02b_background_output_completed_child_result_test.rs");
}

mod part_03_background_output_block_waits_for_running_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/03_background_output_block_waits_for_running_test.rs");
}

mod part_03b_child_agent_toolset_boundary_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/03b_child_agent_toolset_boundary_test.rs");
}

mod part_04_native_batch_and_agent_spawn_preserve_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/04_native_batch_and_agent_spawn_preserve_test.rs");
}

mod part_04b_batch_display_guidance_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/04b_batch_display_guidance_test.rs");
}

mod part_05_task_tool_rejects_missing_loaded_skill_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/05_task_tool_rejects_missing_loaded_skill_test.rs");
}

mod part_05b_task_tool_reentry_by_task_id_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/05b_task_tool_reentry_by_task_id_test.rs");
}

mod part_05c_batch_alias_and_task_reentry_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/05c_batch_alias_and_task_reentry_test.rs");
}

mod part_06_task_tool_rejects_reentry_to_sibling_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/06_task_tool_rejects_reentry_to_sibling_test.rs");
}

mod part_07_task_delegation_contract_fixture_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/07_task_delegation_contract_fixture_test.rs");
}

mod part_08_background_output_full_session_and_thinking_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/08_background_output_full_session_and_thinking_test.rs");
}

mod part_09_background_cancel_all_bulk_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/09_background_cancel_all_bulk_test.rs");
}
