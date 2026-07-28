use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::mcp_form::McpFormHit;
use crate::request_user_input::RequestUserInputHit;
use crate::view::AstralTheme;

use super::PaneHit;

#[derive(Debug, Clone, Copy)]
pub(super) enum OptionMarker {
    Radio,
    Checkbox,
}

pub(super) struct OptionRowRender {
    pub(super) hit: Option<PaneHit>,
    pub(super) marker: OptionMarker,
    pub(super) label: String,
    pub(super) detail: Option<String>,
    pub(super) selected: bool,
    pub(super) committed: bool,
    pub(super) focused: bool,
    pub(super) hovered: bool,
}

pub(super) fn render(
    buffer: &mut Buffer,
    area: Rect,
    content_x: u16,
    content_width: u16,
    row: OptionRowRender,
    theme: AstralTheme,
) {
    let row_background = if row.hovered || (row.selected && row.focused) {
        theme.panel_selected
    } else {
        theme.panel_background
    };
    let text_style = Style::default().fg(theme.text_primary).bg(row_background);
    buffer.set_style(area, text_style);

    let marker_selected = row.committed
        || matches!(
            row.hit,
            Some(PaneHit::UserInput(RequestUserInputHit::Confirmation(_)))
        ) && row.selected;
    let marker = match (row.marker, marker_selected) {
        (OptionMarker::Radio, true) => "(●)",
        (OptionMarker::Radio, false) => "(○)",
        (OptionMarker::Checkbox, true) => "[x]",
        (OptionMarker::Checkbox, false) => "[ ]",
    };
    let mut spans = Vec::new();
    if let Some(shortcut) = row.hit.and_then(shortcut) {
        spans.push(
            format!("{shortcut} ")
                .fg(theme.accent_running)
                .bg(row_background),
        );
        spans.push(
            format!("{marker} ")
                .fg(if marker_selected {
                    theme.text_primary
                } else {
                    theme.gray
                })
                .bg(row_background),
        );
    } else {
        spans.push(
            format!("{marker} ").set_style(text_style.fg(if row.selected {
                theme.accent_running
            } else {
                theme.gray
            })),
        );
    }
    spans.push(if row.selected {
        row.label.bold().bg(row_background)
    } else {
        row.label.set_style(text_style)
    });
    if let Some(detail) = row.detail {
        spans.push(" — ".set_style(text_style.fg(theme.gray_dim)));
        spans.push(detail.set_style(text_style.fg(theme.text_secondary)));
    }
    buffer.set_line(content_x, area.y, &Line::from(spans), content_width);
}

fn shortcut(hit: PaneHit) -> Option<usize> {
    match hit {
        PaneHit::UserInput(
            RequestUserInputHit::Option(index) | RequestUserInputHit::Confirmation(index),
        )
        | PaneHit::McpForm(McpFormHit::Choice(index)) => Some(index + 1),
        PaneHit::UserInput(RequestUserInputHit::Editor) | PaneHit::McpForm(McpFormHit::Editor) => {
            None
        }
    }
}
