include!("common/config_schema_cli_fixtures.rs");

mod part_01_models_probe_generates_harness_catalog_fragment_test {
    use super::*;
    include!("config_schema_cli/01_models_probe_generates_harness_catalog_fragment_test.rs");
}

mod part_02_doctor_cli_reports_shipped_orchestration_health_test {
    use super::*;
    include!("config_schema_cli/02_doctor_cli_reports_shipped_orchestration_health_test.rs");
}

mod part_02b_doctor_cli_reports_native_tool_catalog_readiness_test {
    use super::*;
    include!("config_schema_cli/02b_doctor_cli_reports_native_tool_catalog_readiness_test.rs");
}

mod part_02c_config_validate_and_provider_credentials_test {
    use super::*;
    include!("config_schema_cli/02c_config_validate_and_provider_credentials_test.rs");
}

mod part_02d_auth_credentials_cli_test {
    use super::*;
    include!("config_schema_cli/02d_auth_credentials_cli_test.rs");
}

mod part_02e_config_validate_discovery_test {
    use super::*;
    include!("config_schema_cli/02e_config_validate_discovery_test.rs");
}

mod part_02f_doctor_cli_reports_formatter_status_test {
    use super::*;
    include!("config_schema_cli/02f_doctor_cli_reports_formatter_status_test.rs");
}

mod part_03_config_validate_cli_loads_separate_tui_test {
    use super::*;
    include!("config_schema_cli/03_config_validate_cli_loads_separate_tui_test.rs");
}

mod part_02g_doctor_cli_redacts_provider_credentials_test {
    use super::*;
    include!("config_schema_cli/02g_doctor_cli_redacts_provider_credentials_test.rs");
}

mod part_04_public_runtime_config_accepts_compaction_settings_test {
    use super::*;
    include!("config_schema_cli/04_public_runtime_config_accepts_compaction_settings_test.rs");
}

mod part_05_cli_surface_audit_test {
    use super::*;
    include!("config_schema_cli/05_cli_surface_audit_test.rs");
}
