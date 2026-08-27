use std::sync::Arc;

use harness_core::attachment_transport::AttachmentMetadata;
use harness_core::conversation::{
    project_conversation, ConversationMessage, ConversationProjectionError,
};
use harness_core::event::{
    AssistantMessageFinishedEvent, BranchSummaryEvent, EventEnvelopeV1, EventV1,
    PromptAttachmentsSubmittedEvent, ProviderStreamDeltaEvent, ToolCallFinishedEvent,
    ToolCallStatus,
};
use harness_core::ids::{EntryId, RunId, ToolCallId};
use harness_core::session::legacy::LegacyIdentityNamespace;
use harness_core::session::{AssistantPart, AssistantToolCall as CanonicalAssistantToolCall};
use harness_core::UnwrapOrAbort;
use harness_providers::CompletionRequest;
use serde_json::json;

mod support {
    use super::*;
    include!("06_compaction_v2_protocol/support_test.rs");
}

mod runtime_support {
    use super::*;
    include!("06_compaction_v2_protocol/runtime_support.rs");
}

mod canonical_behavior {
    use super::*;
    include!("06_compaction_v2_protocol/canonical_behavior_test.rs");
}

mod durable_context {
    use super::runtime_support::*;
    use super::*;
    include!("06_compaction_v2_protocol/durable_context_test.rs");
}

mod projection_behavior {
    use super::support::*;
    use super::*;
    include!("06_compaction_v2_protocol/projection_behavior_test.rs");
}
