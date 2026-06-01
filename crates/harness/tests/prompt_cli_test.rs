include!("common/prompt_cli_fixtures.rs");

mod part_01_prompt_cli_calls_responses_endpoint_test {
    use super::*;
    include!("prompt_cli/01_prompt_cli_calls_responses_endpoint_test.rs");
}

mod part_02_prompt_cli_model_variant_and_thinking_test {
    use super::*;
    include!("prompt_cli/02_prompt_cli_model_variant_and_thinking_test.rs");
}

mod part_03_prompt_cli_routes_non_default_profile_test {
    use super::*;
    include!("prompt_cli/03_prompt_cli_routes_non_default_profile_test.rs");
}

mod part_04_prompt_cli_executes_fs_grep_and_test {
    use super::*;
    include!("prompt_cli/04_prompt_cli_executes_fs_grep_and_test.rs");
}

mod part_05_run_cli_prompt_parity_test {
    use super::*;
    include!("prompt_cli/05_run_cli_prompt_parity_test.rs");
}
