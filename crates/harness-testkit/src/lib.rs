//! Test-only helpers for secret scanning and deterministic verification lanes.
//!
//! Keep runtime-independent testing utilities here; PTY/live workflow code
//! belongs under `crates/harness-testkit/tests/` with local support modules.

pub mod fakes;
pub mod secret_scanner;
pub mod simulation;
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
