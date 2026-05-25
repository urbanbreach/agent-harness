include!("common/config_schema_cli_fixtures.rs");

mod part_01_models_probe_generates_harness_catalog_fragment_test {
    use super::*;
    include!("config_schema_cli/01_models_probe_generates_harness_catalog_fragment_test.rs");
}

mod part_02_doctor_cli_reports_shipped_orchestration_health_test {
    use super::*;
    include!("config_schema_cli/02_doctor_cli_reports_shipped_orchestration_health_test.rs");
}

mod part_03_config_validate_cli_loads_separate_tui_test {
    use super::*;
    include!("config_schema_cli/03_config_validate_cli_loads_separate_tui_test.rs");
}

mod part_04_public_runtime_config_accepts_compaction_settings_test {
    use super::*;
    include!("config_schema_cli/04_public_runtime_config_accepts_compaction_settings_test.rs");
}
