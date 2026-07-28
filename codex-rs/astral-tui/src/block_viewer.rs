// Derived from Grok Build's block-viewer modal and pointer behavior at commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified to retain Astral's stable transcript entry id and render the current
// provider-neutral PresentationBlock instead of copying runtime payloads.

use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Rect;

#[path = "block_viewer/search.rs"]
mod search;

use self::search::ViewerSearch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockViewerMouseAction {
    Ignored,
    Redraw,
    Close,
}

/// Focus and viewport state for one transcript block viewer.
///
/// The block itself deliberately remains in `ConversationState`; keeping only
/// its stable entry id here lets live updates and resume use the same canonical
/// projection while the viewer is open.
#[derive(Debug)]
pub(crate) struct BlockViewerState {
    entry_id: String,
    scroll_offset: usize,
    max_scroll_offset: usize,
    page_size: usize,
    total_lines: usize,
    selected_line: Option<usize>,
    popup_area: Option<Rect>,
    content_area: Option<Rect>,
    close_button: Option<Rect>,
    close_hovered: bool,
    rendered_lines: Vec<String>,
    search: ViewerSearch,
}

impl BlockViewerState {
    pub(crate) fn new(entry_id: String) -> Self {
        Self {
            entry_id,
            scroll_offset: 0,
            max_scroll_offset: 0,
            page_size: 1,
            total_lines: 0,
            selected_line: None,
            popup_area: None,
            content_area: None,
            close_button: None,
            close_hovered: false,
            rendered_lines: Vec::new(),
            search: ViewerSearch::default(),
        }
    }

    pub(crate) fn entry_id(&self) -> &str {
        &self.entry_id
    }

    pub(crate) fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub(crate) fn selected_line(&self) -> Option<usize> {
        self.selected_line
    }

    pub(crate) fn close_hovered(&self) -> bool {
        self.close_hovered
    }

    pub(crate) fn search_input_active(&self) -> bool {
        self.search.input_active()
    }

    pub(crate) fn search_bar_visible(&self) -> bool {
        self.search.is_visible()
    }

    pub(crate) fn search_query(&self) -> &str {
        self.search.query()
    }

    pub(crate) fn search_cursor(&self) -> usize {
        self.search.cursor()
    }

    pub(crate) fn search_is_error(&self) -> bool {
        self.search.is_error()
    }

    pub(crate) fn search_match_count(&self) -> usize {
        self.search.match_count()
    }

    pub(crate) fn search_match_ranges(&self, line: usize) -> Vec<std::ops::Range<usize>> {
        self.rendered_lines
            .get(line)
            .map_or_else(Vec::new, |text| self.search.match_ranges(text))
    }

    pub(crate) fn rendered_line(&self, line: usize) -> Option<&str> {
        self.rendered_lines.get(line).map(String::as_str)
    }

    pub(crate) fn observe_frame(
        &mut self,
        popup_area: Rect,
        content_area: Rect,
        close_button: Rect,
        rendered_lines: Vec<String>,
    ) {
        self.popup_area = Some(popup_area);
        self.content_area = Some(content_area);
        self.close_button = Some(close_button);
        self.page_size = usize::from(content_area.height).max(1);
        self.total_lines = rendered_lines.len();
        self.max_scroll_offset = self.total_lines.saturating_sub(self.page_size);
        self.scroll_offset = self.scroll_offset.min(self.max_scroll_offset);
        self.selected_line = match self.total_lines {
            0 => None,
            _ => Some(self.selected_line.unwrap_or(0).min(self.total_lines - 1)),
        };
        self.rendered_lines = rendered_lines;
        self.search.rebuild(&self.rendered_lines);
        self.reveal_selected_line();
    }

    pub(crate) fn open_search(&mut self) {
        self.search.open();
    }

    pub(crate) fn handle_search_key(&mut self, key: crossterm::event::KeyEvent) {
        if let Some(target) = self
            .search
            .handle_key(key, &self.rendered_lines, self.selected_line)
        {
            self.select_line(target);
        }
    }

    pub(crate) fn handle_search_paste(&mut self, text: &str) {
        let target = self
            .search
            .paste(text, &self.rendered_lines, self.selected_line);
        if let Some(target) = target {
            self.select_line(target);
        }
    }

    pub(crate) fn select_next_match(&mut self) -> bool {
        let Some(target) = self.search.next_match(self.selected_line.unwrap_or(0)) else {
            return false;
        };
        self.select_line(target);
        true
    }

