mod support {
    use super::*;
    include!("30c_compaction_v2_permission_denial_test/support_test.rs");
}

use support::{
    assert_one_denied_pair, capture_denial_scenario, only_assistant_tool_call_id,
    provider_tool_call_id,
};
include!("30c_compaction_v2_permission_denial_test/cases_test.rs");
