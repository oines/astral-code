//! Grok-style prompt-area projection for typed app-server client requests.

mod hit_test;
mod mcp_form;
mod user_input;

use codex_app_server_protocol::McpServerElicitationRequest;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::PendingRequest;
use crate::mcp_form::McpFormState;
use crate::request_choice::RequestChoiceState;
use crate::request_user_input::RequestUserInputHit;
use crate::request_user_input::RequestUserInputState;
use crate::view::AstralTheme;

use self::hit_test::PaneHit;

const APPROVAL_HINTS: &[(&str, &str)] = &[
    ("↑/↓", "navigate"),
    ("Enter", "select"),
    ("Tab", "transcript"),
    ("Esc", "cancel"),
];
const WAITING_HINTS: &[(&str, &str)] = &[];

#[derive(Debug, Clone, Copy)]
pub(crate) struct RequestPane<'a> {
    request: &'a PendingRequest,
    request_choice: &'a RequestChoiceState,
    request_user_input: &'a RequestUserInputState,
    mcp_form: &'a McpFormState,
    focused: bool,
}

impl<'a> RequestPane<'a> {
    pub(crate) fn new(
        request: &'a PendingRequest,
        request_choice: &'a RequestChoiceState,
        request_user_input: &'a RequestUserInputState,
        mcp_form: &'a McpFormState,
        focused: bool,
    ) -> Self {
        Self {
            request,
            request_choice,
            request_user_input,
            mcp_form,
            focused,
        }
    }

    pub(crate) fn height(self, screen_height: u16) -> u16 {
        let available = screen_height.saturating_sub(3).clamp(3, 12);
        u16::try_from(self.content(available).rows.len()).unwrap_or(u16::MAX)
    }