    pub(crate) fn select_previous_match(&mut self) -> bool {
        let Some(target) = self.search.previous_match(self.selected_line.unwrap_or(0)) else {
            return false;
        };
        self.select_line(target);
        true
    }

    pub(crate) fn scroll_by(&mut self, lines: isize) -> bool {
        let old_offset = self.scroll_offset;
        let next = self
            .scroll_offset
            .saturating_add_signed(lines)
            .min(self.max_scroll_offset);
        if next == old_offset {
            return false;
        }
        self.scroll_offset = next;
        if let Some(selected) = self.selected_line {
            let screen_row = selected.saturating_sub(old_offset);
            self.selected_line = Some(
                next.saturating_add(screen_row)
                    .min(self.total_lines.saturating_sub(1)),
            );
        }
        true
    }

    pub(crate) fn scroll_page(&mut self, pages: isize) -> bool {
        let lines = isize::try_from(self.page_size)
            .unwrap_or(isize::MAX)
            .saturating_mul(pages);
        self.scroll_by(lines)
    }

    pub(crate) fn scroll_half_page(&mut self, pages: isize) -> bool {
        let lines = isize::try_from((self.page_size / 2).max(1))
            .unwrap_or(isize::MAX)
            .saturating_mul(pages);
        self.scroll_by(lines)
    }

    pub(crate) fn select_by(&mut self, lines: isize) -> bool {
        let Some(selected) = self.selected_line else {
            return false;
        };
        let next = selected
            .saturating_add_signed(lines)
            .min(self.total_lines.saturating_sub(1));
        if next == selected {
            return false;
        }
        self.selected_line = Some(next);
        self.reveal_selected_line();
        true
    }

    pub(crate) fn scroll_to_start(&mut self) -> bool {
        let changed = self.scroll_offset != 0 || self.selected_line != Some(0);
        if !changed {
            return false;
        }
        self.scroll_offset = 0;
        self.selected_line = (self.total_lines > 0).then_some(0);
        changed
    }

    pub(crate) fn scroll_to_end(&mut self) -> bool {
        let last_line = self.total_lines.checked_sub(1);
        let changed =
            self.scroll_offset != self.max_scroll_offset || self.selected_line != last_line;
        if !changed {
            return false;
        }
        self.scroll_offset = self.max_scroll_offset;
        self.selected_line = last_line;
        changed
    }

    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) -> BlockViewerMouseAction {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let position = (mouse.column, mouse.row).into();
                if self
                    .close_button
                    .is_some_and(|area| area.contains(position))
                    || self.popup_area.is_some_and(|area| !area.contains(position))
                {
                    BlockViewerMouseAction::Close
                } else if let Some(area) = self.content_area
                    && area.contains(position)
                {
                    let line = self
                        .scroll_offset
                        .saturating_add(usize::from(mouse.row.saturating_sub(area.y)));
                    if line < self.total_lines {
                        self.selected_line = Some(line);
                    }
                    BlockViewerMouseAction::Redraw
                } else {
                    BlockViewerMouseAction::Ignored
                }
            }
            MouseEventKind::Moved => {
                let hovered = self
                    .close_button
                    .is_some_and(|area| area.contains((mouse.column, mouse.row).into()));
                if hovered == self.close_hovered {
                    BlockViewerMouseAction::Ignored
                } else {
                    self.close_hovered = hovered;
                    BlockViewerMouseAction::Redraw
                }
            }
            MouseEventKind::ScrollUp => {
                self.scroll_by(-3);
                BlockViewerMouseAction::Redraw
            }
            MouseEventKind::ScrollDown => {
                self.scroll_by(3);
                BlockViewerMouseAction::Redraw
            }
            MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
            | MouseEventKind::Up(_)
            | MouseEventKind::Drag(_)
            | MouseEventKind::Down(MouseButton::Right | MouseButton::Middle) => {
                BlockViewerMouseAction::Ignored
            }
        }
    }

    fn reveal_selected_line(&mut self) {
        let Some(selected) = self.selected_line else {
            return;
        };
        if selected < self.scroll_offset {
            self.scroll_offset = selected;
        } else if selected >= self.scroll_offset.saturating_add(self.page_size) {
            self.scroll_offset = selected
                .saturating_add(1)
                .saturating_sub(self.page_size)
                .min(self.max_scroll_offset);
        }
    }

    fn select_line(&mut self, line: usize) {
        if line >= self.total_lines {
            return;
        }
        self.selected_line = Some(line);
        self.reveal_selected_line();
    }
}

#[cfg(test)]
#[path = "block_viewer_tests.rs"]
mod tests;
