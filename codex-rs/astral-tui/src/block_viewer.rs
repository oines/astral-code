// Derived from Grok Build's block-viewer modal and pointer behavior at commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified to retain Astral's stable transcript entry id and render the current
// provider-neutral PresentationBlock instead of copying runtime payloads.

use astral_tui_scrollback::EditCopyLine;
use ratatui::layout::Rect;

#[path = "block_viewer/edit_copy.rs"]
mod edit_copy;
#[path = "block_viewer/follow.rs"]
mod follow;
#[path = "block_viewer/matcher.rs"]
mod matcher;
#[path = "block_viewer/navigation.rs"]
mod navigation;
#[path = "block_viewer/text_selection.rs"]
mod text_selection;

use self::matcher::ViewerMatchMode;
use self::matcher::ViewerMatcher;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BlockViewerMouseAction {
    Ignored,
    Redraw,
    Close,
    Copy(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewerWrapMode {
    Wrap,
    NoWrap,
}

impl ViewerWrapMode {
    fn toggled(self) -> Self {
        match self {
            Self::Wrap => Self::NoWrap,
            Self::NoWrap => Self::Wrap,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ViewerRowGeometry {
    item: usize,
    logical_start: u16,
    logical_end: u16,
}

impl ViewerRowGeometry {
    pub(crate) fn new(item: usize, logical_start: u16, logical_end: u16) -> Self {
        Self {
            item,
            logical_start,
            logical_end,
        }
    }
}

pub(crate) struct BlockViewerFrame {
    pub(crate) popup_area: Rect,
    pub(crate) content_area: Rect,
    pub(crate) close_button: Rect,
    pub(crate) logical_lines: Vec<String>,
    pub(crate) edit_copy_lines: Vec<Option<EditCopyLine>>,
    pub(crate) row_geometry: Vec<ViewerRowGeometry>,
    pub(crate) rendered_rows: Vec<String>,
    pub(crate) is_running: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TextEndpoint {
    item: usize,
    column: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TextDrag {
    anchor: TextEndpoint,
    head: TextEndpoint,
}

impl TextDrag {
    fn ordered(self) -> (TextEndpoint, TextEndpoint) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    fn is_non_empty(self) -> bool {
        self.anchor != self.head
    }
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
    total_rows: usize,
    selected_item: Option<usize>,
    visual_anchor: Option<usize>,
    wrap_mode: ViewerWrapMode,
    popup_area: Option<Rect>,
    content_area: Option<Rect>,
    scrollbar_area: Option<Rect>,
    scrollbar_dragging: bool,
    close_button: Option<Rect>,
    close_hovered: bool,
    logical_lines: Vec<String>,
    edit_copy_lines: Vec<Option<EditCopyLine>>,
    rendered_rows: Vec<String>,
    row_geometry: Vec<ViewerRowGeometry>,
    visible_item_indices: Vec<usize>,
    visible_row_indices: Vec<usize>,
    text_drag: Option<TextDrag>,
    matcher: ViewerMatcher,
    follow_enabled: bool,
    follow_mode: bool,
    at_content_edge: bool,
    mouse_overscroll: usize,
}

impl BlockViewerState {
    pub(crate) fn new(entry_id: String, is_running: bool) -> Self {
        Self {
            entry_id,
            scroll_offset: 0,
            max_scroll_offset: 0,
            page_size: 1,
            total_rows: 0,
            selected_item: None,
            visual_anchor: None,
            wrap_mode: ViewerWrapMode::Wrap,
            popup_area: None,
            content_area: None,
            scrollbar_area: None,
            scrollbar_dragging: false,
            close_button: None,
            close_hovered: false,
            logical_lines: Vec::new(),
            edit_copy_lines: Vec::new(),
            rendered_rows: Vec::new(),
            row_geometry: Vec::new(),
            visible_item_indices: Vec::new(),
            visible_row_indices: Vec::new(),
            text_drag: None,
            matcher: ViewerMatcher::default(),
            follow_enabled: is_running,
            follow_mode: is_running,
            at_content_edge: false,
            mouse_overscroll: 0,
        }
    }

    pub(crate) fn entry_id(&self) -> &str {
        &self.entry_id
    }

    pub(crate) fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    #[cfg(test)]
    pub(crate) fn selected_item(&self) -> Option<usize> {
        self.selected_item
    }

    pub(crate) fn wrap_mode(&self) -> ViewerWrapMode {
        self.wrap_mode
    }

    pub(crate) fn toggle_wrap_mode(&mut self) {
        self.wrap_mode = self.wrap_mode.toggled();
    }

    pub(crate) fn visual_selection_active(&self) -> bool {
        self.visual_anchor.is_some()
    }

    pub(crate) fn visual_selection_range(&self) -> Option<std::ops::RangeInclusive<usize>> {
        let anchor = self.visual_anchor?;
        let selected = self.selected_item?;
        Some(anchor.min(selected)..=anchor.max(selected))
    }

    pub(crate) fn close_hovered(&self) -> bool {
        self.close_hovered
    }

    pub(crate) fn observe_scrollbar_area(&mut self, area: Option<Rect>) {
        self.scrollbar_area = area;
        if area.is_none() {
            self.scrollbar_dragging = false;
        }
    }

    pub(crate) fn query_input_active(&self) -> bool {
        self.matcher.input_active()
    }

    pub(crate) fn query_bar_visible(&self) -> bool {
        self.matcher.is_visible()
    }

    pub(crate) fn query_label(&self) -> &'static str {
        self.matcher.mode().label()
    }

    pub(crate) fn query_text(&self) -> &str {
        self.matcher.query()
    }

    pub(crate) fn query_cursor(&self) -> usize {
        self.matcher.cursor()
    }

    pub(crate) fn query_is_error(&self) -> bool {
        self.matcher.is_error()
    }

    pub(crate) fn match_count(&self) -> usize {
        self.matcher.match_count()
    }

    pub(crate) fn match_ranges(&self, row: usize) -> Vec<std::ops::Range<usize>> {
        self.rendered_row(row)
            .map_or_else(Vec::new, |text| self.matcher.match_ranges(text))
    }

    pub(crate) fn rendered_row(&self, row: usize) -> Option<&str> {
        let physical = *self.visible_row_indices.get(row)?;
        self.rendered_rows.get(physical).map(String::as_str)
    }

    pub(crate) fn visible_row_indices(&self) -> &[usize] {
        &self.visible_row_indices
    }

    pub(crate) fn row_is_selected(&self, row: usize) -> bool {
        let Some(selected) = self.selected_physical_item() else {
            return false;
        };
        self.visible_row_physical_item(row) == Some(selected)
    }

    pub(crate) fn row_is_in_visual_selection(&self, row: usize) -> bool {
        let Some(range) = self.visual_selection_range() else {
            return false;
        };
        let Some(item) = self
            .visible_row_physical_item(row)
            .and_then(|physical| self.visible_item_position(physical))
        else {
            return false;
        };
        range.contains(&item)
    }

    pub(crate) fn observe_frame(&mut self, frame: BlockViewerFrame) {
        let BlockViewerFrame {
            popup_area,
            content_area,
            close_button,
            logical_lines,
            edit_copy_lines,
            row_geometry,
            rendered_rows,
            is_running,
        } = frame;
        self.popup_area = Some(popup_area);
        self.content_area = Some(content_area);
        self.close_button = Some(close_button);
        self.page_size = usize::from(content_area.height).max(1);
        let selected_physical = self.selected_physical_item();
        let anchor_physical = self.visual_anchor_physical_item();
        let layout_changed = self.logical_lines != logical_lines
            || self.row_geometry != row_geometry
            || self.rendered_rows != rendered_rows;
        let selected_screen_row = layout_changed
            .then(|| self.selected_item_screen_row())
            .flatten();
        self.logical_lines = logical_lines;
        self.edit_copy_lines = edit_copy_lines;
        self.row_geometry = row_geometry;
        self.rendered_rows = rendered_rows;
        self.matcher.rebuild(&self.logical_lines);
        self.rebuild_visible_rows(selected_physical, anchor_physical, selected_screen_row);
        if self.follow_enabled && !is_running {
            self.follow_enabled = false;
            self.follow_mode = false;
            self.selected_item = self.visible_item_indices.len().checked_sub(1);
            self.scroll_offset = self.max_scroll_offset;
        }
    }

    pub(crate) fn open_search(&mut self) {
        self.pause_follow();
        self.clear_visual_selection();
        self.matcher.open(ViewerMatchMode::Search);
        self.refresh_visible_rows();
    }

    pub(crate) fn open_filter(&mut self) {
        self.pause_follow();
        self.clear_visual_selection();
        self.matcher.open(ViewerMatchMode::Filter);
        self.refresh_visible_rows();
    }

    pub(crate) fn clear_matcher(&mut self) -> bool {
        if !self.matcher.is_visible() {
            return false;
        }
        self.matcher.clear();
        self.refresh_visible_rows();
        true
    }

    pub(crate) fn handle_query_key(&mut self, key: crossterm::event::KeyEvent) {
        let selected = self.selected_physical_item();
        if let Some(target) = self.matcher.handle_key(key, &self.logical_lines, selected) {
            self.select_physical_item(target);
        }
        self.refresh_visible_rows();
    }

    pub(crate) fn handle_query_paste(&mut self, text: &str) {
        let selected = self.selected_physical_item();
        let target = self.matcher.paste(text, &self.logical_lines, selected);
        if let Some(target) = target {
            self.select_physical_item(target);
        }
        self.refresh_visible_rows();
    }

    pub(crate) fn select_next_match(&mut self) -> bool {
        let Some(target) = self.next_match_line() else {
            return false;
        };
        self.select_physical_item(target);
        true
    }

    pub(crate) fn select_previous_match(&mut self) -> bool {
        let Some(target) = self.previous_match_line() else {
            return false;
        };
        self.select_physical_item(target);
        true
    }

    pub(crate) fn toggle_visual_selection(&mut self) {
        if self.visual_anchor.is_some() {
            self.clear_visual_selection();
        } else {
            self.begin_visual_selection();
        }
    }

    pub(crate) fn begin_visual_selection(&mut self) {
        self.pause_follow();
        if self.visual_anchor.is_none() {
            self.visual_anchor = self.selected_item;
        }
    }

    pub(crate) fn clear_visual_selection(&mut self) -> bool {
        self.visual_anchor.take().is_some()
    }
}

#[cfg(test)]
#[path = "block_viewer_tests.rs"]
mod tests;
