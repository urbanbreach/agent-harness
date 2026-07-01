include!("common/native_execution_surface_fixtures.rs");

mod part_01_native_execution_surface_tools_execute_through_test {
    use super::*;
    include!("native_execution_surface/01_native_execution_surface_tools_execute_through_test.rs");
}

mod part_02_native_public_edit_accepts_unique_hash_test {
    use super::*;
    include!("native_execution_surface/02_native_public_edit_accepts_unique_hash_test.rs");
}

mod part_03_native_write_apply_patch_exact_edit_test {
    use super::*;
    include!("native_execution_surface/03_native_write_apply_patch_exact_edit_test.rs");
}

mod part_04_native_apply_patch_preflight_test {
    use super::*;
    include!("native_execution_surface/04_native_apply_patch_preflight_test.rs");
}

mod part_05_native_baseline_shape_compat_test {
    use super::*;
    include!("native_execution_surface/05_native_baseline_shape_compat_test.rs");
}

mod part_06_native_provider_tool_defs_schema_test {
    use super::*;
    include!("native_execution_surface/06_native_provider_tool_defs_schema_test.rs");
}
