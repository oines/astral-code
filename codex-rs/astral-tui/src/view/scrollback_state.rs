// Derived from Grok Build's unified ScrollbackState at commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Astral keeps its app-server projection, while this type owns all interactive
// transcript state so navigation, folding, selection, and pointer input cannot
// disagree about the active viewport.

use std::path::Path;
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
use super::VisibleLinks;
use super::scrollback_search::ScrollbackSearch;
use super::transcript::TranscriptSection;

#[path = "scrollback_state_links.rs"]
mod links;
#[path = "scrollback_state_search.rs"]
mod search;

pub(crate) use links::ScrollbackMouseAction;

pub(crate) struct ScrollbackFrame<'a> {
    pub(crate) layout: &'a TranscriptLayout,
    pub(crate) viewport: ScrollbackViewport,
    pub(crate) area: Rect,
    pub(crate) scrollbar_area: Rect,
    pub(crate) cwd: &'a Path,
}

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
    turn_prompts: Vec<String>,
    response_anchors: Vec<String>,
    scrollbar: Option<Rect>,
    scrollbar_dragging: bool,
    hovered: Option<String>,
    search: Option<ScrollbackSearch>,
    pending_search_target: Option<(String, usize)>,
    links: VisibleLinks,
}

impl ScrollbackState {
    pub(crate) fn observe_entries(&mut self, turns: &[TranscriptTurn]) {
        self.display.observe(turns);
        self.turn_prompts = turns.iter().filter_map(turn_prompt_id).collect::<Vec<_>>();
        self.response_anchors = turns
            .iter()
            .filter_map(response_anchor_id)
            .collect::<Vec<_>>();
        if self
            .hovered
            .as_deref()
            .is_some_and(|entry_id| !self.display.contains(entry_id))
        {
            self.hovered = None;
        }
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
        let mut viewport = self.navigation.prepare(layout, width, viewport_lines);
        if let Some((entry_id, line_in_entry)) = self.pending_search_target.take() {
            if self
                .navigation
                .reveal_entry_line(layout, &entry_id, line_in_entry)
            {
                viewport = self.navigation.viewport();
            } else {
                self.pending_search_target = Some((entry_id, line_in_entry));
            }
        }
        viewport
    }

    pub(crate) fn observe_frame(
        &mut self,
        frame: ScrollbackFrame<'_>,
        buffer: &mut Buffer,
        theme: AstralTheme,
    ) {
        self.pointer
            .observe(frame.layout, frame.viewport, frame.area);
        self.scrollbar = frame
            .viewport
            .needs_scrollbar()
            .then_some(frame.scrollbar_area);
        self.selection
            .render(frame.layout, frame.viewport, frame.area, buffer, theme);
        self.links
            .rebuild(frame.layout, frame.viewport, frame.area, frame.cwd);
        self.links.paint(buffer, theme);
    }

    pub(crate) fn clear_frame(&mut self) {
        self.pointer.clear_frame();
        self.scrollbar = None;
        self.scrollbar_dragging = false;
        self.hovered = None;
        self.links.clear_frame();
    }

    pub(crate) fn scroll_up(&mut self, lines: usize) {
        self.pointer.cancel_gesture();
        self.scrollbar_dragging = false;
        self.hovered = None;
        self.selection.clear_persistent();
        self.navigation.scroll_up(lines);
    }

