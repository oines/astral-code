//! Shared Grok-style chrome for Astral overlays.
//!
//! It owns geometry and common input routing; presenters retain content,
//! scrolling, forms, and protocol semantics.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;

mod render;

const MIN_MODAL_WIDTH: u16 = 20;
const MIN_MODAL_HEIGHT: u16 = 6;
const SHORTCUT_SEPARATOR: &str = "  |  ";

/// Whether the chrome floats over a fullscreen surface or fills an area owned
/// by an inline/prompt presenter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ModalPresentation {
    #[default]
    Popup,
    Embedded,
}

/// Responsive size policy for a popup. Embedded windows always fill their
/// caller-provided area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModalSizing {
    pub width_fraction: f32,
    pub maximum_width: u16,
    pub minimum_width: u16,
    pub vertical_margin: u16,
    pub horizontal_padding: u16,
    pub vertical_padding: u16,
    pub footer_rows: u16,
}

impl ModalSizing {
    /// Large detail/form window matching Grok's default proportions.
    pub const fn large() -> Self {
        Self {
            width_fraction: 0.9,
            maximum_width: 140,
            minimum_width: 60,
            vertical_margin: 7,
            horizontal_padding: 2,
            vertical_padding: 2,
            footer_rows: 2,
        }
    }
    /// Medium picker window matching Grok's list proportions.
    pub const fn medium() -> Self {
        Self {
            width_fraction: 0.6,
            maximum_width: 120,
            minimum_width: 44,
            vertical_margin: 4,
            horizontal_padding: 2,
            vertical_padding: 1,
            footer_rows: 2,
        }
    }
    /// Maximize usable space without introducing a separate layout path.
    pub const fn compact(mut self) -> Self {
        self.vertical_margin = 0;
        self.horizontal_padding = 1;
        self.vertical_padding = 0;
        self
    }
}

/// One footer hint. Action shortcuts produce an outcome when clicked; passive
/// hints remain hoverable but leave clicks to the content presenter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalShortcut<'a> {
    label: &'a str,
    action: Option<usize>,
}

impl<'a> ModalShortcut<'a> {
    pub const fn action(id: usize, label: &'a str) -> Self {
        Self {
            label,
            action: Some(id),
        }
    }
    pub const fn hint(label: &'a str) -> Self {
        Self {
            label,
            action: None,
        }
    }
}

/// Per-frame chrome configuration. The presenter rebuilds this from its own
/// typed state and then paints its content into [`ModalContentArea::content`].
#[derive(Debug, Clone, Copy)]
pub struct ModalWindowConfig<'a> {
    title: &'a str,
    tabs: &'a [&'a str],
    shortcuts: &'a [ModalShortcut<'a>],
    sizing: ModalSizing,
    presentation: ModalPresentation,
}

impl<'a> ModalWindowConfig<'a> {
    pub const fn new(title: &'a str) -> Self {
        Self {
            title,
            tabs: &[],
            shortcuts: &[],
            sizing: ModalSizing::large(),
            presentation: ModalPresentation::Popup,
        }
    }
    pub const fn with_tabs(mut self, tabs: &'a [&'a str]) -> Self {
        self.tabs = tabs;
        self
    }
    pub const fn with_shortcuts(mut self, shortcuts: &'a [ModalShortcut<'a>]) -> Self {
        self.shortcuts = shortcuts;
        self
    }
    pub const fn with_sizing(mut self, sizing: ModalSizing) -> Self {
        self.sizing = sizing;
        self
    }
    pub const fn with_presentation(mut self, presentation: ModalPresentation) -> Self {
        self.presentation = presentation;
        self
    }
}

/// Theme-independent style roles for modal chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalRenderStyle {
    pub border: Style,
    pub selection: Style,
    pub hover: Style,
}

impl Default for ModalRenderStyle {
    fn default() -> Self {
        Self {
            border: Style::default().dim(),
            selection: Style::default().cyan().bold(),
            hover: Style::default().reversed(),
        }
    }
}

