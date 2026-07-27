mod appearance;

use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::Model;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_protocol::config_types::ModeKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
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
use crate::modal::ModalState;
use crate::model_command::ModelResolveError;
use crate::model_command::ModelSelection;
use crate::permission_picker::PermissionPickerState;
use crate::permission_picker::display_permission_mode;
use crate::render_block;
use crate::slash::SlashCommandId;
use crate::slash::SlashController;
use crate::slash::SlashError;
use crate::slash::SlashInvocation;
use crate::slash::SlashSnapshot;
use crate::theme_picker::ThemePickerState;
use crate::thread_picker::PickerState;
use crate::view::AgentViewLayout;
use crate::view::AgentViewLayoutInput;
use crate::view::AstralTheme;
use crate::view::AstralThemeId;
use crate::view::LayoutConfig;
use crate::view::PaneHeights;
use crate::view::PromptChrome;
use crate::view::ScrollbarConfig;
use crate::view::ShortcutsBar;
use crate::view::SlashMenu;
use crate::view::StatusBar;

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
    token_usage: Option<ThreadTokenUsage>,
    notice: Option<String>,
    scroll_offset: usize,
    slash: SlashController,
    modal: Option<ModalState>,
    thread_picker: Option<PickerState>,
    permission_picker: Option<PermissionPickerState>,
    theme_picker: Option<ThemePickerState>,
    theme: AstralThemeId,
    timeline_visible: bool,
}