    pub(crate) fn scroll_down(&mut self, lines: usize) {
        self.pointer.cancel_gesture();
        self.scrollbar_dragging = false;
        self.hovered = None;
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

    pub(crate) fn half_page_up(&mut self) {
        self.selection.clear_persistent();
        self.navigation.half_page_up();
    }

    pub(crate) fn half_page_down(&mut self) {
        self.selection.clear_persistent();
        self.navigation.half_page_down();
    }

    pub(crate) fn goto_top(&mut self) {
        self.selection.clear_persistent();
        self.navigation.scroll_to_top();
        self.display.select_first();
    }

    pub(crate) fn goto_bottom(&mut self) {
        self.selection.clear_persistent();
        self.navigation.scroll_to_bottom();
        self.display.select_last();
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
        self.search = None;
        self.links.clear_highlight();
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

    pub(crate) fn selected_is_raw(&self) -> bool {
        self.display.selected_is_raw()
    }

    pub(crate) fn is_raw_entry(&self, entry_id: &str) -> bool {
        self.display.is_raw_entry(entry_id)
    }

    pub(crate) fn selected_supports_copy(&self) -> bool {
        self.display.selected_supports_copy()
    }

    pub(crate) fn selected_copy_meta_label(&self) -> Option<&'static str> {
        self.display.selected_copy_meta_label()
    }

    pub(crate) fn hovered_id(&self) -> Option<&str> {
        self.hovered.as_deref()
    }

    pub(crate) fn hovered_mode(&self) -> Option<DisplayMode> {
        let hovered = self.hovered.as_deref()?;
        self.display
            .is_foldable(hovered)
            .then(|| self.display.mode(hovered))
            .flatten()
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

    pub(crate) fn next_turn(&mut self) {
        let selected = self.display.selected_id();
        let current = selected.and_then(turn_id_from_entry).and_then(|turn_id| {
            self.turn_prompts
                .iter()
                .position(|prompt| turn_id_from_entry(prompt) == Some(turn_id))
        });
        let target = current
            .map(|index| (index + 1).min(self.turn_prompts.len().saturating_sub(1)))
            .unwrap_or_default();
        self.select_and_snap(self.turn_prompts.get(target).cloned());
    }

    pub(crate) fn previous_turn(&mut self) {
        let selected = self.display.selected_id();
        let current = selected.and_then(turn_id_from_entry).and_then(|turn_id| {
            self.turn_prompts
                .iter()
                .position(|prompt| turn_id_from_entry(prompt) == Some(turn_id))
        });
        let target = current.and_then(|index| {
            let prompt = self.turn_prompts.get(index)?;
            if selected == Some(prompt.as_str()) {
                index.checked_sub(1)
            } else {
                Some(index)
            }
        });
        self.select_and_snap(target.and_then(|index| self.turn_prompts.get(index).cloned()));
    }

    pub(crate) fn next_response(&mut self) {
        let viewport_top = self.navigation.viewport().first_visible_line;
        let target = self
            .response_anchors
            .iter()
            .filter_map(|entry_id| {
                self.navigation
                    .entry_top(entry_id)
                    .filter(|top| *top > viewport_top)
                    .map(|top| (top, entry_id))
            })
            .min_by_key(|(top, _)| *top)
            .map(|(_, entry_id)| entry_id.clone());
        self.select_and_snap(target);
    }

    pub(crate) fn previous_response(&mut self) {
        let viewport_top = self.navigation.viewport().first_visible_line;
        let target = self
            .response_anchors
            .iter()
            .filter_map(|entry_id| {
                self.navigation
                    .entry_top(entry_id)
                    .filter(|top| *top < viewport_top)
                    .map(|top| (top, entry_id))
            })
            .max_by_key(|(top, _)| *top)
            .map(|(_, entry_id)| entry_id.clone());
        self.select_and_snap(target);
    }

    pub(crate) fn toggle_selected(&mut self) {
        self.display.toggle_selected();
        self.reveal_selected();
    }

    pub(crate) fn toggle_selected_raw(&mut self) -> bool {
        let toggled = self.display.toggle_selected_raw().is_some();
        if toggled {
            self.reveal_selected();
        }
        toggled
    }

    pub(crate) fn toggle_raw(&mut self, entry_id: &str) -> bool {
        self.display.toggle_raw(entry_id)
    }

    pub(crate) fn expand_selected(&mut self) {
        self.display.expand_selected();
        self.reveal_selected();
    }

    pub(crate) fn collapse_selected(&mut self) {
        self.display.collapse_selected();
        self.reveal_selected();
    }

    pub(crate) fn toggle_all_entries(&mut self) {
        self.display.toggle_all();
        self.reveal_selected();
    }

    pub(crate) fn toggle_all_thinking(&mut self) {
        self.display.toggle_all_thinking();
        self.reveal_selected();
    }

    pub(crate) fn thinking_fold_label(&self) -> &'static str {
        self.display.thinking_fold_label()
    }

    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) -> ScrollbackMouseAction {
        if let Some(action) = self.handle_link_mouse(mouse) {
            return action;
        }
        match mouse.kind {
            MouseEventKind::Down(crossterm::event::MouseButton::Left)
                if self
                    .scrollbar
                    .is_some_and(|area| area.contains((mouse.column, mouse.row).into())) =>
            {
                self.pointer.cancel_gesture();
                self.selection.clear();
                self.hovered = None;
                self.scrollbar_dragging = true;
                self.apply_scrollbar_position(mouse.row);
                return ScrollbackMouseAction::Ignored;
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left)
                if self.scrollbar_dragging =>
            {
                self.apply_scrollbar_position(mouse.row);
                return ScrollbackMouseAction::Ignored;
            }
            MouseEventKind::Up(crossterm::event::MouseButton::Left) if self.scrollbar_dragging => {
                self.scrollbar_dragging = false;
                return ScrollbackMouseAction::Ignored;
            }
            MouseEventKind::Moved => {
                self.hovered = self
                    .pointer
                    .item_at(mouse.column, mouse.row)
                    .map(|(item_id, _)| item_id)
                    .filter(|item_id| self.display.contains(item_id));
            }
            MouseEventKind::ScrollUp => {
                self.pointer.cancel_gesture();
                self.scrollbar_dragging = false;
                self.hovered = None;
                self.scroll_up(/* lines */ 3);
                return ScrollbackMouseAction::Ignored;
            }
            MouseEventKind::ScrollDown => {
                self.pointer.cancel_gesture();
                self.scrollbar_dragging = false;
                self.hovered = None;
                self.scroll_down(/* lines */ 3);
                return ScrollbackMouseAction::Ignored;
            }
            _ => {}
        }

        let entry_action = self.pointer.handle_mouse(mouse);
        match self.selection.handle_mouse(mouse) {
            ScrollbackSelectionAction::ScrollUp => self.scroll_up(/* lines */ 1),
            ScrollbackSelectionAction::ScrollDown => self.scroll_down(/* lines */ 1),
            ScrollbackSelectionAction::Copy(text) => {
                return ScrollbackMouseAction::Copy(text);
            }
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
        ScrollbackMouseAction::Ignored
    }

    pub(crate) fn clear_selection(&mut self) -> bool {
        self.selection.clear() | self.links.clear_highlight()
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

    fn select_and_snap(&mut self, item_id: Option<String>) {
        let Some(item_id) = item_id else {
            return;
        };
        if self.display.select(&item_id) {
            self.selection.clear_persistent();
            self.navigation.scroll_entry_to_top(&item_id);
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

    fn apply_scrollbar_position(&mut self, screen_row: u16) {
        let Some(area) = self.scrollbar else {
            return;
        };
        if area.height == 0 || screen_row <= area.y {
            self.navigation.scroll_to_top();
            return;
        }
        if screen_row >= area.bottom().saturating_sub(1) {
            self.navigation.scroll_to_bottom();
            return;
        }

        let viewport = self.navigation.viewport();
        let track_height = usize::from(area.height);
        let thumb_height = viewport
            .viewport_lines
            .saturating_mul(track_height)
            .div_ceil(viewport.total_lines)
            .clamp(1, track_height);
        let thumb_travel = track_height.saturating_sub(thumb_height);
        let max_top = viewport.total_lines.saturating_sub(viewport.viewport_lines);
        let cell = usize::from(screen_row.saturating_sub(area.y));
        let thumb_top = cell.saturating_sub(thumb_height / 2).min(thumb_travel);
        let offset = thumb_top
            .saturating_mul(max_top)
            .saturating_add(thumb_travel / 2)
            .checked_div(thumb_travel)
            .unwrap_or(0);
        self.navigation.set_scroll_offset(offset);
    }
}

fn section_overlaps_viewport(section: &TranscriptSection, viewport: ScrollbackViewport) -> bool {
    section.lines.start < viewport.end_visible_line
        && section.lines.end > viewport.first_visible_line
}

fn turn_prompt_id(turn: &TranscriptTurn) -> Option<String> {
    turn.blocks
        .iter()
        .find(|block| matches!(&block.block, crate::PresentationBlock::User { .. }))
        .map(|block| super::entry_state::entry_id(&turn.id, &block.item_id))
}

fn turn_id_from_entry(entry_id: &str) -> Option<&str> {
    entry_id.split_once('\0').map(|(turn_id, _)| turn_id)
}

fn response_anchor_id(turn: &TranscriptTurn) -> Option<String> {
    let mut anchor = None;
    for block in turn.blocks.iter().rev() {
        match &block.block {
            crate::PresentationBlock::Assistant { text }
            | crate::PresentationBlock::Plan { text, .. }
                if !text.trim().is_empty() =>
            {
                anchor = Some(super::entry_state::entry_id(&turn.id, &block.item_id));
            }
            crate::PresentationBlock::Thinking { .. }
            | crate::PresentationBlock::Tool(_)
            | crate::PresentationBlock::Subagent(_) => break,
            crate::PresentationBlock::User { .. }
            | crate::PresentationBlock::Assistant { .. }
            | crate::PresentationBlock::Plan { .. }
            | crate::PresentationBlock::Todo(_)
            | crate::PresentationBlock::System { .. } => {}
        }
    }
    anchor
}
