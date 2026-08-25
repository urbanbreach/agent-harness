#[path = "../../src/coord/session_compaction/budget/complete_request.rs"]
mod complete_request;
#[path = "../../src/coord/compaction/cut_point/safe.rs"]
pub(super) mod safe_cut;

use complete_request::{
    plan_complete_request, resolve_usage_anchor, AnchorBudgetComponents, CompactionHistoryTokens,
    CompleteRequestComponents, CurrentRequestModel, RequestTerminalStatus, StartedRequestMetadata,
    UsageAnchorResolution, UsageCandidate,
};
use harness_core::{estimate_compaction_text_tokens, UnwrapOrAbort};
use safe_cut::{plan_safe_cut, SafeCutCandidate};

mod previous_summary_counted_once {
    use super::*;
    include!("30_compaction_v2_budget_protocol/previous_summary_counted_once_test.rs");
}

mod huge_turn_safe_prefix {
    use super::*;
    include!("30_compaction_v2_budget_protocol/huge_turn_safe_prefix_test.rs");
}

mod attachments_charge {
    use super::*;
    include!("30_compaction_v2_budget_protocol/attachments_charge_test.rs");
}

mod aborted_usage {
    use super::*;
    include!("30_compaction_v2_budget_protocol/aborted_usage_test.rs");
}

mod model_downshift {
    use super::*;
    include!("30_compaction_v2_budget_protocol/model_downshift_test.rs");
}
