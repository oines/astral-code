use codex_app_server_protocol::McpServerElicitationRequest;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Text;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::CommittedBlock;
use crate::ConversationState;
use crate::PendingRequest;
use crate::PendingRequests;
use crate::RenderOptions;
use crate::SessionState;
use crate::render_block;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceActivity {
    Ready,
    Working,
    Interrupted,
    Disconnected(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TranscriptView {
    Live,
    Full,
}

#[derive(Debug)]
pub struct SurfaceState {
    conversation: ConversationState,
    pending_requests: PendingRequests,
    composer: String,
    activity: SurfaceActivity,
    notice: Option<String>,
}

impl SurfaceState {
    pub fn new(thread_id: impl Into<String>) -> Self {
        Self {
            conversation: ConversationState::new(thread_id),
            pending_requests: PendingRequests::default(),
            composer: String::new(),
            activity: SurfaceActivity::Ready,
            notice: None,
        }
    }

    pub fn from_session(session: &SessionState) -> Self {
        Self {
            conversation: ConversationState::from_turns(
                session.thread.id.clone(),
                &session.thread.turns,
            ),
            pending_requests: PendingRequests::default(),
            composer: String::new(),
            activity: if session.active_turn_id.is_some() {
                SurfaceActivity::Working
            } else {
                SurfaceActivity::Ready
            },
            notice: None,
        }
    }

    pub fn conversation(&self) -> &ConversationState {
        &self.conversation
    }

    pub fn conversation_mut(&mut self) -> &mut ConversationState {
        &mut self.conversation
    }

    pub fn pending_requests(&self) -> &PendingRequests {
        &self.pending_requests
    }

    pub fn pending_requests_mut(&mut self) -> &mut PendingRequests {
        &mut self.pending_requests
    }

    pub fn composer(&self) -> &str {
        &self.composer
    }

    pub fn composer_mut(&mut self) -> &mut String {
        &mut self.composer
    }

    pub fn take_composer(&mut self) -> String {
        std::mem::take(&mut self.composer)
    }

    pub fn activity(&self) -> &SurfaceActivity {
        &self.activity
    }

    pub fn set_activity(&mut self, activity: SurfaceActivity) {
        self.activity = activity;
    }

    pub fn set_notice(&mut self, notice: impl Into<String>) {
        self.notice = Some(notice.into());
    }

    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    pub fn drain_committable(&mut self) -> Vec<CommittedBlock> {
        self.conversation.drain_committable()
    }
}

pub fn committed_height(block: &CommittedBlock, width: u16) -> u16 {
    render_block(
        &block.block,
        RenderOptions {
            width,
            expanded: false,
            max_output_lines: 5,
        },
    )
    .height()
    .try_into()
    .unwrap_or(u16::MAX)
}

pub fn paint_committed(block: &CommittedBlock, buffer: &mut Buffer) {
    let text = render_block(
        &block.block,
        RenderOptions {
            width: buffer.area.width,
            expanded: false,
            max_output_lines: 5,
        },
    );
    Paragraph::new(text).render(buffer.area, buffer);
}

pub fn render_surface(
    state: &SurfaceState,
    session: &SessionState,
    area: Rect,
    buffer: &mut Buffer,
) -> Option<Position> {
    render_surface_with_view(state, session, TranscriptView::Live, area, buffer)
}

pub(crate) fn render_surface_with_view(
    state: &SurfaceState,
    session: &SessionState,
    transcript_view: TranscriptView,
    area: Rect,
    buffer: &mut Buffer,
) -> Option<Position> {
    Clear.render(area, buffer);
    if area.is_empty() {
        return None;
    }

    let mut footer = request_lines(state.pending_requests.front(), state.composer(), area.width);
    if footer.is_empty() {
        footer = composer_lines(state, session, area.width);
    }
    let footer_height = u16::try_from(footer.len())
        .unwrap_or(u16::MAX)
        .min(area.height);
    let live_height = area.height.saturating_sub(footer_height);
    let live_lines = conversation_lines(state, transcript_view, area.width);
    let visible_live = live_lines
        .into_iter()
        .rev()
        .take(usize::from(live_height))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();

    Paragraph::new(Text::from(visible_live)).render(
        Rect {
            height: live_height,
            ..area
        },
        buffer,
    );
    Paragraph::new(Text::from(footer.clone())).render(
        Rect {
            y: area.y + live_height,
            height: footer_height,
            ..area
        },
        buffer,
    );

    (state.pending_requests.is_empty()
        || state
            .pending_requests
            .front()
            .is_some_and(request_uses_composer))
    .then(|| {
        let prompt = footer
            .iter()
            .rev()
            .find(|line| line.to_string().starts_with("❯ "))
            .map(Line::width)
            .unwrap_or(2);
        Position::new(
            area.x
                + u16::try_from(prompt)
                    .unwrap_or(area.width)
                    .min(area.width.saturating_sub(1)),
            area.y + area.height.saturating_sub(2),
        )
    })
}

fn conversation_lines(
    state: &SurfaceState,
    transcript_view: TranscriptView,
    width: u16,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let blocks = match transcript_view {
        TranscriptView::Live => state.conversation.live_blocks(),
        TranscriptView::Full => state.conversation.all_blocks(),
    };
    for block in blocks {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.extend(render_block(&block, RenderOptions::compact(width)).lines);
    }
    lines
}

fn composer_lines(state: &SurfaceState, session: &SessionState, _width: u16) -> Vec<Line<'static>> {
    let (marker, status) = match &state.activity {
        SurfaceActivity::Ready => ("◆ ".green(), "Ready".to_string().dim()),
        SurfaceActivity::Working => ("◇ ".magenta(), "Working".to_string().magenta()),
        SurfaceActivity::Interrupted => ("◆ ".magenta(), "Interrupted".to_string().magenta()),
        SurfaceActivity::Disconnected(message) => {
            ("◆ ".red(), format!("Disconnected · {message}").red())
        }
    };
    let mut status_line = vec![
        marker,
        status,
        "  ".into(),
        session.model.clone().dim(),
        " · ".dim(),
        session.model_provider.clone().dim(),
    ];
    if state.conversation.timeline().skipped_events() > 0 {
        status_line.push(
            format!(
                " · {} events skipped",
                state.conversation.timeline().skipped_events()
            )
            .cyan(),
        );
    }
    if let Some(notice) = state.notice.as_deref() {
        status_line.push(" · ".dim());
        status_line.push(notice.to_string().cyan());
    }
    vec![
        status_line.into(),
        vec!["❯ ".cyan(), state.composer.clone().into()].into(),
        "  Enter send · Ctrl+C interrupt · Ctrl+D exit".dim().into(),
    ]
}

fn request_lines(
    request: Option<&PendingRequest>,
    composer: &str,
    _width: u16,
) -> Vec<Line<'static>> {
    let Some(request) = request else {
        return Vec::new();
    };
    let mut lines = vec![vec!["◇ ".magenta(), "Action required".magenta().bold()].into()];
    match request {
        PendingRequest::CommandExecution { params, .. } => {
            lines.push(
                vec![
                    "  $ ".dim(),
                    params
                        .command
                        .clone()
                        .unwrap_or_else(|| "command".to_string())
                        .into(),
                ]
                .into(),
            );
            if let Some(reason) = params.reason.as_deref() {
                lines.push(vec!["  ".into(), reason.to_string().dim()].into());
            }
            let mut choices = "[y] allow · [a] allow session · [n] deny · [esc] cancel".to_string();
            if params.proposed_execpolicy_amendment.is_some() {
                choices.push_str(" · [e] trust pattern");
            }
            if params
                .proposed_network_policy_amendments
                .as_ref()
                .is_some_and(|amendments| !amendments.is_empty())
            {
                choices.push_str(" · [p] network rule");
            }
            lines.push(vec!["  ".into(), choices.dim()].into());
        }
        PendingRequest::FileChange { params, .. } => {
            lines.push(
                vec![
                    "  Edit files".into(),
                    params
                        .reason
                        .as_deref()
                        .map(|reason| format!(" · {reason}"))
                        .unwrap_or_default()
                        .dim(),
                ]
                .into(),
            );
            lines.push(
                "  [y] allow · [a] allow session · [n] deny · [esc] cancel"
                    .dim()
                    .into(),
            );
        }
        PendingRequest::Permissions { params, .. } => {
            lines.push(
                vec![
                    "  Permissions · ".into(),
                    format!("{:?}", params.permissions).dim(),
                ]
                .into(),
            );
            lines.push(
                "  [y] allow turn · [a] allow session · [n] deny"
                    .dim()
                    .into(),
            );
        }
        PendingRequest::UserInput { params, .. } => {
            for question in &params.questions {
                lines.push(vec!["  ".into(), question.question.clone().into()].into());
                if let Some(options) = &question.options {
                    lines.push(
                        vec![
                            "    ".into(),
                            options
                                .iter()
                                .map(|option| option.label.as_str())
                                .collect::<Vec<_>>()
                                .join(" · ")
                                .dim(),
                        ]
                        .into(),
                    );
                }
            }
            lines.push(vec!["❯ ".cyan(), composer.to_string().into()].into());
            lines.push(
                "  Enter answer · separate multiple answers with | · Esc cancel"
                    .dim()
                    .into(),
            );
        }
        PendingRequest::McpElicitation { params, .. } => {
            let message = match &params.request {
                McpServerElicitationRequest::Form { message, .. }
                | McpServerElicitationRequest::Url { message, .. } => message,
            };
            lines.push(vec!["  ".into(), message.clone().into()].into());
            match &params.request {
                McpServerElicitationRequest::Form { .. } => {
                    lines.push(vec!["❯ ".cyan(), composer.to_string().into()].into());
                    lines.push(
                        "  Enter JSON response · [n] decline · [esc] cancel"
                            .dim()
                            .into(),
                    );
                }
                McpServerElicitationRequest::Url { .. } => {
                    lines.push("  [y] accept · [n] decline · [esc] cancel".dim().into());
                }
            }
        }
        PendingRequest::DynamicTool { params, .. } => {
            lines.push(
                vec![
                    "  Client tool · ".into(),
                    params
                        .namespace
                        .as_ref()
                        .map_or_else(
                            || params.tool.clone(),
                            |namespace| format!("{namespace}/{}", params.tool),
                        )
                        .cyan(),
                ]
                .into(),
            );
            lines.push(
                "  Waiting for registered Astral client handler"
                    .dim()
                    .into(),
            );
        }
        PendingRequest::Attestation { .. } => {
            lines.push("  Waiting for client attestation provider".dim().into());
        }
        PendingRequest::LegacyApplyPatch { .. } | PendingRequest::LegacyExecCommand { .. } => {
            lines.push(
                "  Legacy request · this surface uses app-server v2"
                    .red()
                    .into(),
            );
        }
    }
    lines
}

fn request_uses_composer(request: &PendingRequest) -> bool {
    matches!(request, PendingRequest::UserInput { .. })
        || matches!(
            request,
            PendingRequest::McpElicitation { params, .. }
                if matches!(params.request, McpServerElicitationRequest::Form { .. })
        )
}

#[cfg(test)]
#[path = "surface_tests.rs"]
mod tests;
