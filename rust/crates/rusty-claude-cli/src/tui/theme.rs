//! `tui::theme` — user-selectable color themes. Reads `theme` from
//! `.claw.json` (via `read_theme_from_cwd`) and swaps the accent/dim/
//! success/error palette used by every TUI widget.
//!
//! Presets track Claude Code's four shipped themes (`dark` / `light` /
//! `dark-daltonized` / `light-daltonized`) plus an `evil-cyan` default
//! for the Evil Claude rebrand.

use ratatui::style::Color;

/// Named theme identifier read from `.claw.json:theme`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeName {
    EvilCyan,
    Dark,
    Light,
    DarkDaltonized,
    LightDaltonized,
}

impl ThemeName {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EvilCyan => "evil-cyan",
            Self::Dark => "dark",
            Self::Light => "light",
            Self::DarkDaltonized => "dark-daltonized",
            Self::LightDaltonized => "light-daltonized",
        }
    }

    /// Parse the value stored in `.claw.json:theme`. Unknown strings fall
    /// back to `EvilCyan`.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw {
            "dark" => Self::Dark,
            "light" => Self::Light,
            "dark-daltonized" => Self::DarkDaltonized,
            "light-daltonized" => Self::LightDaltonized,
            _ => Self::EvilCyan,
        }
    }
}

impl Default for ThemeName {
    fn default() -> Self {
        Self::EvilCyan
    }
}

/// Palette used by widgets. Read from the active `ThemeName` via
/// [`Theme::for_name`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub accent: Color,
    pub dim: Color,
    pub success: Color,
    pub warn: Color,
    pub error: Color,
    pub diff_add_bg: Color,
    pub diff_del_bg: Color,
}

impl Theme {
    #[must_use]
    pub const fn for_name(name: ThemeName) -> Self {
        match name {
            ThemeName::EvilCyan => Self {
                accent: Color::Rgb(0, 210, 210),
                dim: Color::DarkGray,
                success: Color::Green,
                warn: Color::Yellow,
                error: Color::Red,
                diff_add_bg: Color::Rgb(0, 40, 0),
                diff_del_bg: Color::Rgb(40, 0, 0),
            },
            ThemeName::Dark => Self {
                accent: Color::Rgb(204, 120, 56), // brand orange
                dim: Color::DarkGray,
                success: Color::Green,
                warn: Color::Yellow,
                error: Color::Red,
                diff_add_bg: Color::Rgb(0, 40, 0),
                diff_del_bg: Color::Rgb(40, 0, 0),
            },
            ThemeName::Light => Self {
                accent: Color::Rgb(184, 92, 31), // darker orange for light bg
                dim: Color::Gray,
                success: Color::Green,
                warn: Color::Yellow,
                error: Color::Red,
                diff_add_bg: Color::Rgb(220, 255, 220),
                diff_del_bg: Color::Rgb(255, 220, 220),
            },
            ThemeName::DarkDaltonized => Self {
                accent: Color::Rgb(0, 210, 210),
                dim: Color::DarkGray,
                // Blue-yellow bands are dichromat-safe.
                success: Color::Rgb(0, 176, 239),
                warn: Color::Rgb(255, 205, 40),
                error: Color::Rgb(238, 68, 68),
                diff_add_bg: Color::Rgb(20, 40, 80),
                diff_del_bg: Color::Rgb(80, 30, 20),
            },
            ThemeName::LightDaltonized => Self {
                accent: Color::Rgb(0, 116, 178),
                dim: Color::Gray,
                success: Color::Rgb(0, 116, 178),
                warn: Color::Rgb(230, 159, 0),
                error: Color::Rgb(213, 94, 0),
                diff_add_bg: Color::Rgb(210, 230, 250),
                diff_del_bg: Color::Rgb(250, 220, 210),
            },
        }
    }
}

/// Walk cwd → parents looking for `.claw.json` with a `"theme"` field.
/// Returns the parsed `ThemeName`, or `EvilCyan` when nothing is set.
#[must_use]
pub fn read_theme_from_cwd() -> ThemeName {
    let Ok(cwd) = std::env::current_dir() else {
        return ThemeName::default();
    };
    let mut cursor: Option<&std::path::Path> = Some(cwd.as_path());
    while let Some(dir) = cursor {
        let candidate = dir.join(".claw.json");
        if candidate.is_file() {
            if let Ok(contents) = std::fs::read_to_string(&candidate) {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents) {
                    if let Some(theme) = value.get("theme").and_then(|v| v.as_str()) {
                        return ThemeName::parse(theme);
                    }
                }
            }
        }
        cursor = dir.parent();
    }
    ThemeName::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_recognized_names() {
        assert_eq!(ThemeName::parse("dark"), ThemeName::Dark);
        assert_eq!(ThemeName::parse("light"), ThemeName::Light);
        assert_eq!(ThemeName::parse("dark-daltonized"), ThemeName::DarkDaltonized);
        assert_eq!(ThemeName::parse("light-daltonized"), ThemeName::LightDaltonized);
        assert_eq!(ThemeName::parse("evil-cyan"), ThemeName::EvilCyan);
    }

    #[test]
    fn parse_unknown_falls_back_to_evil_cyan() {
        assert_eq!(ThemeName::parse("plum"), ThemeName::EvilCyan);
        assert_eq!(ThemeName::parse(""), ThemeName::EvilCyan);
    }

    #[test]
    fn evil_cyan_uses_rgb_0_210_210() {
        let theme = Theme::for_name(ThemeName::EvilCyan);
        assert_eq!(theme.accent, Color::Rgb(0, 210, 210));
    }

    #[test]
    fn all_themes_have_distinct_accents() {
        let evil = Theme::for_name(ThemeName::EvilCyan).accent;
        let dark = Theme::for_name(ThemeName::Dark).accent;
        let light = Theme::for_name(ThemeName::Light).accent;
        assert_ne!(evil, dark);
        assert_ne!(dark, light);
    }
}
