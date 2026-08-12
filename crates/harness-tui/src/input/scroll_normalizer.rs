use std::collections::VecDeque;
use std::time::Duration;

use crate::terminal::{TerminalMultiplexer, TerminalName};

pub const SCROLL_STREAM_GAP: Duration = Duration::from_millis(80);
pub const SCROLL_REDRAW_CADENCE: Duration = Duration::from_millis(16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollInputMode {
    Auto,
    Wheel,
    Trackpad,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScrollConfigOverrides {
    pub mode: Option<ScrollInputMode>,
    pub lines: Option<f32>,
    pub speed: Option<f32>,
    pub inverted: Option<bool>,
}

impl ScrollConfigOverrides {
    pub fn from_values(
        mode: Option<&str>,
        lines: Option<&str>,
        speed: Option<&str>,
        inverted: Option<&str>,
    ) -> Self {
        Self {
            mode: mode.and_then(parse_scroll_mode),
            lines: lines.and_then(|value| value.parse().ok()),
            speed: speed.and_then(|value| value.parse().ok()),
            inverted: inverted.and_then(parse_bool),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollSampleDirection {
    Up,
    Down,
}

impl ScrollSampleDirection {
    const fn sign(self) -> f32 {
        match self {
            Self::Up => -1.0,
            Self::Down => 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollNormalizerConfig {
    mode: ScrollInputMode,
    events_per_notch: u8,
    wheel_lines: f32,
    trackpad_lines: f32,
    speed: f32,
    inverted: bool,
}

impl ScrollNormalizerConfig {
    pub fn for_terminal(terminal: TerminalName, multiplexer: TerminalMultiplexer) -> Self {
        let events_per_notch = if multiplexer.is_detected() {
            1
        } else {
            match terminal {
                TerminalName::Iterm2
                | TerminalName::VsCode
                | TerminalName::Cursor
                | TerminalName::Windsurf
                | TerminalName::Zed
                | TerminalName::WezTerm => 1,
                _ => 3,
            }
        };
        Self {
            mode: ScrollInputMode::Auto,
            events_per_notch,
            wheel_lines: f32::from(events_per_notch),
            trackpad_lines: 1.0,
            speed: 1.0,
            inverted: false,
        }
    }

    pub const fn with_mode(mut self, mode: ScrollInputMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_lines(mut self, lines: f32) -> Self {
        let lines = lines.clamp(1.0, 100.0);
        self.wheel_lines = lines;
        self.trackpad_lines = lines;
        self
    }

    pub fn with_speed(mut self, speed: f32) -> Self {
        self.speed = speed.clamp(0.1, 8.0);
        self
    }

    pub const fn with_inverted(mut self, inverted: bool) -> Self {
        self.inverted = inverted;
        self
    }

    pub fn with_overrides(mut self, overrides: ScrollConfigOverrides) -> Self {
        if let Some(mode) = overrides.mode {
            self = self.with_mode(mode);
        }
        if let Some(lines) = overrides.lines {
            self = self.with_lines(lines);
        }
        if let Some(speed) = overrides.speed {
            self = self.with_speed(speed);
        }
        if let Some(inverted) = overrides.inverted {
            self = self.with_inverted(inverted);
        }
        self
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NormalizedScroll {
    pub lines: i16,
    pub column: u16,
    pub row: u16,
}

#[derive(Debug, Clone, PartialEq)]
struct ScrollStream {
    direction: ScrollSampleDirection,
    started_at: Duration,
    last_event_at: Duration,
    events: u16,
    classified_wheel: bool,
    intervals: VecDeque<Duration>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScrollNormalizer {
    config: ScrollNormalizerConfig,
    stream: Option<ScrollStream>,
    fractional_lines: f32,
}

impl ScrollNormalizer {
    pub const fn new(config: ScrollNormalizerConfig) -> Self {
        Self {
            config,
            stream: None,
            fractional_lines: 0.0,
        }
    }

    pub fn push(
        &mut self,
        now: Duration,
        direction: ScrollSampleDirection,
        column: u16,
        row: u16,
        viewport_height: u16,
    ) -> NormalizedScroll {
        let direction = if self.config.inverted {
            match direction {
                ScrollSampleDirection::Up => ScrollSampleDirection::Down,
                ScrollSampleDirection::Down => ScrollSampleDirection::Up,
            }
        } else {
            direction
        };
        let reset = self.stream.as_ref().is_none_or(|stream| {
            stream.direction != direction
                || now.saturating_sub(stream.last_event_at) > SCROLL_STREAM_GAP
        });
        if reset {
            self.fractional_lines = 0.0;
            self.stream = Some(ScrollStream {
                direction,
                started_at: now,
                last_event_at: now,
                events: 0,
                classified_wheel: false,
                intervals: VecDeque::with_capacity(6),
            });
        }

        let Some(stream) = self.stream.as_mut() else {
            return NormalizedScroll {
                lines: 0,
                column,
                row,
            };
        };
        if stream.events > 0 {
            let interval = now.saturating_sub(stream.last_event_at);
            if interval >= Duration::from_millis(6) {
                if stream.intervals.len() == 6 {
                    stream.intervals.pop_front();
                }
                stream.intervals.push_back(interval);
            }
        }
        stream.last_event_at = now;
        stream.events = stream.events.saturating_add(1);

        let kind = match self.config.mode {
            ScrollInputMode::Auto => classify_stream(stream, self.config.events_per_notch),
            forced => forced,
        };
        let trackpad_base =
            self.config.trackpad_lines / f32::from(self.config.events_per_notch.max(1));
        let wheel_base = self.config.wheel_lines / f32::from(self.config.events_per_notch.max(1));
        if self.config.mode == ScrollInputMode::Auto
            && kind == ScrollInputMode::Wheel
            && !stream.classified_wheel
        {
            let previous_events = stream.events.saturating_sub(1);
            self.fractional_lines += direction.sign()
                * (wheel_base - trackpad_base)
                * f32::from(previous_events)
                * self.config.speed;
            stream.classified_wheel = true;
        }
        let base_lines = match kind {
            ScrollInputMode::Wheel => wheel_base,
            ScrollInputMode::Auto | ScrollInputMode::Trackpad => trackpad_base,
        };
        let acceleration = trackpad_acceleration(kind, &stream.intervals);
        self.fractional_lines += direction.sign() * base_lines * self.config.speed * acceleration;

        let cap = i16::try_from(viewport_height.max(1) / 2)
            .unwrap_or(i16::MAX)
            .clamp(1, 12);
        let whole = whole_lines(self.fractional_lines);
        let lines = whole.clamp(-cap, cap);
        self.fractional_lines -= f32::from(lines);
        NormalizedScroll { lines, column, row }
    }

    pub const fn fractional_lines(&self) -> f32 {
        self.fractional_lines
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the value is explicitly clamped to the complete i16 domain before conversion"
)]
fn whole_lines(value: f32) -> i16 {
    value
        .clamp(f32::from(i16::MIN), f32::from(i16::MAX))
        .trunc() as i16
}

fn classify_stream(stream: &ScrollStream, events_per_notch: u8) -> ScrollInputMode {
    if stream.events >= u16::from(events_per_notch.max(1))
        && stream.last_event_at.saturating_sub(stream.started_at) <= Duration::from_millis(12)
    {
        ScrollInputMode::Wheel
    } else {
        ScrollInputMode::Trackpad
    }
}

fn trackpad_acceleration(kind: ScrollInputMode, intervals: &VecDeque<Duration>) -> f32 {
    if kind != ScrollInputMode::Trackpad || intervals.is_empty() {
        return 1.0;
    }
    let total = intervals.iter().copied().sum::<Duration>();
    let average = total / u32::try_from(intervals.len()).unwrap_or(1);
    if average < Duration::from_millis(10) {
        2.4
    } else if average < Duration::from_millis(18) {
        1.6
    } else {
        1.0
    }
}

fn parse_scroll_mode(value: &str) -> Option<ScrollInputMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(ScrollInputMode::Auto),
        "wheel" => Some(ScrollInputMode::Wheel),
        "trackpad" => Some(ScrollInputMode::Trackpad),
        _ => None,
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}
