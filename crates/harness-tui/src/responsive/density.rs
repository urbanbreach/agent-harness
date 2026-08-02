//! Density mode selection bridge for the responsive shard.
//!
//! Maps viewport dimensions to the layout-owned `SessionResponsiveMode` and
//! geometry targets to the theme-owned `SpacingDensity` without introducing a
//! duplicate selection table. Both functions route through the canonical
//! owners (`layout::session_responsive_mode`, token families) so there is a
//! single source of truth per concern.

use ratatui::layout::Rect;

use crate::layout::{session_responsive_mode, SessionResponsiveMode};
use crate::theme::{ShellGeometryTarget, SpacingDensity, Theme};

pub fn density_for_viewport(theme: &Theme, cols: u16, rows: u16) -> SessionResponsiveMode {
    session_responsive_mode(
        Rect::new(0, 0, cols, rows),
        theme.live_shell_layout(cols, rows),
    )
}

pub fn spacing_density_for(theme: &Theme, target: ShellGeometryTarget) -> SpacingDensity {
    theme
        .token_families()
        .semantic
        .density
        .select(target)
        .density
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_bridge_reports_dense_at_dense_cut() {
        // arrange
        let theme = Theme::default();

        // act
        // assert
        assert_eq!(
            density_for_viewport(&theme, 60, 18),
            SessionResponsiveMode::Dense
        );
        assert_eq!(
            density_for_viewport(&theme, 80, 24),
            SessionResponsiveMode::CompactMinimum
        );
        assert_eq!(
            density_for_viewport(&theme, 120, 40),
            SessionResponsiveMode::Primary
        );
    }

    #[test]
    fn spacing_density_covers_all_geometry_targets() {
        // arrange
        let theme = Theme::default();

        // act
        // assert
        assert_eq!(
            spacing_density_for(&theme, ShellGeometryTarget::Minimum),
            SpacingDensity::Compact
        );
        assert_eq!(
            spacing_density_for(&theme, ShellGeometryTarget::Split),
            SpacingDensity::Standard
        );
        assert_eq!(
            spacing_density_for(&theme, ShellGeometryTarget::Primary),
            SpacingDensity::Roomy
        );
    }
}
