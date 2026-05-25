include!("common/native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_fixtures.rs");

mod part_01_foreground_task_waits_for_child_agent_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/01_foreground_task_waits_for_child_agent_test.rs");
}

mod part_02_native_plan_enter_decline_leaves_build_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/02_native_plan_enter_decline_leaves_build_test.rs");
}

mod part_03_background_output_block_waits_for_running_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/03_background_output_block_waits_for_running_test.rs");
}

mod part_04_native_batch_and_agent_spawn_preserve_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/04_native_batch_and_agent_spawn_preserve_test.rs");
}

mod part_05_task_tool_rejects_missing_loaded_skill_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/05_task_tool_rejects_missing_loaded_skill_test.rs");
}

mod part_06_task_tool_rejects_reentry_to_sibling_test {
    use super::*;
    include!("native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/06_task_tool_rejects_reentry_to_sibling_test.rs");
}
