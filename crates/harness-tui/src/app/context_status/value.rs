//! Pure context status contract: one value computed once from the app and coordinator
//! snapshots, shared by the breadcrumb, footer and every other chrome that shows context.

/// Context pressure level; the chrome maps each to one theme role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextSeverity {
    Normal,
    Warning,
    /// The purple tier between warning and error.
    Elevated,
    Error,
}

/// Highest tier first: (tier, percent of window, absolute tokens). A tier applies at the
/// smaller of its percentage and its token ceiling.
const SEVERITY_TIERS: [(ContextSeverity, f64, f64); 3] = [
    (ContextSeverity::Error, 90.0, 500_000.0),
    (ContextSeverity::Elevated, 70.0, 270_000.0),
    (ContextSeverity::Warning, 50.0, 150_000.0),
];

/// Severity of a usage percentage. An invalid or nonpositive percent is normal; an unknown
/// or nonpositive window leaves only the percentage thresholds.
pub(crate) fn severity(percent: Option<f64>, window: Option<u32>) -> ContextSeverity {
    let Some(percent) = percent.filter(|value| value.is_finite() && *value > 0.0) else {
        return ContextSeverity::Normal;
    };
    let window = window.filter(|window| *window > 0);
    SEVERITY_TIERS
        .into_iter()
        .find(|(_, tier_percent, tier_tokens)| {
            // The token ceiling as a percentage is computed like `percent` itself so a usage
            // exactly at the ceiling compares equal.
            let limit = window.map_or(*tier_percent, |window| {
                tier_percent.min(tier_tokens * 100.0 / f64::from(window))
            });
            percent >= limit
        })
        .map_or(ContextSeverity::Normal, |(tier, _, _)| tier)
}

/// Compact token counts: `6.9K`, `49K`, `200K`, `1.0M`.
pub(crate) fn format_tokens(count: u64) -> String {
    fn one_decimal(tenths: u64, suffix: char, trim_zero: bool) -> String {
        if trim_zero && tenths.is_multiple_of(10) {
            format!("{}{suffix}", tenths / 10)
        } else {
            format!("{}.{}{suffix}", tenths / 10, tenths % 10)
        }
    }
    match count {
        0..=999 => count.to_string(),
        1_000..=9_999 => one_decimal((count + 50) / 100, 'K', true),
        10_000..=999_999 => format!("{}K", (count + 500) / 1_000),
        1_000_000..=9_999_999 => one_decimal((count + 50_000) / 100_000, 'M', false),
        10_000_000..=999_999_999 => format!("{}M", (count + 500_000) / 1_000_000),
        1_000_000_000..=9_999_999_999 => {
            one_decimal((count + 50_000_000) / 100_000_000, 'B', false)
        }
        _ => format!("{}B", count.saturating_add(500_000_000) / 1_000_000_000),
    }
}

/// Glyphs of the context status, by glyph mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContextSymbols {
    pub(crate) context: &'static str,
    pub(crate) auto: &'static str,
    pub(crate) speculation: &'static str,
    pub(crate) compaction: &'static str,
    pub(crate) filled: &'static str,
    pub(crate) empty: &'static str,
}

impl ContextSymbols {
    pub(crate) const UNICODE: Self = Self {
        context: "",
        auto: "⟲",
        speculation: "╎",
        compaction: "┃",
        filled: "█",
        empty: "░",
    };
    pub(crate) const ASCII: Self = Self {
        context: "ctx:",
        auto: "[A]",
        speculation: ":",
        compaction: "|",
        filled: "#",
        empty: "-",
    };

    pub(crate) const fn for_mode(ascii: bool) -> Self {
        if ascii {
            Self::ASCII
        } else {
            Self::UNICODE
        }
    }
}

/// What drives the auto-compaction icon colour: a running pass pulses, an armed window is
/// accent, anything else follows the context severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoState {
    Running,
    Armed,
    Idle,
}

/// What the coordinator last published about automatic compaction for the shown agent.
/// Absent until a status arrives and while automatic compaction is off: unknown is not
/// success, so the icon and the boundaries stay hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PublishedAuto {
    /// The resolved trigger in tokens for the active model.
    pub(crate) threshold_tokens: Option<u32>,
    /// Threshold minus the permitted speculation lead; absent when speculation cannot run.
    pub(crate) speculation_tokens: Option<u32>,
    pub(crate) state: AutoState,
}

