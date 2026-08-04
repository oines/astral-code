use std::time::Duration;
use std::time::Instant;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;

const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(500);

#[cfg(test)]
#[path = "choice_list_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChoiceListOutcome {
    Unchanged,
    Changed,
    Activate(usize),
}

#[derive(Debug, Default)]
pub(super) struct ChoiceList {
    selected: usize,
    hovered: Option<usize>,
    hits: Vec<Rect>,
    last_click: Option<(usize, Instant)>,
}

impl ChoiceList {
    pub(super) fn begin_frame(&mut self) {
        self.hits.clear();
    }

    pub(super) fn selected(&self) -> usize {
        self.selected
    }

    pub(super) fn set_selected(&mut self, selected: usize, len: usize) {
        self.selected = selected.min(len.saturating_sub(1));
    }

    pub(super) fn style(&self, index: usize) -> Style {
        if index == self.selected {
            Style::default().cyan().bold()
        } else if self.hovered == Some(index) {
            Style::default().reversed()
        } else {
            Style::default()
        }
    }

    pub(super) fn prefix(&self, index: usize) -> &'static str {
        if index == self.selected { "› " } else { "  " }
    }

    pub(super) fn record_hit(&mut self, area: Rect) {
        self.hits.push(area);
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent, len: usize) -> ChoiceListOutcome {
        if key.kind == KeyEventKind::Release || len == 0 {
            return ChoiceListOutcome::Unchanged;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::BackTab, _) => {
                self.move_by(-1, len)
            }
            (KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab, KeyModifiers::NONE) => {
                self.move_by(1, len)
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                return ChoiceListOutcome::Activate(self.selected);
            }
            (KeyCode::Char(ch @ '1'..='9'), KeyModifiers::NONE) => {
                let index = usize::from(ch as u8 - b'1');
                if index < len {
                    self.selected = index;
                    return ChoiceListOutcome::Activate(index);
                }
                return ChoiceListOutcome::Unchanged;
            }
            _ => return ChoiceListOutcome::Unchanged,
        }
        ChoiceListOutcome::Changed
    }

    pub(super) fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
        now: Instant,
        len: usize,
    ) -> ChoiceListOutcome {
        if len == 0 {
            return ChoiceListOutcome::Unchanged;
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => self.move_by(-1, len),
            MouseEventKind::ScrollDown => self.move_by(1, len),
            MouseEventKind::Moved => {
                let hovered = self.item_at(mouse);
                if self.hovered == hovered {
                    return ChoiceListOutcome::Unchanged;
                }
                self.hovered = hovered;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(index) = self.item_at(mouse) else {
                    self.last_click = None;
                    return ChoiceListOutcome::Unchanged;
                };
                self.selected = index;
                let double_click = self.last_click.is_some_and(|(previous, at)| {
                    previous == index && now.saturating_duration_since(at) < DOUBLE_CLICK_WINDOW
                });
                self.last_click = (!double_click).then_some((index, now));
                if double_click {
                    return ChoiceListOutcome::Activate(index);
                }
            }
            _ => return ChoiceListOutcome::Unchanged,
        }
        ChoiceListOutcome::Changed
    }

    fn move_by(&mut self, delta: i32, len: usize) {
        let last = len.saturating_sub(1) as i32;
        self.selected = (self.selected as i32 + delta).clamp(0, last) as usize;
        self.last_click = None;
    }

    fn item_at(&self, mouse: MouseEvent) -> Option<usize> {
        let point = (mouse.column, mouse.row).into();
        self.hits.iter().position(|area| area.contains(point))
    }
}
