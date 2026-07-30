//! Link geometry and interaction for the visible Astral transcript.
//!
//! This follows Grok Build's `LinkOverlay` / `VisibleLinkMap` split: transcript
//! rendering records semantic targets, then the last rendered frame maps only
//! visible segments to terminal coordinates.

use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

use astral_terminal_inline::LinkSpan;
use codex_terminal_detection::TerminalName;
use codex_terminal_detection::terminal_info;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use regex::Regex;
use unicode_width::UnicodeWidthStr;

use super::AstralTheme;
use super::ScrollbackViewport;
use super::TranscriptLayout;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    Url(String),
    File(PathBuf),
}

impl LinkTarget {
    pub(crate) fn display(&self) -> String {
        match self {
            Self::Url(url) => url.clone(),
            Self::File(path) => path.display().to_string(),
        }
    }

    fn osc8_url(&self) -> String {
        match self {
            Self::Url(url) => url.clone(),
            Self::File(path) => file_url(path),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptLink {
    pub(crate) line: usize,
    pub(crate) columns: Range<u16>,
    pub(crate) target: String,
    pub(crate) id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VisibleLink {
    rects: Vec<Rect>,
    target: LinkTarget,
    id: Option<String>,
}

impl VisibleLink {
    fn contains(&self, column: u16, row: u16) -> bool {
        self.rects
            .iter()
            .any(|rect| rect.contains((column, row).into()))
    }

    fn looks_like_bare_url_text(&self) -> bool {
        let LinkTarget::Url(url) = &self.target else {
            return false;
        };
        let painted = self
            .rects
            .iter()
            .map(|rect| usize::from(rect.width))
            .sum::<usize>();
        painted == UnicodeWidthStr::width(url.as_str())
    }
}

#[derive(Debug, Default)]
pub(crate) struct VisibleLinks {
    links: Vec<VisibleLink>,
    highlighted: Option<usize>,
    hovered: Option<usize>,
    pending_click: Option<(u16, u16, LinkTarget)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinkMouseAction {
    Ignored,
    Consumed,
    Open(LinkTarget),
}

impl VisibleLinks {
    pub(crate) fn rebuild(
        &mut self,
        layout: &TranscriptLayout,
        viewport: ScrollbackViewport,
        area: Rect,
        cwd: &Path,
    ) {
        let highlighted = self
            .highlighted
            .and_then(|index| self.links.get(index))
            .map(|link| link.target.clone());
        let hovered = self
            .hovered
            .and_then(|index| self.links.get(index))
            .map(|link| link.target.clone());
        self.links.clear();
        for link in &layout.links {
            if link.line < viewport.first_visible_line || link.line >= viewport.end_visible_line {
                continue;
            }
            let Some(target) = resolve_target(&link.target, cwd) else {
                continue;
            };
            let x = area.x.saturating_add(link.columns.start);
            let right = area.x.saturating_add(link.columns.end).min(area.right());
            if x >= right {
                continue;
            }
            let row_offset = link.line.saturating_sub(viewport.first_visible_line);
            let Ok(row_offset) = u16::try_from(row_offset) else {
                continue;
            };
            let rect = Rect::new(x, area.y.saturating_add(row_offset), right - x, 1);
            if let Some(id) = link.id.as_ref()
                && let Some(previous) = self.links.last_mut()
                && previous.id.as_ref() == Some(id)
                && previous.target == target
            {
                previous.rects.push(rect);
            } else {
                self.links.push(VisibleLink {
                    rects: vec![rect],
                    target,
                    id: link.id.clone(),
                });
            }
        }
        self.highlighted =
            highlighted.and_then(|target| self.links.iter().position(|link| link.target == target));
        self.hovered =
            hovered.and_then(|target| self.links.iter().position(|link| link.target == target));
        if self
            .highlighted
            .is_some_and(|index| index >= self.links.len())
        {
            self.highlighted = None;
        }
        if self.hovered.is_some_and(|index| index >= self.links.len()) {
            self.hovered = None;
        }
    }

    pub(crate) fn clear_frame(&mut self) {
        self.links.clear();
        self.highlighted = None;
        self.hovered = None;
        self.pending_click = None;
    }

    pub(crate) fn clear_highlight(&mut self) -> bool {
        let changed = self.highlighted.take().is_some() || self.hovered.take().is_some();
        self.pending_click = None;
        changed
    }

    pub(crate) fn cycle(&mut self, forward: bool) -> bool {
        let count = self.links.len();
        if count == 0 {
            self.highlighted = None;
            return false;
        }
        self.highlighted = Some(match self.highlighted {
            None if forward => 0,
            None => count - 1,
            Some(index) if forward => (index + 1) % count,
            Some(index) => (index + count - 1) % count,
        });
        true
    }

    pub(crate) fn highlighted_target(&self) -> Option<LinkTarget> {
        self.highlighted
            .and_then(|index| self.links.get(index))
            .map(|link| link.target.clone())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    pub(crate) fn paint(&self, buffer: &mut Buffer, theme: AstralTheme) {
        let style = Style::default()
            .fg(theme.accent_running)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
        for index in [self.highlighted, self.hovered].into_iter().flatten() {
            if let Some(link) = self.links.get(index) {
                for rect in &link.rects {
                    buffer.set_style(*rect, style);
                }
            }
        }
    }

    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) -> LinkMouseAction {
        match mouse.kind {
            MouseEventKind::Moved => {
                self.hovered = link_modifier_held(mouse.modifiers)
                    .then(|| self.link_index_at(mouse.column, mouse.row))
                    .flatten()
                    .filter(|index| self.app_handles_click(*index));
                LinkMouseAction::Ignored
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(index) = self.link_index_at(mouse.column, mouse.row).filter(|index| {
                    link_modifier_held(mouse.modifiers) && self.app_handles_click(*index)
                }) else {
                    self.pending_click = None;
                    return LinkMouseAction::Ignored;
                };
                let target = self.links[index].target.clone();
                self.highlighted = Some(index);
                self.pending_click = Some((mouse.column, mouse.row, target));
                LinkMouseAction::Consumed
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.pending_click.take().is_some() {
                    LinkMouseAction::Consumed
                } else {
                    LinkMouseAction::Ignored
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let Some((column, row, target)) = self.pending_click.take() else {
                    return LinkMouseAction::Ignored;
                };
                if column == mouse.column && row == mouse.row {
                    LinkMouseAction::Open(target)
                } else {
                    LinkMouseAction::Consumed
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                self.pending_click = None;
                LinkMouseAction::Ignored
            }
            MouseEventKind::Down(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Up(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Drag(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => LinkMouseAction::Ignored,
        }
    }

    pub(crate) fn frame_spans(&self) -> Vec<LinkSpan> {
        if !supports_osc8(terminal_info().name) {
            return Vec::new();
        }
        self.links
            .iter()
            .enumerate()
            .flat_map(|(index, link)| {
                let url = link.target.osc8_url();
                let id = u32::try_from(index).ok().map(|index| index + 1);
                link.rects.iter().map(move |rect| LinkSpan {
                    row: rect.y,
                    col_start: rect.x,
                    col_end: rect.right(),
                    url: url.clone().into(),
                    id,
                })
            })
            .collect()
    }

    fn link_index_at(&self, column: u16, row: u16) -> Option<usize> {
        self.links
            .iter()
            .position(|link| link.contains(column, row))
    }

    fn app_handles_click(&self, index: usize) -> bool {
        let Some(link) = self.links.get(index) else {
            return false;
        };
        match terminal_info().name {
            TerminalName::VsCode => false,
            TerminalName::WarpTerminal if link.looks_like_bare_url_text() => false,
            TerminalName::AppleTerminal
            | TerminalName::Ghostty
            | TerminalName::Iterm2
            | TerminalName::WarpTerminal
            | TerminalName::WezTerm
            | TerminalName::Kitty
            | TerminalName::Alacritty
            | TerminalName::Konsole
            | TerminalName::GnomeTerminal
            | TerminalName::Vte
            | TerminalName::WindowsTerminal
            | TerminalName::Dumb
            | TerminalName::Unknown => true,
        }
    }
}

fn supports_osc8(terminal: TerminalName) -> bool {
    matches!(
        terminal,
        TerminalName::Ghostty
            | TerminalName::Iterm2
            | TerminalName::VsCode
            | TerminalName::WezTerm
            | TerminalName::Kitty
            | TerminalName::Alacritty
            | TerminalName::Konsole
            | TerminalName::GnomeTerminal
            | TerminalName::Vte
            | TerminalName::WindowsTerminal
    )
}

#[cfg(target_os = "macos")]
fn link_modifier_held(_modifiers: KeyModifiers) -> bool {
    crate::macos_modifiers::snapshot().command
}

#[cfg(not(target_os = "macos"))]
fn link_modifier_held(modifiers: KeyModifiers) -> bool {
    modifiers.contains(KeyModifiers::CONTROL)
}

pub(crate) fn append_detected_links(lines: &[Line<'static>], links: &mut Vec<TranscriptLink>) {
    for (line_index, line) in lines.iter().enumerate() {
        let text = line.to_string();
        append_regex_links(line_index, &text, url_regex(), links);
        append_regex_links(line_index, &text, path_regex(), links);
    }
}

fn append_regex_links(line: usize, text: &str, regex: &Regex, links: &mut Vec<TranscriptLink>) {
    for matched in regex.find_iter(text) {
        let value = matched
            .as_str()
            .trim_end_matches(['.', ',', ';', ':', '!', '?', ')']);
        let end = matched.start().saturating_add(value.len());
        if value.is_empty() || overlaps_existing(line, matched.start()..end, text, links) {
            continue;
        }
        let start_column = UnicodeWidthStr::width(&text[..matched.start()]);
        let end_column = start_column.saturating_add(UnicodeWidthStr::width(value));
        let (Ok(start_column), Ok(end_column)) =
            (u16::try_from(start_column), u16::try_from(end_column))
        else {
            continue;
        };
        links.push(TranscriptLink {
            line,
            columns: start_column..end_column,
            target: value.to_string(),
            id: None,
        });
    }
}

fn overlaps_existing(
    line: usize,
    bytes: Range<usize>,
    text: &str,
    links: &[TranscriptLink],
) -> bool {
    let start = UnicodeWidthStr::width(&text[..bytes.start]);
    let end = start.saturating_add(UnicodeWidthStr::width(&text[bytes]));
    links.iter().any(|link| {
        link.line == line
            && usize::from(link.columns.start) < end
            && start < usize::from(link.columns.end)
    })
}

fn resolve_target(raw: &str, cwd: &Path) -> Option<LinkTarget> {
    let raw = raw.trim();
    let lower = raw.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
    {
        return Some(LinkTarget::Url(raw.to_string()));
    }
    if raw.is_empty() || raw.starts_with('#') || raw.contains("://") || raw.contains(':') {
        return None;
    }
    let path = if raw == "~" {
        std::env::var_os("HOME").map(PathBuf::from)?
    } else if let Some(rest) = raw.strip_prefix("~/") {
        std::env::var_os("HOME").map(PathBuf::from)?.join(rest)
    } else {
        let path = Path::new(raw);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        }
    };
    Some(LinkTarget::File(path))
}

fn url_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"(?i)(?:https?://|mailto:)[^\s<>\[\]{}"'`]+"#)
            .unwrap_or_else(|error| panic!("invalid plain URL regex: {error}"))
    })
}

fn path_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        const SEGMENT: &str = r"[a-zA-Z0-9_@.%][a-zA-Z0-9._+@%\-]*";
        const SPACED: &str =
            r"[a-zA-Z0-9_@.%][a-zA-Z0-9._+@%\-]*(?: [a-zA-Z0-9._+@%\-]+)+\.[a-zA-Z0-9][a-zA-Z0-9._+@%\-]*";
        Regex::new(&format!(r"~?/(?:{SEGMENT}/)+(?:{SPACED}|{SEGMENT})"))
            .unwrap_or_else(|error| panic!("invalid absolute file path regex: {error}"))
    })
}

fn file_url(path: &Path) -> String {
    let path = path.to_string_lossy();
    let mut encoded = String::with_capacity(path.len() + 7);
    encoded.push_str("file://");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}
