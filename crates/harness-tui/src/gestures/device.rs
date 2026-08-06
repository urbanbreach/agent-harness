/// The physical input family used to produce a gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GestureDevice {
    /// Infer the device from the event's delta shape and recent history.
    Auto,
    /// Discrete wheel ticks.
    Wheel,
    /// High-resolution or fractional touchpad movement.
    Trackpad,
}
