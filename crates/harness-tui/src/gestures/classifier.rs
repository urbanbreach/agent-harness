use super::{GestureDevice, GESTURE_BOUNDARY_MS};

/// The input shape used by the pure classifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureInput {
    Scroll,
    DragBegin,
    DragUpdate,
    DragEnd,
}

/// A timestamped input sample. Callers supply timestamps; classification never reads a clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GestureEvent {
    pub input: GestureInput,
    pub device: GestureDevice,
    pub delta_lines: f64,
    pub timestamp_ms: u64,
}

impl GestureEvent {
    pub const fn scroll(delta_lines: f64, timestamp_ms: u64) -> Self {
        Self {
            input: GestureInput::Scroll,
            device: GestureDevice::Auto,
            delta_lines,
            timestamp_ms,
        }
    }

    pub const fn with_device(self, device: GestureDevice) -> Self {
        Self { device, ..self }
    }
}

/// The normalized result consumed by downstream gesture state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureKind {
    None,
    Scroll { device: GestureDevice },
    DragBegin,
    DragUpdate,
    DragEnd,
}

impl GestureKind {
    pub const fn scroll(device: GestureDevice) -> Self {
        Self::Scroll { device }
    }
}

/// Minimal caller-owned history used to keep Auto classification stable.
#[derive(Debug, Clone, Copy, Default)]
pub struct GestureHistory {
    last_scroll_timestamp_ms: Option<u64>,
    last_scroll_device: Option<GestureDevice>,
}

impl GestureHistory {
    pub fn observe(&mut self, event: &GestureEvent) {
        if event.input != GestureInput::Scroll || !event.delta_lines.is_finite() {
            return;
        }
        self.last_scroll_timestamp_ms = Some(event.timestamp_ms);
        self.last_scroll_device = Some(resolve_device(event, self));
    }
}

/// Classifies an event using only the event and supplied history.
pub fn classify(event: &GestureEvent, history: &GestureHistory) -> GestureKind {
    match event.input {
        GestureInput::Scroll => GestureKind::scroll(resolve_device(event, history)),
        GestureInput::DragBegin => GestureKind::DragBegin,
        GestureInput::DragUpdate => GestureKind::DragUpdate,
        GestureInput::DragEnd => GestureKind::DragEnd,
    }
}

fn resolve_device(event: &GestureEvent, history: &GestureHistory) -> GestureDevice {
    match event.device {
        GestureDevice::Auto => {
            let fractional = event.delta_lines.fract() != 0.0;
            let small_delta = event.delta_lines.abs() < 1.0;
            let same_gesture = history
                .last_scroll_timestamp_ms
                .is_some_and(|last| event.timestamp_ms.saturating_sub(last) <= GESTURE_BOUNDARY_MS);
            if fractional || small_delta {
                GestureDevice::Trackpad
            } else if same_gesture && history.last_scroll_device == Some(GestureDevice::Trackpad) {
                GestureDevice::Trackpad
            } else {
                GestureDevice::Wheel
            }
        }
        GestureDevice::Wheel => GestureDevice::Wheel,
        GestureDevice::Trackpad => GestureDevice::Trackpad,
    }
}
