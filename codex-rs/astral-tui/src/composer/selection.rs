//! Mouse selection state for the Astral prompt.
//!
//! The interaction rules follow Grok Build's `xai-ratatui-textarea` at commit
//! 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0), adapted to
//! Astral's existing UTF-8 byte-offset composer.

use std::ops::Range;
use std::time::Duration;
use std::time::Instant;

use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;

use super::ComposerState;
use super::line_end;
use super::line_start;
use super::previous_boundary;
use crate::composer::history::MutationKind;

const MULTI_CLICK_WINDOW: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Selection {
    anchor: usize,
    head: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ClickTracker {
    last_time: Option<Instant>,
    last_position: Option<(u16, u16)>,
    count: u8,
}

impl ClickTracker {
    fn register(&mut self, position: (u16, u16), now: Instant) -> u8 {
        let consecutive = self.last_position == Some(position)
            && self.count < 3
            && self
                .last_time
                .and_then(|last| now.checked_duration_since(last))
                .is_some_and(|elapsed| elapsed < MULTI_CLICK_WINDOW);
        self.count = if consecutive {
            self.count.saturating_add(1)
        } else {
            1
        };
        self.last_time = Some(now);
        self.last_position = Some(position);
        self.count
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComposerMouseAction {
    Nothing,
    Redraw,
    Copy(String),
}

impl ComposerState {
    pub(crate) fn selection_range(&self) -> Option<Range<usize>> {
        let selection = self.selection?;
        let start = selection.anchor.min(selection.head).min(self.text.len());
        let end = selection.anchor.max(selection.head).min(self.text.len());
        (start < end).then(|| self.expand_range_to_element_boundaries(start..end))
    }

    pub(crate) fn mouse_selection_active(&self) -> bool {
        self.drag_anchor.is_some()
    }

    pub(crate) fn mouse_drag_active(&self) -> bool {
        self.drag_active
    }

    pub(crate) fn handle_mouse(
        &mut self,
        mut event: MouseEvent,
        position: Option<usize>,
        now: Instant,
    ) -> ComposerMouseAction {
        if matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) && self.drag_active {
            event.kind = MouseEventKind::Drag(MouseButton::Left);
        }
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let click_count = self.click_tracker.register((event.column, event.row), now);
                let selection_cleared = self.clear_selection_state();
                let Some(position) = position.map(|position| position.min(self.text.len())) else {
                    return if selection_cleared {
                        ComposerMouseAction::Redraw
                    } else {
                        ComposerMouseAction::Nothing
                    };
                };
                match click_count {
                    2 if self.expand_paste_at_position(position) => ComposerMouseAction::Redraw,
                    2 => {
                        if let Some(start) = self.element_start_at(position) {
                            self.drag_anchor = Some(start);
                            self.set_cursor_from_mouse(start);
                            ComposerMouseAction::Redraw
                        } else {
                            self.select_word(position)
                        }
                    }
                    3 => self.select_line(position),
                    _ => {
                        let position = self.element_start_at(position).unwrap_or(position);
                        self.drag_anchor = Some(position);
                        self.set_cursor_from_mouse(position);
                        ComposerMouseAction::Redraw
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(anchor) = self.drag_anchor else {
                    return ComposerMouseAction::Nothing;
                };
                let Some(head) = position.map(|position| {
                    self.snap_position_to_element_boundary(position.min(self.text.len()))
                }) else {
                    return ComposerMouseAction::Nothing;
                };
                if head == anchor {
                    self.selection = None;
                    self.drag_active = false;
                } else {
                    self.selection = Some(Selection { anchor, head });
                    self.drag_active = true;
                }
                self.set_cursor_from_mouse(head);
                ComposerMouseAction::Redraw
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let was_drag = self.drag_active;
                self.drag_anchor = None;
                self.drag_active = false;
                if !was_drag {
                    return ComposerMouseAction::Nothing;
                }
                self.selected_text()
                    .map_or(ComposerMouseAction::Redraw, ComposerMouseAction::Copy)
            }
            MouseEventKind::Down(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Up(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Drag(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Moved
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => ComposerMouseAction::Nothing,
        }
    }

    pub(super) fn clear_selection_state(&mut self) -> bool {
        let changed = self.selection.take().is_some()
            || self.drag_anchor.take().is_some()
            || self.drag_active;
        self.drag_active = false;
        changed
    }

    pub(super) fn delete_selection(&mut self) -> bool {
        let Some(range) = self.selection_range() else {
            self.clear_selection_state();
            return false;
        };
        self.replace_range(range, "", MutationKind::Replace);
        true
    }

    fn selected_text(&self) -> Option<String> {
        Some(self.expanded_text_for_range(self.selection_range()?))
    }

    fn select_word(&mut self, position: usize) -> ComposerMouseAction {
        let whitespace = position < self.text.len()
            && self.text[position..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace);
        if whitespace {
            self.set_cursor_from_mouse(position);
            return ComposerMouseAction::Redraw;
        }
        let start = word_start_at(&self.text, position);
        let end = word_end_at(&self.text, position);
        if start >= end {
            self.set_cursor_from_mouse(position);
            return ComposerMouseAction::Redraw;
        }
        self.selection = Some(Selection {
            anchor: start,
            head: end,
        });
        self.set_cursor_from_mouse(previous_boundary(&self.text, end).unwrap_or(start));
        ComposerMouseAction::Copy(self.expanded_text_for_range(start..end))
    }

    fn select_line(&mut self, position: usize) -> ComposerMouseAction {
        let start = line_start(&self.text, position);
        let line_end = line_end(&self.text, position);
        let end = if line_end < self.text.len() {
            line_end.saturating_add(1)
        } else {
            line_end
        };
        self.selection = Some(Selection {
            anchor: start,
            head: end,
        });
        self.set_cursor_from_mouse(position);
        self.selected_text()
            .map_or(ComposerMouseAction::Redraw, ComposerMouseAction::Copy)
    }

    fn set_cursor_from_mouse(&mut self, position: usize) {
        self.cursor = self.snap_position_to_element_boundary(position);
        self.preferred_column = None;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WordClass {
    Whitespace,
    Word,
    Punctuation,
}

fn word_start_at(text: &str, position: usize) -> usize {
    let Some(target) = character_class_at(text, position) else {
        return 0;
    };
    text[..position]
        .char_indices()
        .rev()
        .find(|(_, character)| character_class(*character) != target)
        .map_or(0, |(index, character)| index + character.len_utf8())
}

fn word_end_at(text: &str, position: usize) -> usize {
    let Some(target) = text[position..].chars().next().map(character_class) else {
        return text.len();
    };
    text[position..]
        .char_indices()
        .find(|(_, character)| character_class(*character) != target)
        .map_or(text.len(), |(offset, _)| position + offset)
}

fn character_class_at(text: &str, position: usize) -> Option<WordClass> {
    text.get(position..)
        .and_then(|suffix| suffix.chars().next())
        .or_else(|| {
            text.get(..position)
                .and_then(|prefix| prefix.chars().next_back())
        })
        .map(character_class)
}

fn character_class(character: char) -> WordClass {
    if character.is_whitespace() {
        WordClass::Whitespace
    } else if character.is_alphanumeric() || character == '_' {
        WordClass::Word
    } else {
        WordClass::Punctuation
    }
}
