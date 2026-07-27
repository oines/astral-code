use ratatui::style::Color;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum AstralThemeId {
    #[default]
    Night,
    Day,
    Terminal,
}

impl AstralThemeId {
    pub(crate) const ALL: [Self; 3] = [Self::Night, Self::Day, Self::Terminal];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Night => "Astral Night",
            Self::Day => "Astral Day",
            Self::Terminal => "Terminal",
        }
    }

    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::Night => "Dark palette with violet accents",
            Self::Day => "Light palette for bright environments",
            Self::Terminal => "Use terminal foreground and background",
        }
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "night" | "astral-night" => Some(Self::Night),
            "day" | "astral-day" => Some(Self::Day),
            "terminal" | "native" => Some(Self::Terminal),
            _ => None,
        }
    }
}

/// Astral's focused palette for the ported main-view chrome.
///
/// Keeping the palette in one value makes later theme selection independent
/// from widgets and avoids spreading literal colors through the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AstralTheme {
    pub(crate) bg_base: Color,
    pub(crate) text_primary: Color,
    pub(crate) text_secondary: Color,
    pub(crate) gray_dim: Color,
    pub(crate) gray: Color,
    pub(crate) accent_running: Color,
    pub(crate) accent_error: Color,
    pub(crate) prompt_border: Color,
    pub(crate) prompt_border_active: Color,
}

impl AstralTheme {
    pub(crate) const fn for_id(id: AstralThemeId) -> Self {
        match id {
            AstralThemeId::Night => Self::astral(),
            AstralThemeId::Day => Self::day(),
            AstralThemeId::Terminal => Self::terminal(),
        }
    }

    // Astral deliberately owns a stable palette for this surface instead of
    // inheriting arbitrary terminal colors. Keep this exception centralized.
    #[allow(clippy::disallowed_methods)]
    pub(crate) const fn astral() -> Self {
        Self {
            bg_base: Color::Rgb(20, 20, 20),
            text_primary: Color::Rgb(225, 225, 225),
            text_secondary: Color::Rgb(200, 200, 200),
            gray_dim: Color::Rgb(88, 88, 88),
            gray: Color::Rgb(108, 108, 108),
            accent_running: Color::Rgb(187, 154, 247),
            accent_error: Color::Rgb(247, 118, 142),
            prompt_border: Color::Rgb(50, 50, 55),
            prompt_border_active: Color::Rgb(80, 80, 88),
        }
    }

    #[allow(clippy::disallowed_methods)]
    const fn day() -> Self {
        Self {
            bg_base: Color::Rgb(244, 244, 244),
            text_primary: Color::Rgb(30, 30, 34),
            text_secondary: Color::Rgb(62, 62, 68),
            gray_dim: Color::Rgb(168, 168, 174),
            gray: Color::Rgb(112, 112, 120),
            accent_running: Color::Rgb(108, 76, 184),
            accent_error: Color::Rgb(190, 55, 70),
            prompt_border: Color::Rgb(208, 208, 214),
            prompt_border_active: Color::Rgb(142, 142, 152),
        }
    }

    const fn terminal() -> Self {
        Self {
            bg_base: Color::Reset,
            text_primary: Color::Reset,
            text_secondary: Color::Gray,
            gray_dim: Color::DarkGray,
            gray: Color::Gray,
            accent_running: Color::Magenta,
            accent_error: Color::Red,
            prompt_border: Color::DarkGray,
            prompt_border_active: Color::Gray,
        }
    }
}

impl Default for AstralTheme {
    fn default() -> Self {
        Self::astral()
    }
}
