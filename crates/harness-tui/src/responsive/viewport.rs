//! Viewport classification and frame-plan leaf for responsive parity rows.
//!
//! Each manifest RESP-* row maps to a `ViewportId` and a `ViewportPlan` that
//! records the deterministic geometry classification, composer border state,
//! and footer hint visibility at that viewport. These are pure value types —
//! no `AppState` or shared registry dependency.

use crate::theme::{ShellBreakpoints, ShellGeometryTarget};

/// Canonical viewport identifiers matching the TUI parity manifest RESP-* rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewportId {
    /// RESP-120x50
    V120x50,
    /// RESP-120x40
    V120x40,
    /// RESP-100x30
    V100x30,
    /// RESP-80x24
    V80x24,
    /// RESP-79x24
    V79x24,
    /// RESP-60x20
    V60x20,
    /// RESP-WIDE (140x40)
    VWide,
}

impl ViewportId {
    /// Manifest behavior_id string (e.g. `"RESP-120x50"`).
    pub const fn behavior_id(self) -> &'static str {
        match self {
            Self::V120x50 => "RESP-120x50",
            Self::V120x40 => "RESP-120x40",
            Self::V100x30 => "RESP-100x30",
            Self::V80x24 => "RESP-80x24",
            Self::V79x24 => "RESP-79x24",
            Self::V60x20 => "RESP-60x20",
            Self::VWide => "RESP-WIDE",
        }
    }

    /// (cols, rows) for this viewport.
    pub const fn dims(self) -> (u16, u16) {
        match self {
            Self::V120x50 => (120, 50),
            Self::V120x40 => (120, 40),
            Self::V100x30 => (100, 30),
            Self::V80x24 => (80, 24),
            Self::V79x24 => (79, 24),
            Self::V60x20 => (60, 20),
            Self::VWide => (140, 40),
        }
    }

    /// All seven manifest viewport IDs in manifest order.
    pub const ALL: [Self; 7] = [
        Self::V120x50,
        Self::V120x40,
        Self::V100x30,
        Self::V80x24,
        Self::V79x24,
        Self::V60x20,
        Self::VWide,
    ];
}

/// All seven required viewport IDs as constants.
#[allow(
    non_upper_case_globals,
    reason = "viewport names mirror manifest RESP-* row IDs"
)]
pub const VIEWPORT_120x50: ViewportId = ViewportId::V120x50;
#[allow(
    non_upper_case_globals,
    reason = "viewport names mirror manifest RESP-* row IDs"
)]
pub const VIEWPORT_120x40: ViewportId = ViewportId::V120x40;
#[allow(
    non_upper_case_globals,
    reason = "viewport names mirror manifest RESP-* row IDs"
)]
pub const VIEWPORT_100x30: ViewportId = ViewportId::V100x30;
#[allow(
    non_upper_case_globals,
    reason = "viewport names mirror manifest RESP-* row IDs"
)]
pub const VIEWPORT_80x24: ViewportId = ViewportId::V80x24;
#[allow(
    non_upper_case_globals,
    reason = "viewport names mirror manifest RESP-* row IDs"
)]
pub const VIEWPORT_79x24: ViewportId = ViewportId::V79x24;
#[allow(
    non_upper_case_globals,
    reason = "viewport names mirror manifest RESP-* row IDs"
)]
pub const VIEWPORT_60x20: ViewportId = ViewportId::V60x20;
pub const VIEWPORT_WIDE: ViewportId = ViewportId::VWide;

/// Geometry classification derived from the shared `ShellBreakpoints`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportClassification {
    pub target: ShellGeometryTarget,
    pub cols: u16,
    pub rows: u16,
}

impl ViewportClassification {
    pub fn from_dims(cols: u16, rows: u16) -> Self {
        Self {
            target: ShellBreakpoints::DEFAULT.target(cols, rows),
            cols,
            rows,
        }
    }

    /// True when the viewport is at or below the minimum breakpoint (80x24).
    pub const fn is_compact(self) -> bool {
        matches!(self.target, ShellGeometryTarget::Minimum)
    }

    /// True when the viewport is wide enough for the primary layout (>=100x30).
    pub const fn is_primary(self) -> bool {
        matches!(self.target, ShellGeometryTarget::Primary)
    }
}

/// Deterministic frame-plan summary for a viewport — no `AppState` dependency.
///
/// Records the observable rendering decisions at a given viewport so tests
/// can assert them without masking geometry differences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportPlan {
    pub id: ViewportId,
    pub classification: ViewportClassification,
    /// Composer retains its bordered box at every viewport.
    pub composer_bordered: bool,
    /// Idle footer hints are visible at every viewport.
    pub footer_hints_visible: bool,
    /// Welcome panel is suppressed in the idle-shell scenario.
    pub welcome_panel_visible: bool,
    /// Breadcrumb top-margin rows (0 at ultra-compact ≤60 cols, 1 elsewhere).
    pub breadcrumb_top_margin: u16,
    /// Composer→disclosure spacer rows (0 at auto-compact ≤20 rows, 1 elsewhere).
    pub composer_footer_spacer: u16,
}

impl ViewportPlan {
    pub fn for_viewport(id: ViewportId) -> Self {
        let (cols, rows) = id.dims();
        let classification = ViewportClassification::from_dims(cols, rows);
        Self {
            id,
            classification,
            // The composer border is preserved at every viewport — no masking.
            composer_bordered: true,
            // Footer hints (Shift+Tab:mode / Ctrl+x:shortcuts) are always visible.
            footer_hints_visible: true,
            // Idle shell scenario: no welcome panel.
            welcome_panel_visible: false,
            // Breadcrumb top margin: 0 at ultra-compact (≤60 cols), 1 elsewhere.
            breadcrumb_top_margin: crate::layout::breadcrumb_top_margin(cols),
            composer_footer_spacer: crate::layout::composer_footer_spacer_rows(rows),
        }
    }