    pub(crate) fn shortcuts(self) -> &'static [(&'static str, &'static str)] {
        match self.request {
            PendingRequest::CommandExecution { .. } | PendingRequest::FileChange { .. } => {
                APPROVAL_HINTS
            }
            PendingRequest::Permissions { .. } => APPROVAL_HINTS,
            PendingRequest::UserInput { params, .. } => {
                user_input::shortcuts(params, self.request_user_input)
            }
            PendingRequest::McpElicitation { params, .. } => match params.request {
                McpServerElicitationRequest::Form { .. } => mcp_form::shortcuts(self.mcp_form),
                McpServerElicitationRequest::Url { .. } => APPROVAL_HINTS,
            },
            PendingRequest::DynamicTool { .. }
            | PendingRequest::Attestation { .. }
            | PendingRequest::LegacyApplyPatch { .. }
            | PendingRequest::LegacyExecCommand { .. } => WAITING_HINTS,
        }
    }

    pub(crate) fn render(
        self,
        area: Rect,
        buffer: &mut Buffer,
        theme: AstralTheme,
    ) -> Option<Position> {
        if area.is_empty() {
            return None;
        }
        buffer.set_style(
            area,
            Style::default()
                .fg(theme.text_primary)
                .bg(theme.panel_background),
        );
        for y in area.y..area.bottom() {
            if let Some(cell) = buffer.cell_mut(Position::new(area.x, y)) {
                cell.set_char('┃');
                cell.set_style(
                    Style::default()
                        .fg(theme.accent_running)
                        .bg(theme.panel_background),
                );
            }
        }

        let content = self.content(area.height);
        let content_x = area.x.saturating_add(3);
        let content_width = area.width.saturating_sub(5);
        let mut cursor = None;
        for (index, row) in content.rows.into_iter().enumerate() {
            let y = area
                .y
                .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
            if y >= area.bottom() {
                break;
            }
            match row {
                PaneRow::Blank => {}
                PaneRow::Title(text) => {
                    buffer.set_line(content_x, y, &Line::from(text).bold(), content_width);
                }
                PaneRow::Body(text) => {
                    buffer.set_line(
                        content_x,
                        y,
                        &Line::from(text).fg(theme.text_secondary),
                        content_width,
                    );
                }
                PaneRow::Option {
                    hit,
                    label,
                    detail,
                    selected,
                    committed,
                } => {
                    let hovered = hit.is_some_and(|hit| match hit {
                        PaneHit::UserInput(hit) => self.request_user_input.hovered() == Some(hit),
                    });
                    let row_area =
                        Rect::new(area.x.saturating_add(1), y, area.width.saturating_sub(1), 1);
                    let row_background = if hovered || (selected && self.focused) {
                        theme.panel_selected
                    } else {
                        theme.panel_background
                    };
                    let text_style = Style::default().fg(theme.text_primary).bg(row_background);
                    buffer.set_style(row_area, text_style);
                    let shortcut = hit.and_then(|hit| match hit {
                        PaneHit::UserInput(
                            RequestUserInputHit::Option(index)
                            | RequestUserInputHit::Confirmation(index),
                        ) => Some(index + 1),
                        PaneHit::UserInput(RequestUserInputHit::Editor) => None,
                    });
                    let marker_selected = committed
                        || matches!(
                            hit,
                            Some(PaneHit::UserInput(RequestUserInputHit::Confirmation(_)))
                        ) && selected;
                    let marker = if marker_selected { "●" } else { "○" };
                    let mut spans = Vec::new();
                    if let Some(shortcut) = shortcut {
                        spans.push(
                            format!("{shortcut} ")
                                .fg(theme.accent_running)
                                .bg(row_background),
                        );
                        spans.push(
                            format!("({marker}) ")
                                .fg(if marker_selected {
                                    theme.text_primary
                                } else {
                                    theme.gray
                                })
                                .bg(row_background),
                        );
                    } else {
                        let marker = if committed {
                            "●"
                        } else if selected {
                            "›"
                        } else {
                            "○"
                        };
                        spans.push(format!("{marker} ").set_style(text_style.fg(if selected {
                            theme.accent_running
                        } else {
                            theme.gray
                        })));
                    }
                    spans.push(if selected {
                        label.bold().bg(row_background)
                    } else {
                        label.set_style(text_style)
                    });
                    if let Some(detail) = detail {
                        spans.push(" — ".set_style(text_style.fg(theme.gray_dim)));
                        spans.push(detail.set_style(text_style.fg(theme.text_secondary)));
                    }
                    buffer.set_line(content_x, y, &Line::from(spans), content_width);
                }
                PaneRow::Choice { index, label } => {
                    let selected = self.request_choice.selected() == Some(index);
                    let hovered = self.request_choice.hovered() == Some(index);
                    let row_area =
                        Rect::new(area.x.saturating_add(1), y, area.width.saturating_sub(1), 1);
                    let row_background = if hovered || (selected && self.focused) {
                        theme.panel_selected
                    } else {
                        theme.panel_background
                    };
                    buffer.set_style(
                        row_area,
                        Style::default().fg(theme.text_primary).bg(row_background),
                    );
                    let marker = if selected { "●" } else { "○" };
                    let label = if selected { label.bold() } else { label.into() };
                    buffer.set_line(
                        content_x,
                        y,
                        &Line::from(vec![
                            format!("{} ", index + 1)
                                .fg(theme.accent_running)
                                .bg(row_background),
                            format!("({marker}) ")
                                .fg(if selected {
                                    theme.text_primary
                                } else {
                                    theme.gray
                                })
                                .bg(row_background),
                            label.bg(row_background),
                        ]),
                        content_width,
                    );
                }
                PaneRow::Input {
                    hit: _,
                    text,
                    cursor_column,
                } => {
                    let row_area =
                        Rect::new(area.x.saturating_add(1), y, area.width.saturating_sub(1), 1);
                    buffer.set_style(
                        row_area,
                        Style::default()
                            .fg(theme.text_primary)
                            .bg(theme.panel_selected),
                    );
                    if let Some(cell) = buffer.cell_mut(Position::new(area.x, y)) {
                        cell.set_style(
                            Style::default()
                                .fg(theme.accent_running)
                                .bg(theme.panel_selected),
                        );
                    }
                    let line = Line::from(vec![
                        "❯ ".fg(theme.accent_running).bg(theme.panel_selected),
                        text.clone().bg(theme.panel_selected),
                    ]);
                    buffer.set_line(content_x, y, &line, content_width);
                    cursor = Some(Position::new(
                        content_x
                            .saturating_add(2)
                            .saturating_add(u16::try_from(cursor_column).unwrap_or(u16::MAX))
                            .min(area.right().saturating_sub(1)),
                        y,
                    ));
                }
                PaneRow::Error(text) => {
                    buffer.set_line(
                        content_x,
                        y,
                        &Line::from(text).fg(theme.accent_error),
                        content_width,
                    );
                }
            }
        }
        cursor
    }

    fn content(self, max_rows: u16) -> PaneContent {
        let mut rows = vec![PaneRow::Blank];
        let mut input = false;
        match self.request {
            PendingRequest::CommandExecution { params, .. } => {
                rows.push(PaneRow::Title("Allow command execution?".to_string()));
                rows.push(PaneRow::Body(format!(
                    "$ {}",
                    params.command.as_deref().unwrap_or("command")
                )));
                if let Some(reason) = params.reason.as_deref() {
                    rows.push(PaneRow::Body(format!("Reason · {reason}")));
                }
                rows.push(PaneRow::Blank);
                push_choices(&mut rows, self.request_choice);
            }
            PendingRequest::FileChange { params, .. } => {
                rows.push(PaneRow::Title("Allow file changes?".to_string()));
                rows.push(PaneRow::Body(params.reason.as_deref().map_or_else(
                    || "Edit requested files".to_string(),
                    std::convert::Into::into,
                )));
                rows.push(PaneRow::Blank);
                push_choices(&mut rows, self.request_choice);
            }
            PendingRequest::Permissions { params, .. } => {
                rows.push(PaneRow::Title("Grant additional permissions?".to_string()));
                rows.push(PaneRow::Body(
                    params
                        .reason
                        .as_deref()
                        .unwrap_or("Additional access requested")
                        .to_string(),
                ));
                rows.push(PaneRow::Body(format!(
                    "Working directory · {}",
                    params.cwd.to_string_lossy()
                )));
                rows.push(PaneRow::Blank);
                push_choices(&mut rows, self.request_choice);
            }
            PendingRequest::UserInput { params, .. } => {
                input =
                    user_input::push_content(&mut rows, params, self.request_user_input, max_rows);
            }
            PendingRequest::McpElicitation { params, .. } => match &params.request {
                McpServerElicitationRequest::Form { message, .. } => {
                    input = mcp_form::push_content(
                        &mut rows,
                        &params.server_name,
                        message,
                        self.mcp_form,
                        max_rows,
                    );
                }
                McpServerElicitationRequest::Url { message, url, .. } => {
                    rows.push(PaneRow::Title(format!(
                        "Authorize {} in the browser?",
                        params.server_name
                    )));
                    rows.push(PaneRow::Body(message.clone()));
                    rows.push(PaneRow::Body(url.clone()));
                    rows.push(PaneRow::Blank);
                    push_choices(&mut rows, self.request_choice);
                }
            },
            PendingRequest::DynamicTool { params, .. } => {
                rows.push(PaneRow::Title("Running Astral client tool".to_string()));
                rows.push(PaneRow::Body(params.namespace.as_ref().map_or_else(
                    || params.tool.clone(),
                    |namespace| format!("{namespace}/{}", params.tool),
                )));
                rows.push(PaneRow::Body(
                    "Waiting for a registered client handler".to_string(),
                ));
            }
            PendingRequest::Attestation { .. } => {
                rows.push(PaneRow::Title("Generating client attestation".to_string()));
                rows.push(PaneRow::Body(
                    "Waiting for the configured attestation provider".to_string(),
                ));
            }
            PendingRequest::LegacyApplyPatch { .. } | PendingRequest::LegacyExecCommand { .. } => {
                rows.push(PaneRow::Title("Legacy client request".to_string()));
                rows.push(PaneRow::Error(
                    "Astral TUI accepts app-server v2 requests only".to_string(),
                ));
            }
        }
        PaneContent::bounded(rows, input, usize::from(max_rows))
    }
}

