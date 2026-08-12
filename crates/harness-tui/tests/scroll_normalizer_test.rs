use std::time::Duration;

use harness_tui::input::{
    ScrollConfigOverrides, ScrollInputMode, ScrollNormalizer, ScrollNormalizerConfig,
    ScrollSampleDirection,
};
use harness_tui::terminal::brand::TerminalName;
use harness_tui::terminal::multiplexer::TerminalMultiplexer;

#[test]
fn low_amplitude_trackpad_stream_accumulates_before_emitting_a_line() {
    let mut normalizer = ScrollNormalizer::new(ScrollNormalizerConfig::for_terminal(
        TerminalName::Ghostty,
        TerminalMultiplexer::Undetected,
    ));

    let first = normalizer.push(Duration::ZERO, ScrollSampleDirection::Down, 20, 8, 24);
    let second = normalizer.push(
        Duration::from_millis(20),
        ScrollSampleDirection::Down,
        20,
        8,
        24,
    );
    let third = normalizer.push(
        Duration::from_millis(40),
        ScrollSampleDirection::Down,
        20,
        8,
        24,
    );

    assert_eq!((first.lines, second.lines, third.lines), (0, 0, 1));
}

#[test]
fn forced_wheel_notch_uses_terminal_event_density_once() {
    let config = ScrollNormalizerConfig::for_terminal(
        TerminalName::Ghostty,
        TerminalMultiplexer::Undetected,
    )
    .with_mode(ScrollInputMode::Wheel);
    let mut normalizer = ScrollNormalizer::new(config);

    let lines = [0, 4, 8]
        .into_iter()
        .map(|millis| {
            normalizer
                .push(
                    Duration::from_millis(millis),
                    ScrollSampleDirection::Down,
                    20,
                    8,
                    24,
                )
                .lines
        })
        .sum::<i16>();

    assert_eq!(lines, 3);
}

#[test]
fn auto_mode_reprices_a_burst_as_one_complete_wheel_notch() {
    let mut normalizer = ScrollNormalizer::new(ScrollNormalizerConfig::for_terminal(
        TerminalName::Ghostty,
        TerminalMultiplexer::Undetected,
    ));

    let lines = [0, 4, 8]
        .into_iter()
        .map(|millis| {
            normalizer
                .push(
                    Duration::from_millis(millis),
                    ScrollSampleDirection::Down,
                    20,
                    8,
                    24,
                )
                .lines
        })
        .sum::<i16>();

    assert_eq!(lines, 3);
}

#[test]
fn direction_change_discards_opposite_fractional_intent() {
    let config = ScrollNormalizerConfig::for_terminal(
        TerminalName::Ghostty,
        TerminalMultiplexer::Undetected,
    )
    .with_mode(ScrollInputMode::Trackpad);
    let mut normalizer = ScrollNormalizer::new(config);

    let _ = normalizer.push(Duration::ZERO, ScrollSampleDirection::Down, 20, 8, 24);
    let reversed = normalizer.push(
        Duration::from_millis(10),
        ScrollSampleDirection::Up,
        20,
        8,
        24,
    );

    assert_eq!(reversed.lines, 0);
    assert!(normalizer.fractional_lines() < 0.0);
}

#[test]
fn operator_overrides_force_mode_lines_speed_and_direction() {
    let overrides =
        ScrollConfigOverrides::from_values(Some("wheel"), Some("2"), Some("1.5"), Some("true"));
    let config = ScrollNormalizerConfig::for_terminal(
        TerminalName::WezTerm,
        TerminalMultiplexer::Undetected,
    )
    .with_overrides(overrides);
    let mut normalizer = ScrollNormalizer::new(config);

    let sample = normalizer.push(Duration::ZERO, ScrollSampleDirection::Down, 20, 8, 24);

    assert_eq!(sample.lines, -3);
}