/// Raw facts the status is computed from; every field is already resolved by the caller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ContextInputs {
    /// Tokens currently in context; `None` while unknown (for example after compaction,
    /// until the next response).
    pub(crate) used_tokens: Option<u32>,
    pub(crate) window_tokens: Option<u32>,
    pub(crate) auto: Option<PublishedAuto>,
}

/// Threshold and speculation boundaries of the gauge, in tokens of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GaugeBoundaries {
    pub(crate) window: u32,
    pub(crate) threshold: u32,
    /// Threshold minus the speculation lead; absent when speculation cannot run.
    pub(crate) speculation: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContextStatus {
    inputs: ContextInputs,
    window: Option<u32>,
    percent: Option<f64>,
    severity: ContextSeverity,
    auto: Option<AutoState>,
    gauge: Option<GaugeBoundaries>,
}

impl ContextStatus {
    /// `None` when neither usage nor a window is known.
    pub(crate) fn new(inputs: ContextInputs, symbols: &ContextSymbols) -> Option<Self> {
        let window = inputs.window_tokens.filter(|window| *window > 0);
        if window.is_none() && inputs.used_tokens.is_none() {
            return None;
        }
        let percent = inputs
            .used_tokens
            .zip(window)
            .map(|(used, window)| f64::from(used) * 100.0 / f64::from(window));
        let severity = severity(percent, window);
        let gauge = window
            .zip(
                inputs
                    .auto
                    .and_then(|auto| auto.threshold_tokens)
                    .filter(|threshold| *threshold > 0),
            )
            .map(|(window, threshold)| GaugeBoundaries {
                window,
                threshold,
                speculation: inputs.auto.and_then(|auto| auto.speculation_tokens),
            });
        let auto = inputs
            .auto
            .filter(|_| !symbols.auto.is_empty())
            .map(|auto| auto.state);
        Some(Self {
            inputs,
            window,
            percent,
            severity,
            auto,
            gauge,
        })
    }

    /// The same status at another usage: window, thresholds and settings are kept.
    pub(crate) fn with_used_tokens(&self, used_tokens: u32, symbols: &ContextSymbols) -> Self {
        let inputs = ContextInputs {
            used_tokens: Some(used_tokens),
            ..self.inputs
        };
        Self::new(inputs, symbols).unwrap_or(*self)
    }

    pub(crate) fn severity(&self) -> ContextSeverity {
        self.severity
    }

    /// `None` when auto-compaction is off; otherwise the state the icon is drawn in.
    pub(crate) fn auto(&self) -> Option<AutoState> {
        self.auto
    }

    /// Boundaries are absent when auto-compaction is off or the window is unknown.
    pub(crate) fn gauge(&self) -> Option<GaugeBoundaries> {
        self.gauge
    }

    /// Formatted window alone; absent for an unknown or zero window.
    pub(crate) fn context_total(&self) -> Option<String> {
        self.window.map(|window| format_tokens(u64::from(window)))
    }

    /// Status segment: `42.0%/200K`, window alone while usage is unknown, `900/?` while
    /// the window is unknown. ASCII mode prefixes `ctx:`.
    pub(crate) fn text(&self, symbols: &ContextSymbols) -> String {
        let body = match (self.percent, self.context_total(), self.inputs.used_tokens) {
            (Some(percent), Some(total), _) => format!("{percent:.1}%/{total}"),
            (None, Some(total), _) => total,
            (_, None, Some(used)) => format!("{}/?", format_tokens(u64::from(used))),
            (_, None, None) => String::new(),
        };
        if symbols.context.is_empty() {
            body
        } else {
            format!("{} {body}", symbols.context)
        }
    }

