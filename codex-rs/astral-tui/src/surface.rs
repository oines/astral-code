mod appearance;
mod mentions;
mod requests;

use codex_app_server_protocol::Model;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_protocol::config_types::ModeKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::CommittedBlock;
use crate::ConversationState;
use crate::PendingRequests;
use crate::SessionState;
use crate::composer::ComposerState;
use crate::mention::MentionController;
use crate::modal::ModalState;
use crate::model_command::ModelResolveError;
use crate::model_command::ModelSelection;
use crate::permission_picker::PermissionPickerState;
use crate::permission_picker::display_permission_mode;
use crate::request_pane::RequestPane;
use crate::request_user_input::RequestUserInputState;
use crate::slash::SlashCommandId;
use crate::slash::SlashCommandState;
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
use crate::view::ColorLevel;
use crate::view::LayoutConfig;
use crate::view::MentionMenu;
use crate::view::PaneHeights;
use crate::view::PromptChrome;
use crate::view::ScrollbackNavigation;
use crate::view::ScrollbackPane;
use crate::view::ScrollbackSelection;
use crate::view::ScrollbackSelectionAction;
use crate::view::ScrollbackViewport;
use crate::view::ScrollbarConfig;
use crate::view::ShortcutsBar;
use crate::view::SlashMenu;
use crate::view::StatusBar;
use crate::view::TranscriptLayout;
use crate::view::prompt_height;
use crate::view::render_committed_block;
use crate::view::render_follow_indicator;
use crate::view::render_transcript;

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
    request_user_input: RequestUserInputState,
    composer: ComposerState,
    activity: SurfaceActivity,
    token_usage: Option<ThreadTokenUsage>,
    notice: Option<String>,
    scrollback: ScrollbackNavigation,
    selection: ScrollbackSelection,
    slash: SlashController,
    mentions: MentionController,
    modal: Option<ModalState>,
    thread_picker: Option<PickerState>,
    permission_picker: Option<PermissionPickerState>,
    theme_picker: Option<ThemePickerState>,
    theme: AstralThemeId,
    color_level: ColorLevel,
    timeline_visible: bool,
}

