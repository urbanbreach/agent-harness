//! Core runtime and domain crate for Agent Harness.

pub mod agent;
pub mod agent_catalog;
pub mod auth;
pub mod clock;
pub mod config;
pub mod conversation;
pub mod coord;
pub(crate) mod counter_id;
pub(crate) mod digest;
pub mod edit;
pub mod event;
pub mod extension_manifest;
pub mod file_tag;
pub mod model_resolution;
pub(crate) mod path_display;
pub(crate) mod path_selector;
pub mod perm;
pub mod plan;
pub mod proj;
pub(crate) mod provider_args;
pub mod provider_catalog;
pub mod question_answers;
pub mod redact;
pub mod sched;
pub mod session_lineage;
pub(crate) mod session_paths;
pub mod session_title;
pub mod store;
pub(crate) mod text;
pub mod tool;
pub mod transcript_projection;
pub mod workspace;

pub trait UnwrapOrAbort<T> {
    fn unwrap_or_abort(self) -> T;
}

#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "replaces .expect() which also panics; abort() kills test processes"
)]
impl<T> UnwrapOrAbort<T> for Option<T> {
    fn unwrap_or_abort(self) -> T {
        match self {
            Some(v) => v,
            None => panic!("unwrap_or_abort on None"),
        }
    }
}

#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "replaces .expect() which also panics; abort() kills test processes"
)]
impl<T, E> UnwrapOrAbort<T> for Result<T, E> {
    fn unwrap_or_abort(self) -> T {
        match self {
            Ok(v) => v,
            Err(_) => panic!("unwrap_or_abort on Err"),
        }
    }
}
