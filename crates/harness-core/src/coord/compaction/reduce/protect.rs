//! Which tool results the reducers must leave alone.
use super::*;
use harness_compaction::prune::{self, ToolMatcher};

impl Worker {
    /// The protection every automatic reducer starts from: `skill` results, skill reads,
    /// recovery reads of `artifact://` locations and results a tool marked as internal
    /// recovery artifacts. Extend this list to protect further results.
    pub(in crate::coord) fn default_protected_matchers(&self) -> Vec<ToolMatcher> {
        let mut matchers = prune::skill_protection();
        matchers.push(ToolMatcher::artifact_recovery());
        matchers
    }

    /// Protection for passes that may rewrite recovery reads (age pruning and manual shake).
    pub(super) fn skill_matchers(&self) -> Vec<ToolMatcher> {
        prune::skill_protection()
    }
}