impl SurfaceState {
    pub fn new(thread_id: impl Into<String>) -> Self {
        Self {
            conversation: ConversationState::new(thread_id),
            pending_requests: PendingRequests::default(),
            request_user_input: RequestUserInputState::default(),
            composer: ComposerState::default(),
            activity: SurfaceActivity::Ready,
            token_usage: None,
            notice: None,
            scrollback: ScrollbackNavigation::default(),
            selection: ScrollbackSelection::default(),
            slash: SlashController::default(),
            mentions: MentionController::default(),
            modal: None,
            thread_picker: None,
            permission_picker: None,
            theme_picker: None,
            theme: AstralThemeId::default(),
            color_level: ColorLevel::default(),
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
            request_user_input: RequestUserInputState::default(),
            composer: ComposerState::default(),
            activity: if session.active_turn_id.is_some() {
                SurfaceActivity::Working
            } else {
                SurfaceActivity::Ready
            },
            token_usage: None,
            notice: None,
            scrollback: ScrollbackNavigation::default(),
            selection: ScrollbackSelection::default(),
            slash: SlashController::default(),
            mentions: MentionController::default(),
            modal: None,
            thread_picker: None,
            permission_picker: None,
            theme_picker: None,
            theme: AstralThemeId::default(),
            color_level: ColorLevel::default(),
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
        self.composer.text()
    }

    pub fn composer_cursor(&self) -> usize {
        self.composer.cursor()
    }

    pub fn set_composer(&mut self, text: impl Into<String>) {
        self.composer.replace(text);
        self.refresh_composer_completions();
    }

    pub(crate) fn composer_state_mut(&mut self) -> &mut ComposerState {
        &mut self.composer
    }

    pub fn take_composer(&mut self) -> String {
        let composer = self.composer.take();
        self.refresh_composer_completions();
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
        self.selection.clear_persistent();
        self.scrollback.scroll_up(lines);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.selection.clear_persistent();
        self.scrollback.scroll_down(lines);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.selection.clear_persistent();
        self.scrollback.scroll_to_bottom();
    }

    pub fn scroll_offset(&self) -> usize {
        self.scrollback.distance_from_bottom()
    }

    pub(crate) fn handle_scrollback_mouse(
        &mut self,
        mouse: crossterm::event::MouseEvent,
    ) -> Option<String> {
        match self.selection.handle_mouse(mouse) {
            ScrollbackSelectionAction::ScrollUp => self.scroll_up(/*lines*/ 1),
            ScrollbackSelectionAction::ScrollDown => self.scroll_down(/*lines*/ 1),
            ScrollbackSelectionAction::Copy(text) => return Some(text),
            ScrollbackSelectionAction::Ignored | ScrollbackSelectionAction::Redraw => {}
        }
        None
    }

    pub(crate) fn clear_scrollback_selection(&mut self) -> bool {
        self.selection.clear()
    }

    pub(crate) fn scrollback_selection_expiry(&self) -> Option<std::time::Instant> {
        self.selection.expiry()
    }

    pub(crate) fn expire_scrollback_selection(&mut self) -> bool {
        self.selection.expire_if_due(std::time::Instant::now())
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
        if self.composer.cursor() == self.composer.text().len() {
            self.slash
                .refresh(self.composer.text(), self.slash_command_state());
        } else {
            self.slash.close();
        }
    }

    pub fn move_slash_selection(&mut self, delta: isize) {
        self.slash.move_selection(delta);
    }

    pub fn close_slash(&mut self) {
        self.slash.close();
    }

    pub fn accept_slash_selection(&mut self) -> bool {
        let Some(completion) = self.slash.accept_selection(self.slash_command_state()) else {
            return false;
        };
        self.composer.replace(completion);
        true
    }

    pub fn slash_invocation(&self) -> Result<Option<SlashInvocation>, SlashError> {
        self.slash
            .invocation(self.composer.text(), self.slash_command_state())
    }

    pub fn record_slash(&mut self, command: SlashCommandId) {
        self.slash.record(command);
    }

    fn slash_command_state(&self) -> SlashCommandState {
        match self.activity {
            SurfaceActivity::Ready | SurfaceActivity::Interrupted => SlashCommandState::Idle,
            SurfaceActivity::Working => SlashCommandState::Working,
            SurfaceActivity::Disconnected(_) => SlashCommandState::Disconnected,
        }
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
    render_committed_block(block, width, AstralTheme::default())
        .len()
        .try_into()
        .unwrap_or(u16::MAX)
}

pub fn paint_committed(block: &CommittedBlock, buffer: &mut Buffer) {
    paint_committed_with_theme(block, buffer, AstralTheme::default());
}

pub(crate) fn paint_committed_with_theme(
    block: &CommittedBlock,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    let lines = render_committed_block(block, buffer.area.width, theme);
    Paragraph::new(lines).render(buffer.area, buffer);
}

pub fn render_surface(
    state: &mut SurfaceState,
    session: &SessionState,
    area: Rect,
    buffer: &mut Buffer,
) -> Option<Position> {
    render_surface_with_view(state, session, TranscriptView::Live, area, buffer)
}

pub(crate) fn render_surface_with_view(
    state: &mut SurfaceState,
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
    state.sync_request_user_input();

    let has_request = state.pending_requests.front().is_some();
    let prompt_height = state.pending_requests.front().map_or_else(
        || prompt_height(state.composer(), state.composer_cursor(), area.width),
        |request| {
            RequestPane::new(
                request,
                state.request_user_input(),
                state.composer(),
                state.composer_cursor(),
            )
            .height(area.height)
        },
    );
    let slash = state.slash().clone();
    let mentions = state.mentions().clone();
    let max_suggestions = if area.height <= 16 { 2 } else { 6 };
    let completion_rows = if mentions.open {
        mentions.matches.len()
    } else if slash.open {
        slash.matches.len()
    } else {
        0
    };
    let completion_height = if !has_request && completion_rows > 0 {
        u16::try_from(completion_rows.min(max_suggestions))
            .unwrap_or(u16::MAX)
            .saturating_add(2)
    } else {
        0
    };
    let turn_status = (!has_request && completion_height == 0)
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
            banner: completion_height,
            prompt_gap: u16::from(area.height > 16),
            shortcuts: 1,
            ..PaneHeights::default()
        },
        timeline_width,
        compact: false,
    });

    render_status_bar(state, session, layout.status_bar, buffer, theme);

    let transcript = conversation_layout(state, transcript_view, layout.scrollback_content.width);
    let viewport = match transcript_view {
        TranscriptView::Live => ScrollbackViewport::measure(
            transcript.lines.len(),
            usize::from(layout.scrollback_content.height),
            /*distance_from_bottom*/ 0,
        ),
        TranscriptView::Full => state.scrollback.prepare(
            &transcript,
            layout.scrollback_content.width,
            usize::from(layout.scrollback_content.height),
        ),
    };
    let viewport = ScrollbackPane {
        lines: &transcript.lines,
        viewport,
    }
    .render(
        layout.scrollback_content,
        Rect::new(
            layout.scrollbar_x,
            layout.scrollback.y,
            1,
            layout.scrollback.height,
        ),
        buffer,
        theme,
    );
    if transcript_view == TranscriptView::Full {
        state.selection.render(
            &transcript,
            viewport,
            layout.scrollback_content,
            buffer,
            theme,
        );
    }
    render_follow_indicator(
        viewport,
        layout.scrollback,
        layout.scrollback.bottom(),
        buffer,
        theme,
    );
    if layout.timeline_width > 0 {
        appearance::render_timeline(
            buffer,
            theme,
            appearance::TimelineFrame {
                scrollback: layout.scrollback,
                rail_x: layout.timeline_x,
                turn_count,
                scroll_offset: viewport.first_visible_line,
                first_visible_line: viewport.first_visible_line,
                total_lines: transcript.lines.len(),
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
    if completion_height > 0 {
        if mentions.open {
            MentionMenu {
                snapshot: &mentions,
            }
            .render(layout.banner, buffer, theme);
        } else {
            SlashMenu { snapshot: &slash }.render(layout.banner, buffer, theme);
        }
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
    let request_pane = state.pending_requests.front().map(|request| {
        RequestPane::new(
            request,
            state.request_user_input(),
            state.composer(),
            state.composer_cursor(),
        )
    });
    let cursor = if let Some(pane) = request_pane {
        pane.render(layout.prompt, buffer, theme)
    } else {
        let flags = [mode];
        PromptChrome {
            text: state.composer(),
            cursor_byte: state.composer_cursor(),
            title: session.thread.name.as_deref(),
            model: &session.model,
            flags: &flags,
            ghost: slash.ghost.as_deref(),
            focused: true,
        }
        .render(layout.prompt, buffer, theme)
    };

    let default_hints = [("Shift+Tab", "mode"), ("Ctrl+.", "shortcuts")];
    let mention_hints = [("↑/↓", "navigate"), ("Tab", "select"), ("Esc", "close")];
    let slash_hints = [("↑/↓", "navigate"), ("Tab", "complete"), ("Esc", "close")];
    ShortcutsBar {
        hints: if let Some(pane) = request_pane {
            pane.shortcuts()
        } else if mentions.open {
            &mention_hints
        } else if slash.open {
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

fn conversation_layout(
    state: &SurfaceState,
    transcript_view: TranscriptView,
    width: u16,
) -> TranscriptLayout {
    let turns = match transcript_view {
        TranscriptView::Live => state.conversation.live_turns(),
        TranscriptView::Full => state.conversation.all_turns(),
    };
    render_transcript(&turns, width, state.theme())
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
    if state.scroll_offset() > 0 {
        if !spans.is_empty() {
            spans.push(" · ".dim());
        }
        spans.push(format!("history ↑{}", state.scroll_offset()).cyan());
    }
    (!spans.is_empty()).then(|| spans.into())
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

#[cfg(test)]
#[path = "surface_tests.rs"]
mod tests;
