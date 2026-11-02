//! `a0-ui` — shared adaptive-UI model for the native shells.
//!
//! The iOS (SwiftUI) and Android (Jetpack Compose) frontends render their
//! own native components; this crate holds the shared, tested decision
//! logic both shells consume over the FFI boundary:
//!
//! - [`adaptive`]: logical window-size classes (600/840 dp) and the
//!   navigation pattern per class (project instruction §5),
//! - [`theme`]: semantic light/dark tokens and the relative type scale
//!   (no hardcoded font sizes, §6).

pub mod adaptive;
pub mod theme;

pub use adaptive::{NavigationPattern, WindowSizeClass, COMPACT_MAX_WIDTH_DP, EXPANDED_MIN_WIDTH_DP};
pub use theme::{ThemeTokens, TYPE_SCALE_BODY, TYPE_SCALE_CAPTION, TYPE_SCALE_TITLE};