/// Rectangles reserved for the caller's content and the chrome footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalContentArea {
    pub content: Rect,
    pub footer: Rect,
    pub inner: Rect,
}

/// Result of routing one input event through the common chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalOutcome {
    Handled,
    CloseRequested,
    TabChanged(usize),
    ShortcutActivated(usize),
    Unhandled,
}

#[derive(Debug, Clone)]
struct ShortcutHit {
    rect: Rect,
    index: usize,
    action: Option<usize>,
}

/// Retained geometry and hover state for one modal presenter.
#[derive(Debug, Clone)]
pub struct ModalWindow {
    style: ModalRenderStyle,
    rendered_area: Option<Rect>,
    close_rect: Option<Rect>,
    close_hovered: bool,
    active_tab: usize,
    tab_rects: Vec<Option<Rect>>,
    shortcut_hits: Vec<ShortcutHit>,
    hovered_shortcut: Option<usize>,
}

impl Default for ModalWindow {
    fn default() -> Self {
        Self::new(ModalRenderStyle::default())
    }
}

impl ModalWindow {
    pub fn new(style: ModalRenderStyle) -> Self {
        Self {
            style,
            rendered_area: None,
            close_rect: None,
            close_hovered: false,
            active_tab: 0,
            tab_rects: Vec::new(),
            shortcut_hits: Vec::new(),
            hovered_shortcut: None,
        }
    }
    pub fn active_tab(&self) -> usize {
        self.active_tab
    }
    pub fn set_active_tab(&mut self, active_tab: usize) {
        self.active_tab = active_tab;
    }
    pub fn handle_key_event(&mut self, key: KeyEvent) -> ModalOutcome {
        if key.kind != KeyEventKind::Release && key.code == KeyCode::Esc {
            ModalOutcome::CloseRequested
        } else {
            ModalOutcome::Unhandled
        }
    }

    pub fn handle_mouse_event(&mut self, mouse: MouseEvent) -> ModalOutcome {
        let on_close = self.close_rect.is_some_and(|rect| contains(rect, mouse));
        let on_tab =
            self.tab_rects.iter().enumerate().find_map(|(index, rect)| {
                rect.filter(|rect| contains(*rect, mouse)).map(|_| index)
            });
        let on_shortcut = self
            .shortcut_hits
            .iter()
            .find(|hit| contains(hit.rect, mouse));
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if on_close {
                    return ModalOutcome::CloseRequested;
                }
                if let Some(tab) = on_tab {
                    if tab != self.active_tab {
                        self.active_tab = tab;
                        return ModalOutcome::TabChanged(tab);
                    }
                    return ModalOutcome::Handled;
                }
                if let Some(action) = on_shortcut.and_then(|hit| hit.action) {
                    return ModalOutcome::ShortcutActivated(action);
                }
                if self
                    .rendered_area
                    .is_some_and(|area| !contains(area, mouse))
                {
                    return ModalOutcome::CloseRequested;
                }
                ModalOutcome::Unhandled
            }
            MouseEventKind::Moved => {
                let shortcut = on_shortcut.map(|hit| hit.index);
                let hover_changed =
                    self.close_hovered != on_close || self.hovered_shortcut != shortcut;
                self.close_hovered = on_close;
                self.hovered_shortcut = shortcut;
                if on_close || on_tab.is_some() || shortcut.is_some() || hover_changed {
                    ModalOutcome::Handled
                } else {
                    ModalOutcome::Unhandled
                }
            }
            _ => ModalOutcome::Unhandled,
        }
    }
    fn clear_geometry(&mut self) {
        self.rendered_area = None;
        self.close_rect = None;
        self.tab_rects.clear();
        self.shortcut_hits.clear();
    }
}

fn contains(area: Rect, mouse: MouseEvent) -> bool {
    mouse.column >= area.x
        && mouse.column < area.right()
        && mouse.row >= area.y
        && mouse.row < area.bottom()
}

#[cfg(test)]
#[path = "modal_tests.rs"]
mod tests;