fn input_cursor_width(text: &str, cursor_byte: usize, secret: bool) -> usize {
    let cursor = cursor_byte.min(text.len());
    if secret {
        text[..cursor].chars().count()
    } else {
        Line::from(&text[..cursor]).width()
    }
}

#[derive(Debug)]
struct PaneContent {
    rows: Vec<PaneRow>,
}

impl PaneContent {
    fn bounded(mut rows: Vec<PaneRow>, preserve_last: bool, max_rows: usize) -> Self {
        if rows.len() > max_rows {
            if preserve_last && max_rows >= 3 {
                let Some(last) = rows.pop() else {
                    return Self { rows };
                };
                rows.truncate(max_rows - 2);
                rows.push(PaneRow::Body("…".to_string()));
                rows.push(last);
            } else {
                rows.truncate(max_rows);
            }
        }
        Self { rows }
    }
}

#[derive(Debug)]
enum PaneRow {
    Blank,
    Title(String),
    Body(String),
    Option {
        hit: Option<PaneHit>,
        label: String,
        detail: Option<String>,
        selected: bool,
        committed: bool,
    },
    Choice {
        index: usize,
        label: &'static str,
    },
    Input {
        hit: Option<PaneHit>,
        text: String,
        cursor_column: usize,
    },
    Error(String),
}

fn push_visible_options(
    rows: &mut Vec<PaneRow>,
    options: Vec<PaneRow>,
    selected: usize,
    capacity: usize,
) {
    if options.len() <= capacity {
        rows.extend(options);
        return;
    }
    let total = options.len();
    let selected = selected.min(total - 1);
    if capacity == 1 {
        rows.extend(options.into_iter().skip(selected).take(1));
        return;
    }
    if capacity == 2 {
        if selected == 0 {
            rows.extend(options.into_iter().take(1));
            rows.push(PaneRow::Body(format!("… {} options below", total - 1)));
        } else {
            rows.push(PaneRow::Body(format!("… {selected} options above")));
            rows.extend(options.into_iter().skip(selected).take(1));
        }
        return;
    }

    let (start, end) = if selected < capacity - 1 {
        (0, capacity - 1)
    } else if selected >= total.saturating_sub(capacity - 1) {
        (total - (capacity - 1), total)
    } else {
        let visible = capacity.saturating_sub(2).max(1);
        let start = selected
            .saturating_sub(visible / 2)
            .clamp(1, total - visible - 1);
        (start, start + visible)
    };
    if start > 0 {
        rows.push(PaneRow::Body(format!("… {start} options above")));
    }
    rows.extend(options.into_iter().skip(start).take(end - start));
    if end < total {
        rows.push(PaneRow::Body(format!("… {} options below", total - end)));
    }
}

fn push_choices(rows: &mut Vec<PaneRow>, state: &RequestChoiceState) {
    rows.extend(
        state
            .choices()
            .iter()
            .enumerate()
            .map(|(index, choice)| PaneRow::Choice {
                index,
                label: choice.label,
            }),
    );
}
