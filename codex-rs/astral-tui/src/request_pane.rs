//! Grok-style prompt-area projection for typed app-server client requests.

mod user_input;

use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::McpServerElicitationRequest;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::PendingRequest;
use crate::request_user_input::RequestUserInputState;
use crate::view::AstralTheme;

const APPROVAL_HINTS: &[(&str, &str)] = &[
    ("Y", "allow"),
    ("A", "session"),
    ("N", "deny"),
    ("Esc", "cancel"),
];
const PERMISSION_HINTS: &[(&str, &str)] = &[("Y", "turn"), ("A", "session"), ("N", "deny")];
const MCP_FORM_HINTS: &[(&str, &str)] = &[("Enter", "submit"), ("N", "decline"), ("Esc", "cancel")];
const MCP_URL_HINTS: &[(&str, &str)] = &[("Y", "accept"), ("N", "decline"), ("Esc", "cancel")];
const WAITING_HINTS: &[(&str, &str)] = &[];

#[derive(Debug, Clone, Copy)]
pub(crate) struct RequestPane<'a> {
    request: &'a PendingRequest,
    request_user_input: &'a RequestUserInputState,
    composer: &'a str,
    cursor_byte: usize,
}

impl<'a> RequestPane<'a> {
    pub(crate) fn new(
        request: &'a PendingRequest,
        request_user_input: &'a RequestUserInputState,
        composer: &'a str,
        cursor_byte: usize,
    ) -> Self {
        Self {
            request,
            request_user_input,
            composer,
            cursor_byte,
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
            PendingRequest::Permissions { .. } => PERMISSION_HINTS,
            PendingRequest::UserInput { params, .. } => {
                user_input::shortcuts(params, self.request_user_input)
            }
            PendingRequest::McpElicitation { params, .. } => match params.request {
                McpServerElicitationRequest::Form { .. } => MCP_FORM_HINTS,
                McpServerElicitationRequest::Url { .. } => MCP_URL_HINTS,
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
                    label,
                    detail,
                    selected,
                    committed,
                } => {
                    let row_area =
                        Rect::new(area.x.saturating_add(1), y, area.width.saturating_sub(1), 1);
                    let text_style = if selected {
                        let style = Style::default()
                            .fg(theme.text_primary)
                            .bg(theme.panel_selected);
                        buffer.set_style(row_area, style);
                        style
                    } else {
                        Style::default()
                    };
                    let marker = if committed {
                        "●"
                    } else if selected {
                        "›"
                    } else {
                        "○"
                    };
                    let mut spans = vec![
                        format!("{marker} ").set_style(text_style.fg(if selected {
                            theme.accent_running
                        } else {
                            theme.gray
                        })),
                        label.set_style(text_style),
                    ];
                    if let Some(detail) = detail {
                        spans.push(" — ".set_style(text_style.fg(theme.gray_dim)));
                        spans.push(detail.set_style(text_style.fg(theme.text_secondary)));
                    }
                    buffer.set_line(content_x, y, &Line::from(spans), content_width);
                }
                PaneRow::Choice { key, label } => {
                    buffer.set_line(
                        content_x,
                        y,
                        &Line::from(vec![
                            format!("{key} ").fg(theme.accent_running).bold(),
                            "(○) ".fg(theme.gray),
                            label.into(),
                        ]),
                        content_width,
                    );
                }
                PaneRow::Input {
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
                push_command_choices(&mut rows, params);
            }
            PendingRequest::FileChange { params, .. } => {
                rows.push(PaneRow::Title("Allow file changes?".to_string()));
                rows.push(PaneRow::Body(params.reason.as_deref().map_or_else(
                    || "Edit requested files".to_string(),
                    std::convert::Into::into,
                )));
                rows.push(PaneRow::Blank);
                rows.extend([
                    PaneRow::choice("y", "Allow once"),
                    PaneRow::choice("a", "Allow for this session"),
                    PaneRow::choice("n", "Deny"),
                ]);
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
                rows.extend([
                    PaneRow::choice("y", "Allow for this turn"),
                    PaneRow::choice("a", "Allow for this session"),
                    PaneRow::choice("n", "Deny"),
                ]);
            }
            PendingRequest::UserInput { params, .. } => {
                input =
                    user_input::push_content(&mut rows, params, self.request_user_input, max_rows);
            }
            PendingRequest::McpElicitation { params, .. } => match &params.request {
                McpServerElicitationRequest::Form { message, .. } => {
                    rows.push(PaneRow::Title(format!(
                        "{} needs structured input",
                        params.server_name
                    )));
                    rows.push(PaneRow::Body(message.clone()));
                    rows.push(PaneRow::Blank);
                    rows.push(PaneRow::Input {
                        text: self.composer.to_string(),
                        cursor_column: input_cursor_width(self.composer, self.cursor_byte, false),
                    });
                    input = true;
                }
                McpServerElicitationRequest::Url { message, url, .. } => {
                    rows.push(PaneRow::Title(format!(
                        "Authorize {} in the browser?",
                        params.server_name
                    )));
                    rows.push(PaneRow::Body(message.clone()));
                    rows.push(PaneRow::Body(url.clone()));
                    rows.push(PaneRow::Blank);
                    rows.extend([
                        PaneRow::choice("y", "Open and continue"),
                        PaneRow::choice("n", "Decline"),
                    ]);
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
        label: String,
        detail: Option<String>,
        selected: bool,
        committed: bool,
    },
    Choice {
        key: &'static str,
        label: &'static str,
    },
    Input {
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

impl PaneRow {
    fn choice(key: &'static str, label: &'static str) -> Self {
        Self::Choice { key, label }
    }
}

fn push_command_choices(rows: &mut Vec<PaneRow>, params: &CommandExecutionRequestApprovalParams) {
    let available = |decision: &CommandExecutionApprovalDecision| {
        params
            .available_decisions
            .as_ref()
            .is_none_or(|decisions| decisions.contains(decision))
    };
    if available(&CommandExecutionApprovalDecision::Accept) {
        rows.push(PaneRow::choice("y", "Allow once"));
    }
    if available(&CommandExecutionApprovalDecision::AcceptForSession) {
        rows.push(PaneRow::choice("a", "Allow for this session"));
    }
    if available(&CommandExecutionApprovalDecision::Decline) {
        rows.push(PaneRow::choice("n", "Deny"));
    }
    if let Some(amendment) = &params.proposed_execpolicy_amendment {
        let decision = CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment {
            execpolicy_amendment: amendment.clone(),
        };
        if available(&decision) {
            rows.push(PaneRow::choice("e", "Trust the proposed command pattern"));
        }
    }
    if let Some(amendment) = params
        .proposed_network_policy_amendments
        .as_ref()
        .and_then(|amendments| amendments.first())
    {
        let decision = CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment {
            network_policy_amendment: amendment.clone(),
        };
        if available(&decision) {
            rows.push(PaneRow::choice("p", "Apply the proposed network rule"));
        }
    }
}
