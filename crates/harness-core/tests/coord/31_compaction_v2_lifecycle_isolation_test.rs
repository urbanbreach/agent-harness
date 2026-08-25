use harness_core::UnwrapOrAbort;

mod support {
    use super::*;
    include!("31_compaction_v2_lifecycle_isolation_test/support.rs");
}

use support::{agent_turn, lifecycle_harness};
include!("31_compaction_v2_lifecycle_isolation_test/initial_cases.rs");
include!("31_compaction_v2_lifecycle_isolation_test/cancellation_cases.rs");
