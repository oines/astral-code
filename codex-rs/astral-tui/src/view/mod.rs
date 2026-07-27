//! Astral-owned terminal view primitives.
//!
//! The geometry and chrome in this module are derived from Grok Build's pager
//! view at commit `47348d13ec4508dcfe440e34c6d511bb02998fb2`
//! (Apache-2.0). Runtime state and commands remain native app-server v2.

mod chrome;
mod layout;
mod slash_menu;
mod theme;

pub(crate) use chrome::PromptChrome;
pub(crate) use chrome::ShortcutsBar;
pub(crate) use chrome::StatusBar;
pub(crate) use layout::AgentViewLayout;
pub(crate) use layout::AgentViewLayoutInput;
pub(crate) use layout::LayoutConfig;
pub(crate) use layout::PaneHeights;
pub(crate) use layout::ScrollbarConfig;
pub(crate) use slash_menu::SlashMenu;
pub(crate) use theme::AstralTheme;