impl SurfaceState {
    pub fn new(thread_id: impl Into<String>) -> Self {
        Self {
            conversation: ConversationState::new(thread_id),
            pending_requests: PendingRequests::default(),
            composer: String::new(),
            activity: SurfaceActivity::Ready,
            token_usage: None,
            notice: None,
            scroll_offset: 0,
            slash: SlashController::default(),
            modal: None,
            thread_picker: None,
            permission_picker: None,
            theme_picker: None,
            theme: AstralThemeId::default(),
            timeline_visible: false,
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
            token_usage: None,
            notice: None,
            scroll_offset: 0,
            slash: SlashController::default(),
            modal: None,
            thread_picker: None,
            permission_picker: None,
            theme_picker: None,
            theme: AstralThemeId::default(),
            timeline_visible: false,
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
        let composer = std::mem::take(&mut self.composer);
        self.refresh_slash();
        composer
    }

    pub fn activity(&self) -> &SurfaceActivity {
        &self.activity
    }

    pub fn set_activity(&mut self, activity: SurfaceActivity) {
        self.activity = activity;
        self.refresh_slash();
    }

    pub fn token_usage(&self) -> Option<&ThreadTokenUsage> {
        self.token_usage.as_ref()
    }

    pub fn set_token_usage(&mut self, token_usage: ThreadTokenUsage) {
        self.token_usage = Some(token_usage);
    }

    pub fn set_notice(&mut self, notice: impl Into<String>) {
        self.notice = Some(notice.into());
    }

    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(lines);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn last_agent_response(&self) -> Option<&str> {
        self.conversation.last_agent_response()
    }

    pub fn drain_committable(&mut self) -> Vec<CommittedBlock> {
        self.conversation.drain_committable()
    }

    pub fn slash(&self) -> &SlashSnapshot {
        self.slash.snapshot()
    }

    pub fn refresh_slash(&mut self) {
        let working = matches!(self.activity, SurfaceActivity::Working);
        self.slash.refresh(&self.composer, working);
    }

    pub fn move_slash_selection(&mut self, delta: isize) {
        self.slash.move_selection(delta);
    }

    pub fn close_slash(&mut self) {
        self.slash.close();
    }

    pub fn accept_slash_selection(&mut self) -> bool {
        let working = matches!(self.activity, SurfaceActivity::Working);
        self.slash.accept_selection(&mut self.composer, working)
    }

    pub fn slash_invocation(&self) -> Result<Option<SlashInvocation>, SlashError> {
        self.slash.invocation(
            &self.composer,
            matches!(self.activity, SurfaceActivity::Working),
        )
    }

    pub fn record_slash(&mut self, command: SlashCommandId) {
        self.slash.record(command);
    }

    pub(crate) fn modal(&self) -> Option<&ModalState> {
        self.modal.as_ref()
    }

    pub(crate) fn modal_mut(&mut self) -> Option<&mut ModalState> {
        self.modal.as_mut()
    }

    pub(crate) fn open_modal(&mut self, modal: ModalState) {
        self.modal = Some(modal);
    }

    pub(crate) fn close_modal(&mut self) {
        self.modal = None;
    }

    pub(crate) fn thread_picker(&self) -> Option<&PickerState> {
        self.thread_picker.as_ref()
    }

    pub(crate) fn thread_picker_mut(&mut self) -> Option<&mut PickerState> {
        self.thread_picker.as_mut()
    }

    pub(crate) fn open_thread_picker(&mut self, picker: PickerState) {
        self.thread_picker = Some(picker);
    }

    pub(crate) fn close_thread_picker(&mut self) {
        self.thread_picker = None;
    }

    pub(crate) fn permission_picker(&self) -> Option<&PermissionPickerState> {
        self.permission_picker.as_ref()
    }

    pub(crate) fn permission_picker_mut(&mut self) -> Option<&mut PermissionPickerState> {
        self.permission_picker.as_mut()
    }

    pub(crate) fn open_permission_picker(&mut self, picker: PermissionPickerState) {
        self.permission_picker = Some(picker);
    }

    pub(crate) fn close_permission_picker(&mut self) {
        self.permission_picker = None;
    }

    pub(crate) fn set_model_catalog(
        &mut self,
        models: Vec<Model>,
        current_model: impl Into<String>,
        current_provider: impl Into<String>,
    ) {
        self.slash
            .set_models(models, current_model, current_provider);
        self.refresh_slash();
    }

    pub(crate) fn update_current_model(
        &mut self,
        model: impl Into<String>,
        model_provider: impl Into<String>,
    ) {
        self.slash.update_current_model(model, model_provider);
        self.refresh_slash();
    }

    pub(crate) fn resolve_model(&self, args: &str) -> Result<ModelSelection, ModelResolveError> {
        self.slash.resolve_model(args)
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
    let theme = state.theme();
    buffer.set_style(area, Style::default().bg(theme.bg_base));

    let request = request_lines(state.pending_requests.front(), state.composer(), area.width);
    let prompt_height = if request.is_empty() {
        composer_height(state.composer())
    } else {
        u16::try_from(request.len()).unwrap_or(u16::MAX).max(3)
    };
    let slash = state.slash();
    let max_suggestions = if area.height <= 16 { 2 } else { 6 };
    let slash_height = if request.is_empty() && slash.open {
        u16::try_from(slash.matches.len().min(max_suggestions))
            .unwrap_or(u16::MAX)
            .saturating_add(2)
    } else {
        0
    };
    let turn_status = (request.is_empty() && slash_height == 0)
        .then(|| turn_status_line(state, theme))
        .flatten();
    let turn_count = session.thread.turns.len();
    let timeline_width = appearance::timeline_width(state, area.width, turn_count);
    let layout = AgentViewLayout::compute(AgentViewLayoutInput {
        area,
        layout: LayoutConfig::default(),
        scrollbar: ScrollbarConfig::default(),
        panes: PaneHeights {
            prompt: prompt_height,
            turn_status: u16::from(turn_status.is_some()),
            banner: slash_height,
            prompt_gap: u16::from(area.height > 16),
            shortcuts: 1,
            ..PaneHeights::default()
        },
        timeline_width,
        compact: false,
    });

    render_status_bar(state, session, layout.status_bar, buffer, theme);

    let live_lines = conversation_lines(state, transcript_view, layout.scrollback_content.width);
    let visible_height = usize::from(layout.scrollback_content.height);
    let scroll_offset = match transcript_view {
        TranscriptView::Live => 0,
        TranscriptView::Full => state
            .scroll_offset
            .min(live_lines.len().saturating_sub(visible_height)),
    };
    let end = live_lines.len().saturating_sub(scroll_offset);
    let start = end.saturating_sub(visible_height);
    let visible_live = live_lines[start..end].to_vec();

    Paragraph::new(Text::from(visible_live)).render(layout.scrollback_content, buffer);
    if layout.timeline_width > 0 {
        appearance::render_timeline(
            buffer,
            theme,
            appearance::TimelineFrame {
                scrollback: layout.scrollback,
                rail_x: layout.timeline_x,
                turn_count,
                scroll_offset,
                first_visible_line: start,
                total_lines: live_lines.len(),
            },
        );
    }
    if let Some(turn_status) = turn_status {
        buffer.set_line(
            layout.turn_status.x,
            layout.turn_status.y,
            &turn_status,
            layout.turn_status.width,
        );
    }
    if slash_height > 0 {
        SlashMenu { snapshot: slash }.render(layout.banner, buffer, theme);
    }

    let mode = if session.collaboration_mode.mode == ModeKind::Plan {
        "plan"
    } else {
        display_permission_mode(
            session
                .active_permission_profile
                .as_ref()
                .map(|profile| profile.id.as_str()),
        )
    };
    let cursor = if request.is_empty() {
        let flags = [mode];
        PromptChrome {
            text: state.composer(),
            title: session.thread.name.as_deref(),
            model: &session.model,
            flags: &flags,
            ghost: slash.ghost.as_deref(),
            focused: true,
        }
        .render(layout.prompt, buffer, theme)
    } else {
        Paragraph::new(Text::from(request.clone())).render(layout.prompt, buffer);
        state
            .pending_requests
            .front()
            .filter(|request| request_uses_composer(request))
            .and_then(|_| request_cursor(&request, layout.prompt))
    };

    let default_hints = [("Shift+Tab", "mode")];
    let slash_hints = [("↑/↓", "navigate"), ("Tab", "complete"), ("Esc", "close")];
    ShortcutsBar {
        hints: if slash.open {
            &slash_hints
        } else {
            &default_hints
        },
        right: None,
    }
    .render(layout.shortcuts, buffer, theme);
    if appearance::render_overlay(state, area, buffer, theme) {
        None
    } else {
        cursor
    }
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

fn turn_status_line(state: &SurfaceState, theme: AstralTheme) -> Option<Line<'static>> {
    let (marker, status, color) = match &state.activity {
        SurfaceActivity::Ready => ("◆ ", None, theme.gray),
        SurfaceActivity::Working => ("◇ ", Some("Working".to_string()), theme.accent_running),
        SurfaceActivity::Interrupted => {
            ("◆ ", Some("Interrupted".to_string()), theme.accent_running)
        }
        SurfaceActivity::Disconnected(message) => (
            "◆ ",
            Some(format!("Disconnected · {message}")),
            theme.accent_error,
        ),
    };
    let mut spans = status
        .map(|status| vec![marker.to_string().fg(color), status.fg(color)])
        .unwrap_or_default();
    if let Some(notice) = state.notice.as_deref() {
        if !spans.is_empty() {
            spans.push(" · ".dim());
        }
        spans.push(notice.to_string().cyan());
    }
    if state.conversation.timeline().skipped_events() > 0 {
        if !spans.is_empty() {
            spans.push(" · ".dim());
        }
        spans.push(
            format!(
                "{} events skipped",
                state.conversation.timeline().skipped_events()
            )
            .cyan(),
        );
    }
    if state.scroll_offset > 0 {
        if !spans.is_empty() {
            spans.push(" · ".dim());
        }
        spans.push(format!("history ↑{}", state.scroll_offset).cyan());
    }
    (!spans.is_empty()).then(|| spans.into())
}

fn composer_height(composer: &str) -> u16 {
    let rows = composer.split('\n').count().max(1);
    u16::try_from(rows)
        .unwrap_or(u16::MAX)
        .saturating_add(2)
        .min(8)
}

fn render_status_bar(
    state: &SurfaceState,
    session: &SessionState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    let cwd = collapse_home(&session.thread.cwd.to_string_lossy());
    let branch = session
        .thread
        .git_info
        .as_ref()
        .and_then(|git| git.branch.as_deref())
        .filter(|branch| !branch.is_empty());
    let left = match branch {
        Some(branch) => vec![
            "⎇ ".fg(theme.text_secondary).dim(),
            branch.to_string().fg(theme.text_secondary).dim(),
            "  ".into(),
            cwd.fg(theme.gray_dim),
        ]
        .into(),
        None => cwd.fg(theme.gray_dim).into(),
    };
    let right = state.token_usage().map(|usage| {
        token_status(usage.last.total_tokens, usage.model_context_window)
            .fg(theme.gray)
            .into()
    });
    StatusBar { left, right }.render(area, buffer, theme);
}

fn collapse_home(path: &str) -> String {
    std::env::var("HOME")
        .ok()
        .and_then(|home| path.strip_prefix(&home).map(|suffix| format!("~{suffix}")))
        .unwrap_or_else(|| path.to_string())
}

fn request_cursor(lines: &[Line<'_>], area: Rect) -> Option<Position> {
    lines
        .iter()
        .enumerate()
        .rev()
        .find(|(_, line)| line.to_string().starts_with("❯ "))
        .map(|(row, line)| {
            let width = u16::try_from(line.width()).unwrap_or(u16::MAX);
            Position::new(
                (area.x + width).min(area.right().saturating_sub(1)),
                area.y + u16::try_from(row).unwrap_or(u16::MAX),
            )
        })
}

fn token_status(used: i64, context_window: Option<i64>) -> String {
    let used = compact_token_count(used);
    context_window.map_or(used.clone(), |context_window| {
        format!("{used} / {}", compact_token_count(context_window))
    })
}

fn compact_token_count(tokens: i64) -> String {
    let absolute = tokens.saturating_abs();
    if absolute >= 1_000_000 {
        compact_scaled(tokens, 1_000_000, "M")
    } else if absolute >= 1_000 {
        compact_scaled(tokens, 1_000, "K")
    } else {
        tokens.to_string()
    }
}

fn compact_scaled(value: i64, divisor: i64, suffix: &str) -> String {
    if value % divisor == 0 {
        format!("{}{suffix}", value / divisor)
    } else {
        format!("{:.1}{suffix}", value as f64 / divisor as f64)
    }
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
                    params
                        .reason
                        .as_deref()
                        .unwrap_or("additional access")
                        .to_string()
                        .dim(),
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
