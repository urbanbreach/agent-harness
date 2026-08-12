use std::time::Instant;

use super::PresentationTimestamp;

#[derive(Clone, Debug)]
pub struct PresentationClock {
    epoch: Instant,
}

impl PresentationClock {
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }

    pub fn now(&self) -> PresentationTimestamp {
        self.timestamp(Instant::now())
    }

    pub fn timestamp(&self, instant: Instant) -> PresentationTimestamp {
        let elapsed = instant.saturating_duration_since(self.epoch).as_micros();
        PresentationTimestamp::from_micros(u64::try_from(elapsed).unwrap_or(u64::MAX))
    }
}

impl Default for PresentationClock {
    fn default() -> Self {
        Self::new()
    }
}
