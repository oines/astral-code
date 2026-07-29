// Derived from Grok Build's background transcript search at commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Astral indexes only its renderer-facing PresentationBlock stream.

mod worker;

use std::sync::Arc;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use regex::Regex;
use regex::RegexBuilder;
use unicode_width::UnicodeWidthStr;

use crate::composer::ComposerState;
use crate::conversation::TranscriptTurn;

use super::AstralTheme;
use super::entry_state::entry_id;
use worker::SearchDaemon;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScrollbackMatch {
    pub(super) entry_id: String,
    pub(super) line_in_entry: usize,
}

#[derive(Debug)]
struct IndexedEntry {
    id: String,
    text: String,
}

#[derive(Debug, Default)]
struct SearchIndex {
    entries: Arc<[IndexedEntry]>,
    generation: Option<u64>,
}

impl SearchIndex {
    fn needs_corpus(&self, generation: u64) -> bool {
        self.generation != Some(generation)
    }

    fn rebuild(&mut self, generation: u64, turns: &[TranscriptTurn]) -> Arc<[IndexedEntry]> {
        self.entries = turns
            .iter()
            .flat_map(|turn| {
                turn.blocks.iter().filter_map(|block| {
                    block.block.searchable_text().map(|text| IndexedEntry {
                        id: entry_id(&turn.id, &block.item_id),
                        text,
                    })
                })
            })
            .collect::<Vec<_>>()
            .into();
        self.generation = Some(generation);
        self.entries.clone()
    }
}

#[derive(Debug)]
pub(super) struct ScrollbackSearch {
    editor: ComposerState,
    index: SearchIndex,
    matcher: Option<Regex>,
    matcher_error: bool,
    matches: Arc<[ScrollbackMatch]>,
    current: Option<usize>,
    composing: bool,
    daemon: SearchDaemon,
    request_generation: u64,
    last_seen_generation: u64,
}

impl ScrollbackSearch {
    pub(super) fn open() -> Self {
        Self {
            editor: ComposerState::default(),
            index: SearchIndex::default(),
            matcher: None,
            matcher_error: false,
            matches: Arc::from([]),
            current: None,
            composing: true,
            daemon: SearchDaemon::new(),
            request_generation: 0,
            last_seen_generation: 0,
        }
    }

    pub(super) fn query(&self) -> &str {
        self.editor.text()
    }

    pub(super) fn cursor(&self) -> usize {
        self.editor.cursor()
    }

    pub(super) fn is_composing(&self) -> bool {
        self.composing
    }

    pub(super) fn accept(&mut self) {
        self.composing = false;
    }

    pub(super) fn has_error(&self) -> bool {
        self.matcher_error
    }

    pub(super) fn match_count(&self) -> usize {
        self.matches.len()
    }

    pub(super) fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub(super) fn current(&self) -> Option<&ScrollbackMatch> {
        self.current.and_then(|index| self.matches.get(index))
    }

    pub(super) fn highlight_regex(&self) -> Option<&Regex> {
        self.matcher.as_ref()
    }

    pub(super) fn needs_corpus(&self, generation: u64) -> bool {
        self.index.needs_corpus(generation)
    }

    pub(super) fn edit_key(&mut self, key: KeyEvent) -> bool {
        let before = self.editor.text().to_string();
        let handled = match (key.code, key.modifiers) {
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => self.editor.move_home(),
            (KeyCode::Char('e'), KeyModifiers::CONTROL) => self.editor.move_end(),
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                if self.editor.cursor() == self.editor.text().len() {
                    false
                } else {
                    let prefix = self.editor.text()[..self.editor.cursor()].to_string();
                    self.editor.replace(prefix);
                    true
                }
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                if self.editor.cursor() == 0 {
                    false
                } else {
                    let suffix = self.editor.text()[self.editor.cursor()..].to_string();
                    self.editor.replace(suffix);
                    let _ = self.editor.move_home();
                    true
                }
            }
            (KeyCode::Char('w'), KeyModifiers::CONTROL) => self.editor.delete_word_left(),
            _ => self.editor.edit_key(key),
        };
        let changed = handled && self.editor.text() != before;
        if changed {
            self.refresh_matcher();
        }
        changed
    }

    pub(super) fn paste(&mut self, text: &str) -> bool {
        let single_line = text
            .chars()
            .filter(|character| !matches!(character, '\n' | '\r'))
            .collect::<String>();
        if single_line.is_empty() {
            return false;
        }
        self.editor.insert_text(&single_line);
        self.refresh_matcher();
        true
    }

    pub(super) fn clear_query(&mut self) -> bool {
        if self.editor.text().is_empty() {
            return false;
        }
        self.editor.clear();
        self.refresh_matcher();
        true
    }

    pub(super) fn submit(&mut self, generation: u64, turns: Option<&[TranscriptTurn]>) {
        let corpus = turns.map(|turns| self.index.rebuild(generation, turns));
        debug_assert!(
            corpus.is_some() || !self.index.needs_corpus(generation),
            "changed transcript generation requires a fresh search corpus"
        );
        self.request_generation = self.request_generation.wrapping_add(1);
        let request_generation = self.request_generation;
        if !self
            .daemon
            .update(corpus, self.editor.text().to_string(), request_generation)
        {
            self.last_seen_generation = request_generation;
        }
    }

    pub(super) fn poll(&mut self) -> bool {
        let result = match self.daemon.latest_result() {
            Ok(Some(result)) => result,
            Ok(None) => return false,
            Err(()) => {
                self.last_seen_generation = self.request_generation;
                return false;
            }
        };
        self.last_seen_generation = result.request_generation;
        if result.request_generation != self.request_generation || result.query != self.query() {
            return false;
        }
        self.matches = result.matches;
        self.current = (!self.matches.is_empty()).then_some(0);
        true
    }

    pub(super) fn pending(&self) -> bool {
        self.last_seen_generation != self.request_generation
    }

    pub(super) fn next(&mut self) {
        self.step(1);
    }

    pub(super) fn previous(&mut self) {
        self.step(-1);
    }

    fn step(&mut self, delta: isize) {
        if self.matches.is_empty() {
            self.current = None;
            return;
        }
        let current = self.current.unwrap_or_default() as isize;
        self.current = Some((current + delta).rem_euclid(self.matches.len() as isize) as usize);
    }

    fn refresh_matcher(&mut self) {
        match compile_query(self.editor.text()) {
            Ok(matcher) => {
                self.matcher = matcher;
                self.matcher_error = false;
            }
            Err(()) => {
                self.matcher = None;
                self.matcher_error = true;
            }
        }
        if self.matcher.is_none() {
            self.matches = Arc::from([]);
            self.current = None;
        }
    }
}

