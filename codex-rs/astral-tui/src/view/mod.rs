//! Astral-owned terminal view primitives.
//!
//! The geometry and chrome in this module are derived from Grok Build's pager
//! view at commit `47348d13ec4508dcfe440e34c6d511bb02998fb2`
//! (Apache-2.0). Runtime state and commands remain native app-server v2.

mod block_viewer;
mod chrome;
mod color_support;
mod entry_chrome;
mod entry_content;
mod entry_group;
mod entry_mouse;
mod entry_state;
mod layout;
mod links;
mod markdown_content;
mod mention_menu;
mod modal;
mod plan_review;
mod scrollback;
mod scrollback_search;
mod scrollback_state;
mod selection;
mod slash_menu;
mod theme;
mod transcript;
mod transcript_layout;

pub(crate) use block_viewer::BlockViewerPane;
pub(crate) use chrome::PromptChrome;
pub(crate) use chrome::ShortcutsBar;
pub(crate) use chrome::StatusBar;
pub(crate) use chrome::prompt_height;
pub(crate) use color_support::ColorLevel;
pub(crate) use entry_chrome::EntryChromeState;
pub(crate) use entry_chrome::render_entry_chrome;
pub(crate) use entry_group::EntryGroupSpan;
pub(crate) use entry_mouse::EntryMouseAction;
pub(crate) use entry_mouse::EntryMouseState;
pub(crate) use entry_state::EntryDisplayState;
pub(crate) use layout::AgentViewLayout;
pub(crate) use layout::AgentViewLayoutInput;
pub(crate) use layout::LayoutConfig;
pub(crate) use layout::PaneHeights;
pub(crate) use layout::ScrollbarConfig;
pub(crate) use links::LinkMouseAction;
pub use links::LinkTarget;
pub(crate) use links::TranscriptLink;
pub(crate) use links::VisibleLinks;
pub(crate) use links::append_detected_links;
pub(crate) use mention_menu::MentionMenu;
pub(crate) use modal::InfoModal;
pub(crate) use modal::ModalHeight;
pub(crate) use modal::modal_choice_style;
pub(crate) use modal::render_modal_close_button;
pub(crate) use modal::render_modal_frame_with_geometry;
pub(crate) use plan_review::PlanReviewMouseAction;
pub(crate) use plan_review::PlanReviewMouseState;
pub(crate) use plan_review::PlanReviewPane;
pub(crate) use scrollback::ScrollbackNavigation;
pub(crate) use scrollback::ScrollbackPane;
pub(crate) use scrollback::ScrollbackViewport;
pub(crate) use scrollback::render_follow_indicator;
pub(crate) use scrollback_state::ScrollbackState;
pub(crate) use selection::ScrollbackSelection;
pub(crate) use selection::ScrollbackSelectionAction;
pub(crate) use slash_menu::SlashMenu;
pub(crate) use theme::AstralTheme;
pub(crate) use theme::AstralThemeId;
pub(crate) use transcript::render_committed_block;
pub(crate) use transcript::render_transcript;
pub(crate) use transcript_layout::TranscriptLayout;
