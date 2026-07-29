//! Grok-style source file viewer used by `@file` references.

use std::path::Path;

use astral_tui_scrollback::CodeLineHighlighter;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::file_viewer::FileViewerContent;
use crate::file_viewer::FileViewerState;

use super::AstralTheme;
use super::block_viewer::ContentViewerPane;
use super::block_viewer::ViewerItem;
use super::block_viewer::viewer_item;
use super::block_viewer::viewer_item_with_logical_text;

pub(crate) struct FileViewerPane<'a> {
    pub(crate) state: &'a mut FileViewerState,
}

impl FileViewerPane<'_> {
    pub(crate) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        let title = self.state.path().to_string();
        let items = match self.state.content() {
            FileViewerContent::Loading => vec![viewer_item(
                Line::from("Opening file…").style(Style::default().fg(theme.gray)),
                None,
            )],
            FileViewerContent::Ready(source) => source_items(&title, source, theme),
            FileViewerContent::Error(error) => vec![viewer_item(
                Line::from(error.clone()).style(Style::default().fg(theme.accent_error)),
                None,
            )],
        };
        let initial_selection = self.state.take_initial_selection();
        ContentViewerPane {
            state: self.state.viewer_mut(),
            title,
            footer: "Esc close · / search · f filter · v select · Enter insert · x file only · y copy · Y path · w wrap".to_string(),
            items,
            is_running: false,
            initial_selection,
        }
        .render(area, buffer, theme);
    }
}

fn source_items(path: &str, source: &str, theme: AstralTheme) -> Vec<ViewerItem> {
    let lines = source.lines().collect::<Vec<_>>();
    let lines = if lines.is_empty() { vec![""] } else { lines };
    let digits = lines.len().to_string().len();
    let mut highlighter =
        CodeLineHighlighter::for_path(Path::new(path), source, theme.syntax_theme);
    lines
        .into_iter()
        .enumerate()
        .map(|(index, source_line)| {
            let mut spans = vec![Span::styled(
                format!("{:>digits$} ", index + 1),
                Style::default().fg(theme.gray),
            )];
            if let Some(highlighted) = highlighter
                .as_mut()
                .and_then(|highlighter| highlighter.highlight_line(source_line))
            {
                spans.extend(highlighted);
            } else {
                spans.push(source_line.to_string().into());
            }
            viewer_item_with_logical_text(Line::from(spans), source_line.to_string())
        })
        .collect()
}
