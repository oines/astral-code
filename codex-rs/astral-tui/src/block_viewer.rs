//! Fullscreen content viewer over one canonical transcript entry.
//!
//! Grok's viewer reads the selected block again on every frame instead of
//! copying its text when the modal opens. Astral keeps the same invariant:
//! app-server transcript state remains authoritative while this host owns only
//! viewport and raw/summary presentation state.

use astral_tui_scrollback::MarkdownLine;
use astral_tui_scrollback::TranscriptEntryId;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Rect;

use crate::ModalOutcome;
use crate::ModalWindow;
use crate::SurfaceNodeId;

#[path = "block_viewer/conversation_source.rs"]
mod conversation_source;
#[path = "block_viewer/render.rs"]
mod render;

const COPY_SHORTCUT: usize = 0;
const RAW_SHORTCUT: usize = 1;
const WHEEL_ROWS: usize = 3;

/// Result of routing one viewer input event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockViewerOutcome {
    Unchanged,
    Changed,
    Close,
    Copy(String),
}

/// Which canonical representation a block viewer is displaying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockViewerMode {
    Rich,
    Raw,
}

impl BlockViewerMode {
    const fn alternate(self) -> Self {
        match self {
            Self::Rich => Self::Raw,
            Self::Raw => Self::Rich,
        }
    }
}

/// One width-resolved viewer document produced by an authoritative transcript source.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockViewerDocument {
    title: String,
    lines: Vec<MarkdownLine>,
}

impl BlockViewerDocument {
    /// Construct a viewer document, rejecting empty content before a modal can open.
    pub fn new(title: impl Into<String>, lines: Vec<MarkdownLine>) -> Option<Self> {
        (!lines.is_empty()).then(|| Self {
            title: title.into(),
            lines,
        })
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn lines(&self) -> &[MarkdownLine] {
        &self.lines
    }
}

/// Resolves viewer content by stable transcript id on every interaction and render.
///
/// Implementations must read their current authoritative transcript state rather than cache a
/// cloned entry. Returning `None` for a mode means that representation is not available; returning
/// `None` for both modes prevents the viewer from opening.
pub trait BlockViewerSource {
    fn block_viewer_document(
        &self,
        entry_id: TranscriptEntryId,
        width: u16,
        mode: BlockViewerMode,
    ) -> Option<BlockViewerDocument>;

    fn block_viewer_default_mode(&self, _entry_id: TranscriptEntryId) -> BlockViewerMode {
        BlockViewerMode::Rich
    }

    fn block_viewer_follow_bottom(&self, _entry_id: TranscriptEntryId) -> bool {
        false
    }
}

/// Retained interaction state for one selected transcript entry.
///
/// The entry itself is deliberately not cloned. Re-rendering resolves the stable local id
/// through [`BlockViewerSource`], so streaming updates and resume replacement cannot leave the
/// viewer on stale content.
#[derive(Debug)]
pub struct BlockViewerHost {
    entry_id: TranscriptEntryId,
    raw: bool,
    scroll_offset: usize,
    row_count: usize,
    content_height: u16,
    content_width: u16,
    follow_bottom: bool,
    content_area: Option<Rect>,
    scrollbar_area: Option<Rect>,
    scrollbar_dragging: bool,
    modal: ModalWindow,
}

impl BlockViewerHost {
    /// Open a normal content viewer for one source entry.
    ///
    /// Synthetic verb-group headers retain Enter as their expand/collapse
    /// action. Opaque reasoning has no displayable body and is rejected here,
    /// which prevents the empty Thought modal seen in the prototype.
    pub fn open(source: &(impl BlockViewerSource + ?Sized), node: SurfaceNodeId) -> Option<Self> {
        let SurfaceNodeId::Entry(entry_id) = node else {
            return None;
        };
        let mode = available_mode(source, entry_id, source.block_viewer_default_mode(entry_id))?;
        Some(Self {
            entry_id,
            raw: mode == BlockViewerMode::Raw,
            scroll_offset: 0,
            row_count: 0,
            content_height: 0,
            content_width: 1,
            follow_bottom: source.block_viewer_follow_bottom(entry_id),
            content_area: None,
            scrollbar_area: None,
            scrollbar_dragging: false,
            modal: ModalWindow::default(),
        })
    }

    pub fn entry_id(&self) -> TranscriptEntryId {
        self.entry_id
    }