    /// Fixed-width bar with the threshold and speculation boundaries drawn over the fill.
    /// `None` without a window or without boundaries to show.
    pub(crate) fn gauge_text(&self, cells: u16, symbols: &ContextSymbols) -> Option<String> {
        let gauge = self.gauge?;
        if cells == 0 {
            return None;
        }
        let (cells, window) = (u64::from(cells), u64::from(gauge.window));
        let cell_of = |tokens: u32| (u64::from(tokens) * cells / window).min(cells - 1);
        let threshold_cell = cell_of(gauge.threshold);
        let speculation_cell = gauge.speculation.map(cell_of);
        let used = self.inputs.used_tokens.map(u64::from);
        Some(
            (0..cells)
                .map(|cell| {
                    if cell == threshold_cell {
                        symbols.compaction
                    } else if speculation_cell == Some(cell) {
                        symbols.speculation
                    } else if used.is_some_and(|used| used * cells * 2 >= (cell * 2 + 1) * window) {
                        symbols.filled
                    } else {
                        symbols.empty
                    }
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(used: Option<u32>, window: Option<u32>) -> ContextInputs {
        ContextInputs {
            used_tokens: used,
            window_tokens: window,
            ..ContextInputs::default()
        }
    }

    #[test]
    fn token_format_follows_the_compact_table() {
        for (count, expected) in [
            (0, "0"),
            (999, "999"),
            (1_000, "1K"),
            (6_900, "6.9K"),
            (9_999, "10K"),
            (10_000, "10K"),
            (48_573, "49K"),
            (200_000, "200K"),
            (262_144, "262K"),
            (1_000_000, "1.0M"),
            (1_250_000, "1.3M"),
            (12_500_000, "13M"),
            (1_000_000_000, "1.0B"),
            (12_000_000_000, "12B"),
        ] {
            assert_eq!(format_tokens(count), expected, "{count}");
        }
    }

    #[test]
    fn status_text_covers_percent_window_only_and_unknown_window() {
        // (used, window, expected unicode text, expected context_total)
        let cases = [
            (Some(84_000), Some(200_000), "42.0%/200K", Some("200K")),
            (None, Some(200_000), "200K", Some("200K")),
            (Some(0), Some(200_000), "0.0%/200K", Some("200K")),
            (Some(900), None, "900/?", None),
            (Some(900), Some(0), "900/?", None),
            (Some(48_573), None, "49K/?", None),
        ];
        for (used, window, text, total) in cases {
            let status = ContextStatus::new(inputs(used, window), &ContextSymbols::UNICODE)
                .expect("known usage or window");
            assert_eq!(status.text(&ContextSymbols::UNICODE), text);
            assert_eq!(status.context_total().as_deref(), total);
        }
        let ascii = ContextStatus::new(inputs(Some(84_000), Some(200_000)), &ContextSymbols::ASCII)
            .expect("known");
        assert_eq!(ascii.text(&ContextSymbols::ASCII), "ctx: 42.0%/200K");
        assert!(ContextStatus::new(inputs(None, None), &ContextSymbols::UNICODE).is_none());
        assert!(ContextStatus::new(inputs(None, Some(0)), &ContextSymbols::UNICODE).is_none());
    }

    #[test]
    fn severity_switches_at_the_exact_tier_limits() {
        use ContextSeverity::{Elevated, Error, Normal, Warning};
        let percent = |used: u32, window: u32| Some(f64::from(used) * 100.0 / f64::from(window));
        // (window, used tokens that sit one below and exactly at each limit)
        let cases: [(u32, [(u32, ContextSeverity); 8]); 2] = [
            (
                200_000,
                [
                    (99_999, Normal),
                    (100_000, Warning),
                    (139_999, Warning),
                    (140_000, Elevated),
                    (179_999, Elevated),
                    (180_000, Error),
                    (1, Normal),
                    (200_000, Error),
                ],
            ),
            (
                1_000_000,
                [
                    (149_999, Normal),
                    (150_000, Warning),
                    (269_999, Warning),
                    (270_000, Elevated),
                    (499_999, Elevated),
                    (500_000, Error),
                    (1, Normal),
                    (1_000_000, Error),
                ],
            ),
        ];
        for (window, rows) in cases {
            for (used, expected) in rows {
                assert_eq!(
                    severity(percent(used, window), Some(window)),
                    expected,
                    "{used}/{window}"
                );
            }
        }
    }

    #[test]
    fn severity_ignores_invalid_percent_and_unknown_window_uses_percent_only() {
        for percent in [
            None,
            Some(0.0),
            Some(-5.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
        ] {
            assert_eq!(severity(percent, Some(200_000)), ContextSeverity::Normal);
        }
        for (percent, expected) in [
            (49.9, ContextSeverity::Normal),
            (50.0, ContextSeverity::Warning),
            (70.0, ContextSeverity::Elevated),
            (90.0, ContextSeverity::Error),
        ] {
            assert_eq!(severity(Some(percent), None), expected);
            assert_eq!(severity(Some(percent), Some(0)), expected);
        }
    }

    fn auto_inputs(used: u32, speculation: bool, state: AutoState) -> ContextInputs {
        ContextInputs {
            used_tokens: Some(used),
            window_tokens: Some(200_000),
            auto: Some(PublishedAuto {
                threshold_tokens: Some(160_000),
                speculation_tokens: speculation.then_some(140_000),
                state,
            }),
        }
    }

    #[test]
    fn auto_icon_state_is_the_published_state_and_absent_without_one() {
        use AutoState::{Armed, Idle, Running};
        let none = ContextInputs {
            auto: None,
            ..auto_inputs(150_000, true, Idle)
        };
        let cases = [
            (auto_inputs(10_000, true, Idle), Some(Idle)),
            (auto_inputs(150_000, true, Armed), Some(Armed)),
            (auto_inputs(150_000, true, Running), Some(Running)),
            (none, None),
        ];
        for (inputs, expected) in cases {
            let status = ContextStatus::new(inputs, &ContextSymbols::UNICODE).expect("known");
            assert_eq!(status.auto(), expected, "{inputs:?}");
        }
    }

    #[test]
    fn gauge_boundaries_are_the_published_ones_and_absent_when_unknown() {
        let with = ContextStatus::new(
            auto_inputs(0, true, AutoState::Idle),
            &ContextSymbols::UNICODE,
        )
        .and_then(|status| status.gauge())
        .expect("gauge");
        assert_eq!(with.threshold, 160_000);
        assert_eq!(with.speculation, Some(140_000));
        assert_eq!(with.window, 200_000);

        let without_speculation = ContextStatus::new(
            auto_inputs(0, false, AutoState::Idle),
            &ContextSymbols::UNICODE,
        )
        .and_then(|status| status.gauge())
        .expect("gauge");
        assert_eq!(without_speculation.speculation, None);

        for inputs in [
            ContextInputs {
                auto: None,
                ..auto_inputs(0, true, AutoState::Idle)
            },
            ContextInputs {
                window_tokens: None,
                ..auto_inputs(0, true, AutoState::Idle)
            },
            ContextInputs {
                auto: Some(PublishedAuto {
                    threshold_tokens: None,
                    speculation_tokens: None,
                    state: AutoState::Idle,
                }),
                ..auto_inputs(0, true, AutoState::Idle)
            },
        ] {
            let status = ContextStatus::new(inputs, &ContextSymbols::UNICODE).expect("known");
            assert_eq!(status.gauge(), None, "{inputs:?}");
        }
    }

    #[test]
    fn gauge_text_draws_distinct_boundary_symbols_in_both_glyph_sets() {
        // 10 cells over a 200,000 window: speculation at cell 7, compaction at cell 8.
        for (symbols, expected) in [
            (ContextSymbols::UNICODE, "█████░░╎┃░"),
            (ContextSymbols::ASCII, "#####--:|-"),
        ] {
            let status = ContextStatus::new(auto_inputs(90_000, true, AutoState::Idle), &symbols)
                .expect("known");
            assert_eq!(status.gauge_text(10, &symbols).as_deref(), Some(expected));
        }
        let symbols = ContextSymbols::UNICODE;
        let ascii = ContextSymbols::ASCII;
        assert_ne!(symbols.speculation, symbols.compaction);
        assert_ne!(ascii.speculation, ascii.compaction);
        let status = ContextStatus::new(auto_inputs(1, true, AutoState::Armed), &symbols)
            .map(|status| status.with_used_tokens(150_000, &symbols))
            .expect("known");
        assert_eq!(status.auto(), Some(AutoState::Armed));
        assert_eq!(status.gauge_text(0, &symbols), None);
    }
}
