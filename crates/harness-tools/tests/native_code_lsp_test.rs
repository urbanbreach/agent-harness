include!("common/native_code_lsp_fixtures.rs");

mod part_01_native_code_lsp_supports_configured_custom_test {
    use super::*;
    include!("native_code_lsp/01_native_code_lsp_supports_configured_custom_test.rs");
}

mod part_02_native_code_lsp_supports_direct_file_test {
    use super::*;
    include!("native_code_lsp/02_native_code_lsp_supports_direct_file_test.rs");
}

mod part_03_native_code_lsp_rename_previews_and_test {
    use super::*;
    include!("native_code_lsp/03_native_code_lsp_rename_previews_and_test.rs");
}

mod part_04_native_code_lsp_install_decision_test {
    use super::*;
    include!("native_code_lsp/04_native_code_lsp_install_decision_test.rs");
}