    pub fn raw(&self) -> bool {
        self.raw
    }

    /// Whether the canonical entry still exists and has viewer content.
    pub fn is_available(&self, source: &(impl BlockViewerSource + ?Sized)) -> bool {
        available_mode(source, self.entry_id, self.mode()).is_some()
    }

    pub fn handle_key_event(
        &mut self,
        key: KeyEvent,
        source: &(impl BlockViewerSource + ?Sized),
    ) -> BlockViewerOutcome {
        if key.kind == KeyEventKind::Release {
            return BlockViewerOutcome::Unchanged;
        }
        if self.modal.handle_key_event(key) == ModalOutcome::CloseRequested {
            return BlockViewerOutcome::Close;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), KeyModifiers::NONE)
            | (KeyCode::Char('f'), KeyModifiers::CONTROL) => BlockViewerOutcome::Close,
            (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                changed(self.scroll_up(1))
            }
            (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                changed(self.scroll_down(1))
            }
            (KeyCode::PageUp, KeyModifiers::NONE) => changed(self.scroll_up(self.page_rows())),
            (KeyCode::PageDown, KeyModifiers::NONE) | (KeyCode::Char(' '), KeyModifiers::NONE) => {
                changed(self.scroll_down(self.page_rows()))
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                changed(self.scroll_up(self.half_page_rows()))
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                changed(self.scroll_down(self.half_page_rows()))
            }
            (KeyCode::Home, KeyModifiers::NONE) | (KeyCode::Char('g'), KeyModifiers::NONE) => {
                changed(self.goto_top())
            }
            (KeyCode::End, KeyModifiers::NONE)
            | (KeyCode::Char('G'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                changed(self.goto_bottom())
            }
            (KeyCode::Char('r'), KeyModifiers::NONE) if self.supports_raw(source) => {
                self.raw = !self.raw;
                self.scroll_offset = 0;
                self.follow_bottom = false;
                BlockViewerOutcome::Changed
            }
            (KeyCode::Char('y'), KeyModifiers::NONE) => self
                .copy_text(source)
                .map_or(BlockViewerOutcome::Unchanged, BlockViewerOutcome::Copy),
            _ => BlockViewerOutcome::Unchanged,
        }
    }

    pub fn handle_mouse_event(
        &mut self,
        mouse: MouseEvent,
        source: &(impl BlockViewerSource + ?Sized),
    ) -> BlockViewerOutcome {
        match self.modal.handle_mouse_event(mouse) {
            ModalOutcome::CloseRequested => return BlockViewerOutcome::Close,
            ModalOutcome::ShortcutActivated(COPY_SHORTCUT) => {
                return self
                    .copy_text(source)
                    .map_or(BlockViewerOutcome::Unchanged, BlockViewerOutcome::Copy);
            }
            ModalOutcome::ShortcutActivated(RAW_SHORTCUT) if self.supports_raw(source) => {
                self.raw = !self.raw;
                self.scroll_offset = 0;
                self.follow_bottom = false;
                return BlockViewerOutcome::Changed;
            }
            ModalOutcome::Handled | ModalOutcome::TabChanged(_) => {
                return BlockViewerOutcome::Changed;
            }
            ModalOutcome::ShortcutActivated(_) | ModalOutcome::Unhandled => {}
        }

        match mouse.kind {
            MouseEventKind::ScrollUp if self.pointer_in_content(mouse) => {
                changed(self.scroll_up(WHEEL_ROWS))
            }
            MouseEventKind::ScrollDown if self.pointer_in_content(mouse) => {
                changed(self.scroll_down(WHEEL_ROWS))
            }
            MouseEventKind::Down(MouseButton::Left) if self.pointer_in_scrollbar(mouse) => {
                self.scrollbar_dragging = true;
                changed(self.apply_scrollbar_row(mouse.row))
            }
            MouseEventKind::Drag(MouseButton::Left) if self.scrollbar_dragging => {
                changed(self.apply_scrollbar_row(mouse.row))
            }
            MouseEventKind::Up(MouseButton::Left) if self.scrollbar_dragging => {
                self.scrollbar_dragging = false;
                changed(self.apply_scrollbar_row(mouse.row))
            }
            _ => BlockViewerOutcome::Unchanged,
        }
    }

