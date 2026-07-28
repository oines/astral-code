// Derived from Grok Build's unified ScrollbackState at commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Astral keeps its app-server projection, while this type owns all interactive
// transcript state so navigation, folding, selection, and pointer input cannot
// disagree about the active viewport.

use std::time::Instant;

use astral_tui_scrollback::DisplayMode;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::conversation::TranscriptTurn;

use super::AstralTheme;
use super::EntryDisplayState;
use super::EntryMouseAction;
use super::EntryMouseState;
use super::ScrollbackNavigation;
use super::ScrollbackSelection;
use super::ScrollbackSelectionAction;
use super::ScrollbackViewport;
use super::TranscriptLayout;
use super::transcript::TranscriptSection;

/// Unified owner for Astral's interactive transcript.
///
/// The projected transcript remains immutable input. Everything that can move
/// or reinterpret the visible transcript lives here and uses the geometry from
/// the last rendered frame.
#[derive(Debug, Default)]
pub(crate) struct ScrollbackState {
    navigation: ScrollbackNavigation,
    display: EntryDisplayState,
    selection: ScrollbackSelection,
    pointer: EntryMouseState,
}

impl ScrollbackState {
    pub(crate) fn observe_entries(&mut self, turns: &[TranscriptTurn]) {
        self.display.observe(turns);
    }

    pub(crate) fn display(&self) -> &EntryDisplayState {
        &self.display
    }

    pub(crate) fn prepare(
        &mut self,
        layout: &TranscriptLayout,
        width: u16,
        viewport_lines: usize,
    ) -> ScrollbackViewport {
        self.navigation.prepare(layout, width, viewport_lines)
    }

    pub(crate) fn observe_frame(
        &mut self,
        layout: &TranscriptLayout,
        viewport: ScrollbackViewport,
        area: Rect,
        buffer: &mut Buffer,
        theme: AstralTheme,
    ) {
        self.pointer.observe(layout, viewport, area);
        self.selection.render(layout, viewport, area, buffer, theme);
    }

    pub(crate) fn clear_frame(&mut self) {
        self.pointer.clear_frame();
    }

    pub(crate) fn scroll_up(&mut self, lines: usize) {
        self.selection.clear_persistent();
        self.navigation.scroll_up(lines);
    }

    pub(crate) fn scroll_down(&mut self, lines: usize) {
        self.selection.clear_persistent();
        self.navigation.scroll_down(lines);
    }

    pub(crate) fn page_up(&mut self) {
        self.selection.clear_persistent();
        let viewport = self.navigation.page_up();
        if self.display.is_focused() {
            self.select_viewport_edge(viewport, /* prefer_top */ true);
        }
    }

    pub(crate) fn page_down(&mut self) {
        self.selection.clear_persistent();
        let viewport = self.navigation.page_down();
        if self.display.is_focused() {
            self.select_viewport_edge(viewport, /* prefer_top */ false);
        }
    }

    pub(crate) fn scroll_to_bottom(&mut self) {
        self.selection.clear_persistent();
        self.navigation.scroll_to_bottom();
    }

    pub(crate) fn distance_from_bottom(&self) -> usize {
        self.navigation.distance_from_bottom()
    }

    pub(crate) fn focus_scrollback(&mut self) -> bool {
        self.display.focus_scrollback()
    }

    pub(crate) fn focus_prompt(&mut self) {
        self.display.focus_prompt();
    }

    pub(crate) fn is_focused(&self) -> bool {
        self.display.is_focused()
    }

    pub(crate) fn selected_id(&self) -> Option<&str> {
        self.display.selected_id()
    }

    pub(crate) fn selected_mode(&self) -> Option<DisplayMode> {
        self.display.selected_mode()
    }

    pub(crate) fn selected_is_group_header(&self) -> bool {
        self.display.selected_is_group_header()
    }

    pub(crate) fn selected_is_foldable(&self) -> bool {
        self.display.selected_is_foldable()
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let previous = self.display.selected_id().map(str::to_owned);
        let selected = self.display.move_selection(delta);
        if selected != previous {
            if let Some(selected) = selected {
                self.navigation.reveal_entry(&selected);
            }
        } else if delta > 0 {
            self.navigation.scroll_to_bottom();
        }
    }

    pub(crate) fn toggle_selected(&mut self) {
        self.display.toggle_selected();
        self.reveal_selected();
    }

    pub(crate) fn expand_selected(&mut self) {
        self.display.expand_selected();
        self.reveal_selected();
    }

    pub(crate) fn collapse_selected(&mut self) {
        self.display.collapse_selected();
        self.reveal_selected();
    }

    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<String> {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.scroll_up(/* lines */ 3);
                return None;
            }
            MouseEventKind::ScrollDown => {
                self.scroll_down(/* lines */ 3);
                return None;
            }
            _ => {}
        }

        let entry_action = self.pointer.handle_mouse(mouse);
        match self.selection.handle_mouse(mouse) {
            ScrollbackSelectionAction::ScrollUp => self.scroll_up(/* lines */ 1),
            ScrollbackSelectionAction::ScrollDown => self.scroll_down(/* lines */ 1),
            ScrollbackSelectionAction::Copy(text) => return Some(text),
            ScrollbackSelectionAction::Ignored | ScrollbackSelectionAction::Redraw => {}
        }
        match entry_action {
            EntryMouseAction::Select(item_id) => {
                self.display.select(&item_id);
            }
            EntryMouseAction::Toggle(item_id) => {
                if self.display.select(&item_id) {
                    self.display.toggle_selected();
                    self.navigation.reveal_entry(&item_id);
                }
            }
            EntryMouseAction::ToggleGroup(item_id) => {
                if self.display.select(&item_id) {
                    self.display.toggle_group(&item_id);
                    self.navigation.reveal_entry(&item_id);
                }
            }
            EntryMouseAction::Ignored => {}
        }
        None
    }

    pub(crate) fn clear_selection(&mut self) -> bool {
        self.selection.clear()
    }

    pub(crate) fn selection_expiry(&self) -> Option<Instant> {
        self.selection.expiry()
    }

    pub(crate) fn expire_selection(&mut self, now: Instant) -> bool {
        self.selection.expire_if_due(now)
    }

    fn reveal_selected(&mut self) {
        if let Some(item_id) = self.display.selected_id().map(str::to_owned) {
            self.navigation.reveal_entry(&item_id);
        }
    }

    fn select_viewport_edge(&mut self, viewport: ScrollbackViewport, prefer_top: bool) {
        let sections = self
            .navigation
            .sections()
            .iter()
            .filter(|section| section_overlaps_viewport(section, viewport));
        let selected = if prefer_top {
            sections
                .filter(|section| self.display.contains(section.item_id.as_str()))
                .next()
        } else {
            sections
                .filter(|section| self.display.contains(section.item_id.as_str()))
                .next_back()
        };
        if let Some(selected) = selected {
            self.display.select(&selected.item_id);
        }
    }
}

fn section_overlaps_viewport(section: &TranscriptSection, viewport: ScrollbackViewport) -> bool {
    section.lines.start < viewport.end_visible_line
        && section.lines.end > viewport.first_visible_line
}
