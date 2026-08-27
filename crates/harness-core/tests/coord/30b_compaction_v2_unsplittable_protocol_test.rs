use super::part_30_compaction_v2_budget_protocol_test::safe_cut::{
    plan_safe_cut, SafeCutCandidate, SafeCutError,
};
use harness_core::{estimate_compaction_text_tokens, UnwrapOrAbort};

mod support {
    use super::*;
    include!("30b_compaction_v2_unsplittable_protocol/support_test.rs");
}

mod large_tool_result {
    use super::support::*;
    use super::*;
    include!("30b_compaction_v2_unsplittable_protocol/large_tool_result_test.rs");
}

mod same_turn_tool_overflow {
    use super::support::*;
    use super::*;
    include!("30b_compaction_v2_unsplittable_protocol/same_turn_tool_overflow_test.rs");
}

mod unsplittable_protocol_entry {
    use super::support::*;
    use super::*;
    include!("30b_compaction_v2_unsplittable_protocol/unsplittable_protocol_entry_test.rs");
}
