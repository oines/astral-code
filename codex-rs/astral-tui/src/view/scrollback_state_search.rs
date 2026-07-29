use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;

use crate::conversation::TranscriptTurn;

use super::super::AstralTheme;
use super::super::scrollback_search::ScrollbackSearch;
use super::super::scrollback_search::paint_search_highlights;
use super::super::scrollback_search::render_search_bar;
use super::ScrollbackState;

impl ScrollbackState {
    pub(crate) fn open_search(&mut self) -> bool {
        if !self.display.focus_scrollback() {
            return false;
        }
        self.search = Some(ScrollbackSearch::open());
        true
    }

    /// Handles an open transcript search.
    ///
    /// `Some(true)` means the query text changed and a new background scan must
    /// be submitted. `Some(false)` means the key was consumed without changing
    /// the query. `None` lets browsing-mode keys fall through to scrollback.
    pub(crate) fn handle_search_key(&mut self, key: KeyEvent) -> Option<bool> {
        let composing = self.search.as_ref()?.is_composing();
        if key.code == KeyCode::Esc {
            self.search = None;
            return Some(false);
        }
        if key.modifiers.is_empty() {
            match key.code {
                KeyCode::Down => {
                    self.navigate_search(/* forward */ true);
                    return Some(false);
                }
                KeyCode::Up => {
                    self.navigate_search(/* forward */ false);
                    return Some(false);
                }
                _ => {}
            }
        }
        if composing {
            if key.code == KeyCode::Enter {
                if self.search.as_ref()?.query().is_empty() {
                    self.search = None;
                } else {
                    self.search.as_mut()?.accept();
                    self.queue_current_search_match();
                }
                return Some(false);
            }
            if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
                let changed = self.search.as_mut()?.clear_query();
                if !changed {
                    self.search = None;
                }
                return Some(changed);
            }
            return Some(self.search.as_mut()?.edit_key(key));
        }
        match key.code {
            KeyCode::Char('n') if key.modifiers.is_empty() => {
                self.navigate_search(/* forward */ true);
                Some(false)
            }
            KeyCode::Char('N')
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                self.navigate_search(/* forward */ false);
                Some(false)
            }
            _ => None,
        }
    }

    pub(crate) fn paste_search(&mut self, text: &str) -> Option<bool> {
        let search = self.search.as_mut()?;
        if !search.is_composing() {
            return Some(false);
        }
        Some(search.paste(text))
    }

    pub(crate) fn search_needs_corpus(&self, generation: u64) -> bool {
        self.search
            .as_ref()
            .is_some_and(|search| search.needs_corpus(generation))
    }

    pub(crate) fn submit_search(&mut self, generation: u64, turns: Option<&[TranscriptTurn]>) {
        if let Some(search) = self.search.as_mut() {
            search.submit(generation, turns);
        }
    }

    pub(crate) fn poll_search(&mut self) -> bool {
        let changed = self.search.as_mut().is_some_and(ScrollbackSearch::poll);
        if changed {
            self.queue_current_search_match();
        }
        changed
    }

    pub(crate) fn search_pending(&self) -> bool {
        self.search.as_ref().is_some_and(ScrollbackSearch::pending)
    }

    pub(crate) fn search_active(&self) -> bool {
        self.search.is_some()
    }

    pub(crate) fn search_composing(&self) -> bool {
        self.search
            .as_ref()
            .is_some_and(ScrollbackSearch::is_composing)
    }

    pub(crate) fn search_reserved_rows(&self, scrollback_height: u16) -> u16 {
        if self.search_active() {
            scrollback_height.min(2)
        } else {
            0
        }
    }

    pub(crate) fn render_search(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        theme: AstralTheme,
    ) -> Option<Position> {
        self.search
            .as_ref()
            .and_then(|search| render_search_bar(search, area, buffer, theme))
    }

    pub(crate) fn render_search_highlights(
        &self,
        lines: &[ratatui::text::Line<'_>],
        area: Rect,
        buffer: &mut Buffer,
    ) {
        if let Some(search) = self.search.as_ref() {
            paint_search_highlights(search, lines, area, buffer);
        }
    }

    fn navigate_search(&mut self, forward: bool) {
        let Some(search) = self.search.as_mut() else {
            return;
        };
        if forward {
            search.next();
        } else {
            search.previous();
        }
        self.queue_current_search_match();
    }

    fn queue_current_search_match(&mut self) {
        let target = self
            .search
            .as_ref()
            .and_then(ScrollbackSearch::current)
            .map(|matched| (matched.entry_id.clone(), matched.line_in_entry));
        let Some((entry_id, line_in_entry)) = target else {
            return;
        };
        let _ = self.display.reveal(&entry_id);
        self.pending_search_target = Some((entry_id, line_in_entry));
    }
}