    fn scroll_up(&mut self, rows: usize) -> bool {
        let before = self.scroll_offset;
        self.scroll_offset = self.scroll_offset.saturating_sub(rows);
        if self.scroll_offset < self.maximum_scroll() {
            self.follow_bottom = false;
        }
        self.scroll_offset != before
    }

    fn scroll_down(&mut self, rows: usize) -> bool {
        let before = self.scroll_offset;
        self.scroll_offset = self
            .scroll_offset
            .saturating_add(rows)
            .min(self.maximum_scroll());
        self.follow_bottom = self.scroll_offset == self.maximum_scroll();
        self.scroll_offset != before
    }

    fn goto_top(&mut self) -> bool {
        let changed = self.scroll_offset != 0;
        self.scroll_offset = 0;
        self.follow_bottom = false;
        changed
    }

    fn goto_bottom(&mut self) -> bool {
        let maximum = self.maximum_scroll();
        let changed = self.scroll_offset != maximum || !self.follow_bottom;
        self.scroll_offset = maximum;
        self.follow_bottom = true;
        changed
    }

    fn maximum_scroll(&self) -> usize {
        self.row_count
            .saturating_sub(usize::from(self.content_height))
    }

    fn page_rows(&self) -> usize {
        usize::from(self.content_height).saturating_sub(1).max(1)
    }

    fn half_page_rows(&self) -> usize {
        (usize::from(self.content_height) / 2).max(1)
    }

    fn pointer_in_content(&self, mouse: MouseEvent) -> bool {
        self.content_area.is_some_and(|area| contains(area, mouse))
            || self.pointer_in_scrollbar(mouse)
    }

    fn pointer_in_scrollbar(&self, mouse: MouseEvent) -> bool {
        self.scrollbar_area
            .is_some_and(|area| contains(area, mouse))
    }

    fn apply_scrollbar_row(&mut self, row: u16) -> bool {
        let Some(area) = self.scrollbar_area else {
            return false;
        };
        let height = usize::from(area.height);
        let maximum = self.maximum_scroll();
        if height == 0 || maximum == 0 {
            return false;
        }
        let click = usize::from(
            row.saturating_sub(area.y)
                .min(area.height.saturating_sub(1)),
        );
        let thumb_height = height
            .saturating_mul(height)
            .div_ceil(self.row_count)
            .clamp(1, height);
        let travel = height.saturating_sub(thumb_height);
        let target = maximum
            .saturating_mul(click.saturating_sub(thumb_height / 2).min(travel))
            .checked_div(travel)
            .unwrap_or(0);
        let changed = self.scroll_offset != target;
        self.scroll_offset = target;
        self.follow_bottom = target == maximum;
        changed
    }

    fn mode(&self) -> BlockViewerMode {
        if self.raw {
            BlockViewerMode::Raw
        } else {
            BlockViewerMode::Rich
        }
    }

    fn supports_raw(&self, source: &(impl BlockViewerSource + ?Sized)) -> bool {
        source
            .block_viewer_document(
                self.entry_id,
                self.content_width.max(1),
                BlockViewerMode::Rich,
            )
            .is_some()
            && source
                .block_viewer_document(
                    self.entry_id,
                    self.content_width.max(1),
                    BlockViewerMode::Raw,
                )
                .is_some()
    }

    fn reconcile(&mut self, source: &(impl BlockViewerSource + ?Sized)) -> bool {
        let Some(mode) = available_mode(source, self.entry_id, self.mode()) else {
            return false;
        };
        self.raw = mode == BlockViewerMode::Raw;
        true
    }
}

fn available_mode(
    source: &(impl BlockViewerSource + ?Sized),
    entry_id: TranscriptEntryId,
    requested: BlockViewerMode,
) -> Option<BlockViewerMode> {
    [requested, requested.alternate()].into_iter().find(|mode| {
        source
            .block_viewer_document(entry_id, /*width*/ 1, *mode)
            .is_some()
    })
}

fn contains(area: Rect, mouse: MouseEvent) -> bool {
    mouse.column >= area.x
        && mouse.column < area.right()
        && mouse.row >= area.y
        && mouse.row < area.bottom()
}

fn changed(changed: bool) -> BlockViewerOutcome {
    if changed {
        BlockViewerOutcome::Changed
    } else {
        BlockViewerOutcome::Unchanged
    }
}

#[cfg(test)]
#[path = "block_viewer_tests.rs"]
mod tests;
