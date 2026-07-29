use astral_tui_scrollback::MarkdownSyntaxTheme;
use ratatui::style::Color;

use super::color_support::ColorLevel;
use super::color_support::quantize_color;

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

    pub(crate) fn config_name(self) -> &'static str {
        match self {
            Self::Night => "astral-night",
            Self::Day => "astral-day",
            Self::Terminal => "terminal",
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
    pub(crate) path: Color,
    pub(crate) diff_gutter: Color,
    pub(crate) diff_insert_foreground: Color,
    pub(crate) diff_delete_foreground: Color,
    pub(crate) diff_insert_background: Option<Color>,
    pub(crate) diff_delete_background: Option<Color>,
    pub(crate) diff_equal_foreground: Color,
    pub(crate) syntax_theme: MarkdownSyntaxTheme,
    pub(crate) panel_background: Color,
    pub(crate) panel_selected: Color,
    pub(crate) prompt_border: Color,
    pub(crate) prompt_border_active: Color,
    pub(crate) prompt_selection_background: Color,
    pub(crate) prompt_selection_foreground: Color,
    pub(crate) selection_border: Color,
}

impl AstralTheme {
    pub(crate) const fn for_id(id: AstralThemeId) -> Self {
        match id {
            AstralThemeId::Night => Self::astral(),
            AstralThemeId::Day => Self::day(),
            AstralThemeId::Terminal => Self::terminal(),
        }
    }

    pub(crate) fn for_color_level(id: AstralThemeId, level: ColorLevel) -> Self {
        Self::for_id(id).quantized(level)
    }

    fn quantized(self, level: ColorLevel) -> Self {
        let quantize = |color| quantize_color(color, level);
        Self {
            bg_base: quantize(self.bg_base),
            text_primary: quantize(self.text_primary),
            text_secondary: quantize(self.text_secondary),
            gray_dim: quantize(self.gray_dim),
            gray: quantize(self.gray),
            accent_running: quantize(self.accent_running),
            accent_error: quantize(self.accent_error),
            path: quantize(self.path),
            diff_gutter: quantize(self.diff_gutter),
            diff_insert_foreground: quantize(self.diff_insert_foreground),
            diff_delete_foreground: quantize(self.diff_delete_foreground),
            diff_insert_background: self.diff_insert_background.map(quantize),
            diff_delete_background: self.diff_delete_background.map(quantize),
            diff_equal_foreground: quantize(self.diff_equal_foreground),
            syntax_theme: self.syntax_theme,
            panel_background: quantize(self.panel_background),
            panel_selected: quantize(self.panel_selected),
            prompt_border: quantize(self.prompt_border),
            prompt_border_active: quantize(self.prompt_border_active),
            prompt_selection_background: quantize(self.prompt_selection_background),
            prompt_selection_foreground: quantize(self.prompt_selection_foreground),
            selection_border: quantize(self.selection_border),
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
            path: Color::Rgb(255, 158, 100),
            diff_gutter: Color::Rgb(108, 108, 108),
            diff_insert_foreground: Color::Rgb(158, 206, 106),
            diff_delete_foreground: Color::Rgb(247, 118, 142),
            diff_insert_background: Some(Color::Rgb(6, 56, 6)),
            diff_delete_background: Some(Color::Rgb(66, 14, 20)),
            diff_equal_foreground: Color::Rgb(108, 108, 108),
            syntax_theme: MarkdownSyntaxTheme::Night,
            panel_background: Color::Rgb(27, 27, 29),
            panel_selected: Color::Rgb(45, 43, 50),
            prompt_border: Color::Rgb(50, 50, 55),
            prompt_border_active: Color::Rgb(80, 80, 88),
            prompt_selection_background: Color::Rgb(49, 62, 115),
            prompt_selection_foreground: Color::Rgb(192, 202, 245),
            selection_border: Color::Rgb(80, 80, 88),
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
            path: Color::Rgb(195, 105, 30),
            diff_gutter: Color::Rgb(118, 118, 118),
            diff_insert_foreground: Color::Rgb(55, 142, 35),
            diff_delete_foreground: Color::Rgb(205, 48, 72),
            diff_insert_background: Some(Color::Rgb(218, 242, 220)),
            diff_delete_background: Some(Color::Rgb(245, 218, 222)),
            diff_equal_foreground: Color::Rgb(118, 118, 118),
            syntax_theme: MarkdownSyntaxTheme::Day,
            panel_background: Color::Rgb(235, 235, 238),
            panel_selected: Color::Rgb(218, 215, 226),
            prompt_border: Color::Rgb(208, 208, 214),
            prompt_border_active: Color::Rgb(142, 142, 152),
            prompt_selection_background: Color::Rgb(49, 62, 115),
            prompt_selection_foreground: Color::Rgb(192, 202, 245),
            selection_border: Color::Rgb(142, 142, 152),
        }
    }

    #[allow(clippy::disallowed_methods)]
    const fn terminal() -> Self {
        Self {
            bg_base: Color::Reset,
            text_primary: Color::Reset,
            text_secondary: Color::Gray,
            gray_dim: Color::DarkGray,
            gray: Color::Gray,
            accent_running: Color::Magenta,
            accent_error: Color::Red,
            path: Color::Cyan,
            diff_gutter: Color::Reset,
            diff_insert_foreground: Color::Green,
            diff_delete_foreground: Color::Red,
            diff_insert_background: None,
            diff_delete_background: None,
            diff_equal_foreground: Color::Reset,
            syntax_theme: MarkdownSyntaxTheme::Terminal,
            panel_background: Color::Reset,
            panel_selected: Color::DarkGray,
            prompt_border: Color::DarkGray,
            prompt_border_active: Color::Gray,
            prompt_selection_background: Color::Rgb(49, 62, 115),
            prompt_selection_foreground: Color::Rgb(192, 202, 245),
            selection_border: Color::Gray,
        }
    }
}

impl Default for AstralTheme {
    fn default() -> Self {
        Self::astral()
    }
}
