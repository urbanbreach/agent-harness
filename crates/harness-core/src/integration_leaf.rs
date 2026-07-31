//! Integration leaf contract.
//!
//! Defines the narrow, typed API boundary through which integration leaf
//! owners (plugin lifecycle, ACP connections, MCP transport) interact with
//! the coordinator.
//!
//! # Authority preserved by the coordinator
//!
//! - **Permission**: plugin activation requires coordinator-owned permission
//!   resolution before side effects are applied. Leaf owners never bypass.
//! - **Event append**: integration lifecycle events are appended by the
//!   coordinator through `event_helpers`. Leaf owners never write events.
//! - **Lifecycle state**: plugin enable/disable state is managed by the
//!   coordinator's `RunState`. Leaf owners observe results, never mutate.
//!
//! Leaf owners call into [`CoordinatorHandle`] through this trait; they do not
//! access `RunState`, `event_helpers`, or the event store directly.

use async_trait::async_trait;

use crate::coord::{CoordinatorError, CoordinatorHandle};
use crate::integrations::PluginLifecycleSummary;

/// Typed request to query the plugin lifecycle summary.
///
/// This is a read-only query that does not append events or mutate state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginLifecycleQuery;

/// Integration leaf contract.
///
/// Implementors route all integration lifecycle operations through the
/// coordinator handle. No method on this trait appends events, resolves
/// permissions, or mutates coordinator lifecycle state directly.
///
/// # Permission boundary
///
/// Plugin activation requires coordinator-owned permission resolution.
/// Leaf owners must not activate plugins without coordinator-owned
/// permission checks.
///
/// # Integration requests
///
/// If a leaf owner needs a new coordinator-owned integration operation
/// (e.g. a new ACP transport variant), it must produce an integration
/// request describing the exact hunk rather than editing shared coord
/// files.
#[async_trait]
pub trait IntegrationLeaf: Send + Sync {
    /// Query the plugin lifecycle summary from the coordinator.
    ///
    /// This is a read-only query that returns the current plugin
    /// enable/disable state. It does not append events.
    async fn plugin_lifecycle_summary(
        &self,
        _query: PluginLifecycleQuery,
    ) -> Result<PluginLifecycleSummary, CoordinatorError>;
}

/// Coordinator-backed implementation of [`IntegrationLeaf`].
///
/// This is the canonical adapter that routes integration operations through
/// the coordinator handle. Leaf owners should depend on the trait, not this
/// struct, so test doubles can substitute.
#[derive(Debug, Clone)]
pub struct CoordinatorIntegrationLeaf {
    handle: CoordinatorHandle,
}

impl CoordinatorIntegrationLeaf {
    pub fn new(handle: CoordinatorHandle) -> Self {
        Self { handle }
    }
}

#[async_trait]
impl IntegrationLeaf for CoordinatorIntegrationLeaf {
    async fn plugin_lifecycle_summary(
        &self,
        _query: PluginLifecycleQuery,
    ) -> Result<PluginLifecycleSummary, CoordinatorError> {
        self.handle.plugin_lifecycle_summary().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_lifecycle_query_is_empty_marker() {
        // arrange
        // act
        let query = PluginLifecycleQuery;

        // assert — the query is an empty marker type; it exists to make
        // the trait method signature explicit and future-proof for
        // additional read-only query parameters.
        let _ = query;
    }

    // Full integration tests that exercise CoordinatorIntegrationLeaf with a
    // live coordinator are in tests/leaf_contracts_test.rs. CoordinatorHandle
    // construction requires a live coordinator channel (tx is pub(in crate::coord)).
}
