//! Centralized terminal capability and accessibility matrix.

pub mod axes;
pub mod classify;
pub mod matrix;
pub mod test_data;

pub use axes::*;
pub use classify::CapabilityClassifier;
pub use matrix::{CapabilityCell, CapabilityMatrix};
pub use test_data::well_known_profiles;
