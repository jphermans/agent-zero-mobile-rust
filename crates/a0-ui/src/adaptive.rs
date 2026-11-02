//! Adaptive window-size classes (project instruction §5).
//!
//! Breakpoints are logical dp/points on the CURRENT window — never physical
//! pixels or device names — so foldables, split-screen and window resize
//! reclassify naturally. Initial breakpoints 600/840 (Material convention);
//! the fields are `pub` so hosts can tune after measured content testing.

/// Which navigation pattern the shell should render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationPattern {
    /// Single column; collapsible drawer; bottom composer.
    BottomBar,
    /// Navigation rail beside content (medium).`
    Rail,
    /// Persistent sidebar; list+detail side by side (expanded).
    Sidebar,
}

/// Logical window size class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowSizeClass {
    /// Below [`COMPACT_MAX_WIDTH_DP`] logical dp.
    Compact,
    /// [`COMPACT_MAX_WIDTH_DP`] up to (excluding) [`EXPANDED_MIN_WIDTH_DP`].
    Medium,
    /// At or above [`EXPANDED_MIN_WIDTH_DP`] logical dp.
    Expanded,
}

/// Upper bound (exclusive) of the compact class, in logical dp.
pub const COMPACT_MAX_WIDTH_DP: f32 = 600.0;
/// Lower bound (inclusive) of the expanded class, in logical dp.
pub const EXPANDED_MIN_WIDTH_DP: f32 = 840.0;

impl WindowSizeClass {
    /// Classifies a window from its logical width in dp.
    pub fn from_width_dp(width_dp: f32) -> Self {
        if width_dp < COMPACT_MAX_WIDTH_DP {
            Self::Compact
        } else if width_dp < EXPANDED_MIN_WIDTH_DP {
            Self::Medium
        } else {
            Self::Expanded
        }
    }

    /// Navigation pattern for the class. Expanded keeps the sidebar
    /// persistent "when sufficient space exists" (§5); medium may use a rail.
    pub fn navigation(self) -> NavigationPattern {
        match self {
            Self::Compact => NavigationPattern::BottomBar,
            Self::Medium => NavigationPattern::Rail,
            Self::Expanded => NavigationPattern::Sidebar,
        }
    }

    /// Whether the conversation list and chat detail render side by side.
    pub fn supports_list_detail(self) -> bool {
        matches!(self, Self::Expanded)
    }

    /// Whether the composer docks at the bottom full-width (compact rule).
    pub fn full_width_composer(self) -> bool {
        matches!(self, Self::Compact)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoints_match_project_spec() {
        assert_eq!(
            WindowSizeClass::from_width_dp(599.0),
            WindowSizeClass::Compact
        );
        assert_eq!(
            WindowSizeClass::from_width_dp(600.0),
            WindowSizeClass::Medium
        );
        assert_eq!(
            WindowSizeClass::from_width_dp(839.0),
            WindowSizeClass::Medium
        );
        assert_eq!(
            WindowSizeClass::from_width_dp(840.0),
            WindowSizeClass::Expanded
        );
    }

    #[test]
    fn navigation_patterns_per_class() {
        assert_eq!(
            WindowSizeClass::Compact.navigation(),
            NavigationPattern::BottomBar
        );
        assert_eq!(
            WindowSizeClass::Medium.navigation(),
            NavigationPattern::Rail
        );
        assert_eq!(
            WindowSizeClass::Expanded.navigation(),
            NavigationPattern::Sidebar
        );
    }

    #[test]
    fn list_detail_only_when_expanded() {
        assert!(!WindowSizeClass::Compact.supports_list_detail());
        assert!(!WindowSizeClass::Medium.supports_list_detail());
        assert!(WindowSizeClass::Expanded.supports_list_detail());
    }

    #[test]
    fn compact_docks_full_width_composer() {
        assert!(WindowSizeClass::Compact.full_width_composer());
        assert!(!WindowSizeClass::Expanded.full_width_composer());
    }
}
