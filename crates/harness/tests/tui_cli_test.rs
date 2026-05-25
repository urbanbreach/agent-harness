include!("common/tui_cli_fixtures.rs");

mod part_01_tui_cli_help_does_not_expose_test {
    use super::*;
    include!("tui_cli/01_tui_cli_help_does_not_expose_test.rs");
}

mod part_02_replay_bootstrap_falls_back_when_recorded_test {
    use super::*;
    include!("tui_cli/02_replay_bootstrap_falls_back_when_recorded_test.rs");
}

mod part_03_tui_cli_invalid_config_fails_without_test {
    use super::*;
    include!("tui_cli/03_tui_cli_invalid_config_fails_without_test.rs");
}
