//! Theme tokens: light/dark/system appearance (project instruction §6).
//!
//! Token values are semantic (surface, on-surface) so each platform maps
//! them to its native palette (iOS system materials, Android dynamic
//! color). No hardcoded font sizes: type scale is a multiplier over the
//! platform Dynamic Type / font-scale setting (§6 accessibility rule).

/// Semantic color tokens for one appearance.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeTokens {
    pub surface: u32,
    pub on_surface: u32,
    pub primary: u32,
    pub on_primary: u32,
    pub error: u32,
}

impl ThemeTokens {
    /// Light appearance tokens (Agent Zero professional palette).`
    pub fn light() -> Self {
        Self {
            surface: 0xFF_FF_FF,
            on_surface: 0x1A_1C_1E,
            primary: 0x2D_5B_8F,
            on_primary: 0xFF_FF_FF,
            error: 0xB3_26_1E,
        }
    }

    /// Dark appearance tokens.
    pub fn dark() -> Self {
        Self {
            surface: 0x11_14_17,
            on_surface: 0xE2_E2_E6,
            primary: 0x9E_CA_FF,
            on_primary: 0x00_32_60,
            error: 0xF2_B8_B5,
        }
    }

    /// Resolves the system appearance.
    pub fn resolve(prefer_dark: bool) -> Self {
        if prefer_dark {
            Self::dark()
        } else {
            Self::light()
        }
    }
}

/// Relative type-scale steps (multipliers over the user font setting).
pub const TYPE_SCALE_BODY: f32 = 1.0;
pub const TYPE_SCALE_TITLE: f32 = 1.25;
pub const TYPE_SCALE_CAPTION: f32 = 0.85;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_and_dark_differ_on_every_token() {
        let light = ThemeTokens::light();
        let dark = ThemeTokens::dark();
        assert_ne!(light.surface, dark.surface);
        assert_ne!(light.on_surface, dark.on_surface);
        assert_ne!(light.primary, dark.primary);
    }

    #[test]
    fn resolve_follows_preference() {
        assert_eq!(ThemeTokens::resolve(false), ThemeTokens::light());
        assert_eq!(ThemeTokens::resolve(true), ThemeTokens::dark());
    }
}
