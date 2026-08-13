use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionDemand {
    None,
    Until(Duration),
    Slow(Duration),
    Fast(Duration),
}

impl MotionDemand {
    pub const fn until(remaining: Duration) -> Self {
        Self::Until(remaining)
    }

    pub const fn slow(interval: Duration) -> Self {
        Self::Slow(interval)
    }

    pub const fn fast(interval: Duration) -> Self {
        Self::Fast(interval)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionCadence {
    None,
    Slow(Duration),
    Fast(Duration),
}

impl MotionCadence {
    pub const fn interval(self) -> Option<Duration> {
        match self {
            Self::None => None,
            Self::Slow(interval) | Self::Fast(interval) => Some(interval),
        }
    }

    const fn merge(self, candidate: Self) -> Self {
        match (self, candidate) {
            (Self::Fast(left), Self::Fast(right)) => Self::Fast(min_duration(left, right)),
            (Self::Fast(interval), Self::Slow(_) | Self::None)
            | (Self::Slow(_) | Self::None, Self::Fast(interval)) => Self::Fast(interval),
            (Self::Slow(left), Self::Slow(right)) => Self::Slow(min_duration(left, right)),
            (Self::Slow(interval), Self::None) | (Self::None, Self::Slow(interval)) => {
                Self::Slow(interval)
            }
            (Self::None, Self::None) => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionPlan {
    cadence: MotionCadence,
    until: Option<Duration>,
    revision: u64,
    visual_sample: u64,
}

impl MotionPlan {
    pub const fn none() -> Self {
        Self {
            cadence: MotionCadence::None,
            until: None,
            revision: 0,
            visual_sample: 0,
        }
    }

    pub const fn from_demand(demand: MotionDemand) -> Self {
        Self::none().merge(demand)
    }

    pub fn from_demands(demands: impl IntoIterator<Item = MotionDemand>) -> Self {
        demands.into_iter().fold(Self::none(), Self::merge)
    }

    pub const fn cadence(self) -> MotionCadence {
        self.cadence
    }

    pub const fn until(self) -> Option<Duration> {
        self.until
    }

    pub const fn revision(self) -> u64 {
        self.revision
    }

    pub const fn with_revision(mut self, revision: u64) -> Self {
        self.revision = revision;
        self
    }

    pub const fn visual_sample(self) -> u64 {
        self.visual_sample
    }

    pub const fn with_visual_sample(mut self, visual_sample: u64) -> Self {
        self.visual_sample = visual_sample;
        self
    }

    pub(super) const fn without_cadence(mut self) -> Self {
        self.cadence = MotionCadence::None;
        self
    }

    pub const fn is_none(self) -> bool {
        matches!(self.cadence, MotionCadence::None) && self.until.is_none()
    }

    pub const fn merge(mut self, demand: MotionDemand) -> Self {
        match demand {
            MotionDemand::None => {}
            MotionDemand::Until(remaining) => {
                self.until = Some(match self.until {
                    Some(current) => min_duration(current, remaining),
                    None => remaining,
                });
            }
            MotionDemand::Slow(interval) => {
                self.cadence = self.cadence.merge(MotionCadence::Slow(interval));
            }
            MotionDemand::Fast(interval) => {
                self.cadence = self.cadence.merge(MotionCadence::Fast(interval));
            }
        }
        self
    }
}

impl Default for MotionPlan {
    fn default() -> Self {
        Self::none()
    }
}

impl From<bool> for MotionPlan {
    fn from(active: bool) -> Self {
        if active {
            Self::from_demand(MotionDemand::fast(Duration::from_millis(
                super::ANIMATION_PERIOD_MS,
            )))
        } else {
            Self::none()
        }
    }
}

const fn min_duration(left: Duration, right: Duration) -> Duration {
    if left.as_nanos() <= right.as_nanos() {
        left
    } else {
        right
    }
}
