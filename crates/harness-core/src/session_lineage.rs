//! Pure helpers for session lineage operations.
//!
//! Contract:
//! - `fork = selected stable prefix`
//! - `clone = latest stable prefix`
//! - `tree = read-only lineage browser`

mod materialization;
mod stable_prefix;
mod tree;

pub use self::materialization::{
    materialize_child_session, materialize_child_session_with_child_run_id_source,
    rewrite_child_event_envelope, ChildRunIdSource, ChildSessionMaterializationError,
    ChildSessionMaterializationRequest, ChildSessionMaterializationResult,
    ChildSessionMaterializationSourceKind, SystemChildRunIdSource,
};
pub use self::stable_prefix::{
    latest_clone_stable_prefix, validate_fork_stable_prefix, validate_stable_prefix,
    validate_tui_fork_stable_prefix, SessionLineageError, StableSessionPrefix,
};
pub use self::tree::{
    project_lineage_tree, SessionLineageNode, SessionLineageRow, SessionLineageTree,
};

#[cfg(test)]
use self::materialization::materialize_child_session_inner;

#[cfg(test)]
#[path = "session_lineage/tests.rs"]
mod tests;
