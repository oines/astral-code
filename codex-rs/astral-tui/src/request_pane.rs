//! Grok-style prompt-area projection for typed app-server client requests.

use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::McpServerElicitationRequest;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::PendingRequest;
use crate::view::AstralTheme;

const APPROVAL_HINTS: &[(&str, &str)] = &[
    ("Y", "allow"),
    ("A", "session"),
    ("N", "deny"),
    ("Esc", "cancel"),
];
const PERMISSION_HINTS: &[(&str, &str)] = &[("Y", "turn"), ("A", "session"), ("N", "deny")];
const INPUT_HINTS: &[(&str, &str)] = &[("Enter", "submit"), ("Esc", "cancel")];
const MCP_FORM_HINTS: &[(&str, &str)] = &[("Enter", "submit"), ("N", "decline"), ("Esc", "cancel")];
const MCP_URL_HINTS: &[(&str, &str)] = &[("Y", "accept"), ("N", "decline"), ("Esc", "cancel")];
const WAITING_HINTS: &[(&str, &str)] = &[];

#[derive(Debug, Clone, Copy)]
pub(crate) struct RequestPane<'a> {
    request: &'a PendingRequest,
    composer: &'a str,
}

impl<'a> RequestPane<'a> {
    pub(crate) fn new(request: &'a PendingRequest, composer: &'a str) -> Self {
        Self { request, composer }
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
            PendingRequest::UserInput { .. } => INPUT_HINTS,
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
                PaneRow::Option { label, detail } => {
                    let mut spans = vec!["○ ".fg(theme.gray), label.into()];
                    if let Some(detail) = detail {
                        spans.push(" — ".fg(theme.gray_dim));
                        spans.push(detail.fg(theme.text_secondary));
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
                PaneRow::Input(text) => {
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
                    let line_width = u16::try_from(line.width()).unwrap_or(u16::MAX);
                    buffer.set_line(content_x, y, &line, content_width);
                    cursor = Some(Position::new(
                        content_x
                            .saturating_add(line_width)
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
                let question_count = params.questions.len();
                for (index, question) in params.questions.iter().enumerate() {
                    let counter = if question_count > 1 {
                        format!(" · {}/{}", index + 1, question_count)
                    } else {
                        String::new()
                    };
                    rows.push(PaneRow::Title(format!("{}{counter}", question.header)));
                    rows.push(PaneRow::Body(question.question.clone()));
                    if let Some(options) = &question.options {
                        rows.extend(
                            options.iter().map(|option| PaneRow::Option {
                                label: option.label.clone(),
                                detail: (!option.description.is_empty())
                                    .then(|| option.description.clone()),
                            }),
                        );
                    }
                    if index + 1 < question_count {
                        rows.push(PaneRow::Blank);
                    }
                }
                rows.push(PaneRow::Blank);
                let text = if params.questions.iter().any(|question| question.is_secret) {
                    "•".repeat(self.composer.chars().count())
                } else {
                    self.composer.to_string()
                };
                rows.push(PaneRow::Input(text));
                input = true;
            }
            PendingRequest::McpElicitation { params, .. } => match &params.request {
                McpServerElicitationRequest::Form { message, .. } => {
                    rows.push(PaneRow::Title(format!(
                        "{} needs structured input",
                        params.server_name
                    )));
                    rows.push(PaneRow::Body(message.clone()));
                    rows.push(PaneRow::Blank);
                    rows.push(PaneRow::Input(self.composer.to_string()));
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
    },
    Choice {
        key: &'static str,
        label: &'static str,
    },
    Input(String),
    Error(String),
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
