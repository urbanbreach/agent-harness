use std::fmt;

use super::Point;

/// Errors from an invalid drag lifecycle transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragError {
    AlreadyActive,
    NotActive,
}

impl fmt::Display for DragError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyActive => formatter.write_str("drag is already active"),
            Self::NotActive => formatter.write_str("drag is not active"),
        }
    }
}

impl std::error::Error for DragError {}

/// The stable snapshot returned when a drag ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DragSnapshot {
    pub start: Point,
    pub current: Point,
}

/// Button-drag lifecycle with no clock dependency.
#[derive(Debug, Clone, Copy, Default)]
pub struct DragLifecycle {
    start: Option<Point>,
    current: Option<Point>,
}

impl DragLifecycle {
    pub const fn new() -> Self {
        Self {
            start: None,
            current: None,
        }
    }

    pub fn begin(&mut self, point: Point) -> Result<(), DragError> {
        if self.start.is_some() {
            return Err(DragError::AlreadyActive);
        }
        self.start = Some(point);
        self.current = Some(point);
        Ok(())
    }

    pub fn update(&mut self, point: Point) -> Result<DragSnapshot, DragError> {
        let Some(start) = self.start else {
            return Err(DragError::NotActive);
        };
        self.current = Some(point);
        Ok(DragSnapshot {
            start,
            current: point,
        })
    }

    pub fn end(&mut self, point: Point) -> Result<DragSnapshot, DragError> {
        let Some(start) = self.start else {
            return Err(DragError::NotActive);
        };
        let snapshot = DragSnapshot {
            start,
            current: point,
        };
        self.start = None;
        self.current = None;
        Ok(snapshot)
    }

    pub const fn is_active(self) -> bool {
        self.start.is_some()
    }
}
