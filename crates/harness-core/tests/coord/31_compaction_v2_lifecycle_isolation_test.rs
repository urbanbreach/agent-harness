use harness_core::UnwrapOrAbort;

mod support {
    use super::*;
    include!("31_compaction_v2_lifecycle_isolation_test/support_test.rs");
}

use support::{agent_turn, lifecycle_harness};
include!("31_compaction_v2_lifecycle_isolation_test/initial_cases_test.rs");
include!("31_compaction_v2_lifecycle_isolation_test/cancellation_cases_test.rs");
