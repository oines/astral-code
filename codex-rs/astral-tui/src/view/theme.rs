use ratatui::style::Color;

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
}

impl Default for AstralTheme {
    fn default() -> Self {
        Self::astral()
    }
}
