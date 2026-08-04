//! Focus-aware terminal notifications: policy, protocols, debounce, fallback.

pub mod policy;
pub mod protocol;
pub mod writer;

pub use policy::{
    FocusState, NotificationEvent, NotificationKind, NotificationPolicy, SuppressionState,
};
pub use protocol::{Multiplexer, NotificationProtocol, ProtocolSet};
pub use writer::{NotificationWriter, WriteOutcome};
