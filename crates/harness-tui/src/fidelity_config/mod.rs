//! TUI fidelity settings contract: defaults, migration, rollback, persistence.

pub mod contract;
pub mod migration;
pub mod rollback;

pub use contract::{
    ConfigValidationError, FidelityConfig, FidelityDefaults, InputMode, MotionMode,
    NotificationMode,
};
pub use migration::{ConfigMigration, MigrationError, MigrationResult};
pub use rollback::{RollbackDecision, RollbackToggles};
