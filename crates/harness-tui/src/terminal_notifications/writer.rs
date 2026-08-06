use std::io::Write;

use super::{NotificationEvent, NotificationProtocol, ProtocolSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    Written {
        protocol: NotificationProtocol,
        bytes: usize,
    },
    FallbackExhausted,
    WriteFailed(String),
}

pub struct NotificationWriter {
    protocol_set: ProtocolSet,
}

impl NotificationWriter {
    pub fn new(protocol_set: ProtocolSet) -> Self {
        Self { protocol_set }
    }

    pub fn write(&self, event: &NotificationEvent, out: &mut impl Write) -> WriteOutcome {
        if self.protocol_set.protocols.is_empty() {
            return WriteOutcome::FallbackExhausted;
        }
        let mut last_error = None;
        for protocol in self.protocol_set.fallback() {
            let sequence = protocol.sequence(&event.title, &event.body);
            let prefix = self
                .protocol_set
                .multiplexer
                .forwarding_prefix()
                .unwrap_or("");
            let suffix = self
                .protocol_set
                .multiplexer
                .forwarding_suffix()
                .unwrap_or("");
            let payload = format!("{prefix}{sequence}{suffix}");
            match out.write_all(payload.as_bytes()) {
                Ok(()) => {
                    return WriteOutcome::Written {
                        protocol: *protocol,
                        bytes: payload.len(),
                    };
                }
                Err(error) => last_error = Some(error.to_string()),
            }
        }
        WriteOutcome::WriteFailed(
            last_error.unwrap_or_else(|| "notification write failed".to_string()),
        )
    }

    pub fn shutdown(&self, out: &mut impl Write) -> Result<(), std::io::Error> {
        let _ = out.write_all(b"\x07");
        Ok(())
    }
}

impl Default for NotificationWriter {
    fn default() -> Self {
        Self::new(ProtocolSet::negotiate_from_env())
    }
}