    /// All seven manifest viewport plans in manifest order.
    pub fn all_plans() -> Vec<Self> {
        ViewportId::ALL
            .iter()
            .map(|&id| Self::for_viewport(id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_ids_cover_all_manifest_rows() {
        // arrange
        let expected = [
            "RESP-120x50",
            "RESP-120x40",
            "RESP-100x30",
            "RESP-80x24",
            "RESP-79x24",
            "RESP-60x20",
            "RESP-WIDE",
        ];

        // act
        let actual: Vec<&str> = ViewportId::ALL.iter().map(|v| v.behavior_id()).collect();

        // assert
        assert_eq!(actual.as_slice(), expected.as_slice());
    }

    #[test]
    fn viewport_dims_match_manifest() {
        // arrange
        // act
        // assert
        assert_eq!(VIEWPORT_120x50.dims(), (120, 50));
        assert_eq!(VIEWPORT_120x40.dims(), (120, 40));
        assert_eq!(VIEWPORT_100x30.dims(), (100, 30));
        assert_eq!(VIEWPORT_80x24.dims(), (80, 24));
        assert_eq!(VIEWPORT_79x24.dims(), (79, 24));
        assert_eq!(VIEWPORT_60x20.dims(), (60, 20));
        assert_eq!(VIEWPORT_WIDE.dims(), (140, 40));
    }

    #[test]
    fn viewport_plan_preserves_composer_border_at_every_viewport() {
        // arrange
        // act
        // assert
        for plan in ViewportPlan::all_plans() {
            assert!(
                plan.composer_bordered,
                "{}: composer must be bordered",
                plan.id.behavior_id()
            );
            assert!(
                plan.footer_hints_visible,
                "{}: footer hints must be visible",
                plan.id.behavior_id()
            );
            assert!(
                !plan.welcome_panel_visible,
                "{}: welcome panel must not appear in idle shell",
                plan.id.behavior_id()
            );
        }
    }

    #[test]
    fn viewport_plan_breadcrumb_margin_is_zero_only_at_ultra_compact() {
        // arrange
        // act
        let v60 = ViewportPlan::for_viewport(VIEWPORT_60x20);
        let v79 = ViewportPlan::for_viewport(VIEWPORT_79x24);
        let v80 = ViewportPlan::for_viewport(VIEWPORT_80x24);
        let v100 = ViewportPlan::for_viewport(VIEWPORT_100x30);
        let v120x40 = ViewportPlan::for_viewport(VIEWPORT_120x40);
        let v120x50 = ViewportPlan::for_viewport(VIEWPORT_120x50);
        let vwide = ViewportPlan::for_viewport(VIEWPORT_WIDE);

        // assert — only 60x20 has zero breadcrumb top margin
        assert_eq!(
            v60.breadcrumb_top_margin, 0,
            "60x20: no breadcrumb top margin"
        );
        assert_eq!(v79.breadcrumb_top_margin, 1, "79x24: breadcrumb top margin");
        assert_eq!(v80.breadcrumb_top_margin, 1, "80x24: breadcrumb top margin");
        assert_eq!(
            v100.breadcrumb_top_margin, 1,
            "100x30: breadcrumb top margin"
        );
        assert_eq!(
            v120x40.breadcrumb_top_margin, 1,
            "120x40: breadcrumb top margin"
        );
        assert_eq!(
            v120x50.breadcrumb_top_margin, 1,
            "120x50: breadcrumb top margin"
        );
        assert_eq!(
            vwide.breadcrumb_top_margin, 1,
            "140x40: breadcrumb top margin"
        );
    }

    #[test]
    fn viewport_plan_composer_footer_spacer_is_zero_only_at_ultra_compact() {
        // arrange
        // act
        let v60 = ViewportPlan::for_viewport(VIEWPORT_60x20);
        let v80 = ViewportPlan::for_viewport(VIEWPORT_80x24);
        let v120 = ViewportPlan::for_viewport(VIEWPORT_120x40);

        // assert — only 60x20 has zero spacer (gap=0); others have 1-row spacer
        assert_eq!(v60.composer_footer_spacer, 0, "60x20: no spacer");
        assert_eq!(v80.composer_footer_spacer, 1, "80x24: 1-row spacer");
        assert_eq!(v120.composer_footer_spacer, 1, "120x40: 1-row spacer");
    }

    #[test]
    fn compact_viewports_classify_as_minimum() {
        // arrange
        // act
        let v80 = ViewportClassification::from_dims(80, 24);
        let v79 = ViewportClassification::from_dims(79, 24);
        let v60 = ViewportClassification::from_dims(60, 20);

        // assert
        assert!(v80.is_compact());
        assert!(v79.is_compact());
        assert!(v60.is_compact());
        assert!(!v80.is_primary());
    }

    #[test]
    fn primary_viewports_classify_as_primary() {
        // arrange
        // act
        let v120x50 = ViewportClassification::from_dims(120, 50);
        let v120x40 = ViewportClassification::from_dims(120, 40);
        let v100x30 = ViewportClassification::from_dims(100, 30);
        let v140x40 = ViewportClassification::from_dims(140, 40);

        // assert
        assert!(v120x50.is_primary());
        assert!(v120x40.is_primary());
        assert!(v100x30.is_primary());
        assert!(v140x40.is_primary());
    }
}
