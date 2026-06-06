include!("common/skill_load_discovery_fixtures.rs");

mod part_01_skill_load_discovers_project_and_global_test {
    use super::*;
    include!("skill_load_discovery/01_skill_load_discovers_project_and_global_test.rs");
}

mod part_01b_skill_load_workspace_and_policy_test {
    use super::*;
    include!("skill_load_discovery/01b_skill_load_workspace_and_policy_test.rs");
}

mod part_02_skill_load_uses_registered_custom_roots_test {
    use super::*;
    include!("skill_load_discovery/02_skill_load_uses_registered_custom_roots_test.rs");
}

mod part_03_v1_skill_contract_test {
    use super::*;
    include!("skill_load_discovery/03_v1_skill_contract_test.rs");
}

mod part_03b_v1_shipped_skill_contract_test {
    use super::*;
    include!("skill_load_discovery/03b_v1_shipped_skill_contract_test.rs");
}