pub(super) fn render_search_bar(
    search: &ScrollbackSearch,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) -> Option<Position> {
    if area.is_empty() {
        return None;
    }
    buffer.set_style(area, Style::default().bg(theme.bg_base));
    if area.height >= 2 {
        buffer.set_stringn(
            area.x,
            area.y,
            "─".repeat(usize::from(area.width)),
            usize::from(area.width),
            Style::default().fg(theme.gray_dim).bg(theme.bg_base),
        );
    }
    let y = area.bottom().saturating_sub(1);
    let counter = match search.current_index() {
        Some(index) => Some(format!("{}/{}", index + 1, search.match_count())),
        None if search.has_error() => Some("bad pattern".to_string()),
        None if !search.query().is_empty() => Some("no matches".to_string()),
        None => None,
    };
    let counter_width = counter.as_deref().map_or(0, UnicodeWidthStr::width);
    let label = " search: ";
    let label_width = UnicodeWidthStr::width(label);
    let trailing = if counter_width > 0
        && usize::from(area.width) >= label_width.saturating_add(counter_width).saturating_add(2)
    {
        counter_width + 1
    } else {
        0
    };
    let input_width = usize::from(area.width)
        .saturating_sub(label_width)
        .saturating_sub(trailing);
    buffer.set_stringn(
        area.x,
        y,
        label,
        usize::from(area.width),
        Style::default()
            .fg(if search.has_error() {
                theme.accent_error
            } else {
                theme.gray
            })
            .bg(theme.bg_base),
    );
    let (visible, cursor_column) = query_input_window(search.query(), search.cursor(), input_width);
    buffer.set_stringn(
        area.x
            .saturating_add(u16::try_from(label_width).unwrap_or(u16::MAX)),
        y,
        visible,
        input_width,
        Style::default().fg(theme.text_primary).bg(theme.bg_base),
    );
    if let Some(counter) = counter
        && trailing > 0
    {
        buffer.set_string(
            area.right()
                .saturating_sub(u16::try_from(counter_width).unwrap_or(u16::MAX)),
            y,
            counter,
            Style::default().fg(theme.gray).bg(theme.bg_base),
        );
    }
    search.is_composing().then(|| {
        let x = area
            .x
            .saturating_add(u16::try_from(label_width).unwrap_or(u16::MAX))
            .saturating_add(u16::try_from(cursor_column).unwrap_or(u16::MAX))
            .min(area.right().saturating_sub(1));
        if let Some(cell) = buffer.cell_mut((x, y)) {
            cell.modifier.insert(Modifier::REVERSED);
        }
        Position::new(x, y)
    })
}

pub(super) fn paint_search_highlights(
    search: &ScrollbackSearch,
    lines: &[ratatui::text::Line<'_>],
    area: Rect,
    buffer: &mut Buffer,
) {
    let Some(matcher) = search.highlight_regex() else {
        return;
    };
    for (screen_row, line) in lines.iter().enumerate() {
        let y = area
            .y
            .saturating_add(u16::try_from(screen_row).unwrap_or(u16::MAX));
        if y >= area.bottom() {
            break;
        }
        let text = line.to_string();
        for matched in matcher.find_iter(&text) {
            if matched.start() == matched.end() {
                continue;
            }
            let start = UnicodeWidthStr::width(&text[..matched.start()]);
            let end = UnicodeWidthStr::width(&text[..matched.end()]);
            for column in start..end {
                let x = area
                    .x
                    .saturating_add(u16::try_from(column).unwrap_or(u16::MAX));
                if x >= area.right() {
                    break;
                }
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.modifier.insert(Modifier::REVERSED);
                }
            }
        }
    }
}

fn query_input_window(query: &str, cursor: usize, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }
    let cursor = cursor.min(query.len());
    let mut start = cursor;
    let cursor_limit = width.saturating_sub(1);
    while start > 0 {
        let previous = query[..start]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index);
        if UnicodeWidthStr::width(&query[previous..cursor]) > cursor_limit {
            break;
        }
        start = previous;
    }
    let cursor_column = UnicodeWidthStr::width(&query[start..cursor]).min(cursor_limit);
    let mut end = start;
    for (offset, character) in query[start..].char_indices() {
        let candidate = start + offset + character.len_utf8();
        if UnicodeWidthStr::width(&query[start..candidate]) > width {
            break;
        }
        end = candidate;
    }
    (query[start..end].to_string(), cursor_column)
}

fn compile_query(query: &str) -> Result<Option<Regex>, ()> {
    if query.is_empty() {
        return Ok(None);
    }
    RegexBuilder::new(query)
        .case_insensitive(!query.chars().any(char::is_uppercase))
        .build()
        .map(Some)
        .map_err(|_| ())
}
