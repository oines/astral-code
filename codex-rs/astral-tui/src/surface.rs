mod appearance;
mod block_viewer;
mod content_actions;
mod history;
mod mentions;
mod overlay;
mod plan_review;
mod pointer;
mod requests;
mod subagent;
mod transcript_cache;

use std::sync::Arc;

use astral_terminal_inline::LinkSpan;
use astral_tui_scrollback::DisplayMode;
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
use crate::block_viewer::BlockViewerState;
use crate::composer::ComposerState;
use crate::history::PromptHistory;
use crate::mcp_form::McpFormState;
use crate::mention::MentionController;
use crate::modal::ModalState;
use crate::model_command::ModelResolveError;
use crate::model_command::ModelSelection;
use crate::permission_picker::PermissionPickerState;
use crate::permission_picker::display_permission_mode;
use crate::plan_review::CompletedPlan;
use crate::plan_review::PlanReviewFocus;
use crate::plan_review::PlanReviewState;
use crate::request_choice::RequestChoiceState;
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
use crate::view::EntryChromeState;
use crate::view::HistoryMenu;
use crate::view::LayoutConfig;
use crate::view::MentionMenu;
use crate::view::PaneHeights;
use crate::view::PlanReviewMouseAction;
use crate::view::PlanReviewMouseState;
use crate::view::PlanReviewPane;
use crate::view::PromptChrome;
use crate::view::ScrollbackFrame;
use crate::view::ScrollbackMouseAction;
use crate::view::ScrollbackPane;
use crate::view::ScrollbackState;
use crate::view::ScrollbackViewport;
use crate::view::ScrollbarConfig;
use crate::view::ShortcutsBar;
use crate::view::SlashMenu;
use crate::view::StatusBar;
use crate::view::TranscriptLayout;
use crate::view::prompt_height;
use crate::view::render_committed_block;
use crate::view::render_entry_chrome;
use crate::view::render_follow_indicator;
use crate::view::render_transcript;

use self::pointer::SurfacePointerState;
use self::subagent::SubagentViewState;
use self::transcript_cache::TranscriptCache;
use self::transcript_cache::TranscriptCacheKey;

pub(crate) use self::overlay::ActiveOverlay;

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
    ReadOnly,
}

#[derive(Debug)]
pub struct SurfaceState {
    conversation: ConversationState,
    pending_requests: PendingRequests,
    request_choice: RequestChoiceState,
    request_user_input: RequestUserInputState,
    mcp_form: McpFormState,
    composer: ComposerState,
    activity: SurfaceActivity,
    token_usage: Option<ThreadTokenUsage>,
    notice: Option<String>,
    scrollback: ScrollbackState,
    history: PromptHistory,
    slash: SlashController,
    mentions: MentionController,
    block_viewer: Option<BlockViewerState>,
    subagent_view: Option<Box<SubagentViewState>>,
    modal: Option<ModalState>,
    thread_picker: Option<PickerState>,
    permission_picker: Option<PermissionPickerState>,
    theme_picker: Option<ThemePickerState>,
    completed_plan: Option<CompletedPlan>,
    plan_review: Option<PlanReviewState>,
    plan_review_mouse: PlanReviewMouseState,
    pointer_areas: SurfacePointerState,
    theme: AstralThemeId,
    color_level: ColorLevel,
    timeline_visible: bool,
    transcript_cache: TranscriptCache,
}

impl SurfaceState {
    pub fn new(thread_id: impl Into<String>) -> Self {
        Self {
            conversation: ConversationState::new(thread_id),
            pending_requests: PendingRequests::default(),
            request_choice: RequestChoiceState::default(),
            request_user_input: RequestUserInputState::default(),
            mcp_form: McpFormState::default(),
            composer: ComposerState::default(),
            activity: SurfaceActivity::Ready,
            token_usage: None,
            notice: None,
            scrollback: ScrollbackState::default(),
            history: PromptHistory::default(),
            slash: SlashController::default(),
            mentions: MentionController::default(),
            block_viewer: None,
            subagent_view: None,
            modal: None,
            thread_picker: None,
            permission_picker: None,
            theme_picker: None,
            completed_plan: None,
            plan_review: None,
            plan_review_mouse: PlanReviewMouseState::default(),
            pointer_areas: SurfacePointerState::default(),
            theme: AstralThemeId::default(),
            color_level: ColorLevel::default(),
            timeline_visible: false,
            transcript_cache: TranscriptCache::default(),
        }
    }

    pub fn from_session(session: &SessionState) -> Self {
        Self {
            conversation: ConversationState::from_turns(
                session.thread.id.clone(),
                &session.thread.turns,
            ),
            pending_requests: PendingRequests::default(),
            request_choice: RequestChoiceState::default(),
            request_user_input: RequestUserInputState::default(),
            mcp_form: McpFormState::default(),
            composer: ComposerState::default(),
            activity: if session.active_turn_id.is_some() {
                SurfaceActivity::Working
            } else {
                SurfaceActivity::Ready
            },
            token_usage: None,
            notice: None,
            scrollback: ScrollbackState::default(),
            history: PromptHistory::from_turns(&session.thread.turns),
            slash: SlashController::default(),
            mentions: MentionController::default(),
            block_viewer: None,
            subagent_view: None,
            modal: None,
            thread_picker: None,
            permission_picker: None,
            theme_picker: None,
            completed_plan: None,
            plan_review: None,
            plan_review_mouse: PlanReviewMouseState::default(),
            pointer_areas: SurfacePointerState::default(),
            theme: AstralThemeId::default(),
            color_level: ColorLevel::default(),
            timeline_visible: false,
            transcript_cache: TranscriptCache::default(),
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
        self.scrollback.scroll_up(lines);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scrollback.scroll_down(lines);
    }

    pub fn page_up(&mut self) {
        self.scrollback.page_up();
    }

    pub fn page_down(&mut self) {
        self.scrollback.page_down();
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scrollback.scroll_to_bottom();
    }

    pub fn scroll_offset(&self) -> usize {
        self.scrollback.distance_from_bottom()
    }

    pub(crate) fn focus_scrollback(&mut self) -> bool {
        self.cancel_history();
        self.slash.close();
        self.mentions.dismiss(self.composer.text());
        self.clear_completion_menu();
        self.scrollback.focus_scrollback()
    }

    pub(crate) fn focus_prompt(&mut self) {
        self.scrollback.focus_prompt();
    }

    pub(crate) fn scrollback_focused(&self) -> bool {
        self.scrollback.is_focused()
    }

    pub(crate) fn move_entry_selection(&mut self, delta: isize) {
        self.scrollback.move_selection(delta);
    }

    pub(crate) fn half_page_up(&mut self) {
        self.scrollback.half_page_up();
    }

    pub(crate) fn half_page_down(&mut self) {
        self.scrollback.half_page_down();
    }

    pub(crate) fn goto_scrollback_top(&mut self) {
        self.scrollback.goto_top();
    }

    pub(crate) fn goto_scrollback_bottom(&mut self) {
        self.scrollback.goto_bottom();
    }

    pub(crate) fn next_turn(&mut self) {
        self.scrollback.next_turn();
    }

    pub(crate) fn previous_turn(&mut self) {
        self.scrollback.previous_turn();
    }

    pub(crate) fn next_response(&mut self) {
        self.scrollback.next_response();
    }

    pub(crate) fn previous_response(&mut self) {
        self.scrollback.previous_response();
    }

    pub(crate) fn toggle_selected_entry(&mut self) {
        self.scrollback.toggle_selected();
    }

    pub(crate) fn expand_selected_entry(&mut self) {
        self.scrollback.expand_selected();
    }

    pub(crate) fn collapse_selected_entry(&mut self) {
        self.scrollback.collapse_selected();
    }

    pub(crate) fn toggle_all_entries(&mut self) {
        self.scrollback.toggle_all_entries();
    }

    pub(crate) fn toggle_all_thinking(&mut self) {
        self.scrollback.toggle_all_thinking();
    }

    pub(crate) fn handle_scrollback_mouse(
        &mut self,
        mouse: crossterm::event::MouseEvent,
    ) -> ScrollbackMouseAction {
        self.scrollback.handle_mouse(mouse)
    }

    pub(crate) fn cycle_scrollback_link(&mut self, forward: bool) -> bool {
        self.scrollback.cycle_link(forward)
    }

    pub(crate) fn highlighted_scrollback_link(&self) -> Option<crate::LinkTarget> {
        self.scrollback.highlighted_link_target()
    }

    pub(crate) fn frame_link_spans(&self) -> Vec<LinkSpan> {
        self.scrollback.frame_link_spans()
    }

    pub(crate) fn handle_plan_review_mouse(
        &mut self,
        mouse: crossterm::event::MouseEvent,
    ) -> PlanReviewMouseAction {
        self.plan_review_mouse.handle_mouse(mouse)
    }

    pub(crate) fn clear_scrollback_selection(&mut self) -> bool {
        self.scrollback.clear_selection()
    }

    pub(crate) fn open_scrollback_search(&mut self) -> bool {
        self.scrollback.open_search()
    }

    pub(crate) fn handle_scrollback_search_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Option<()> {
        let query_changed = self.scrollback.handle_search_key(key)?;
        if query_changed {
            self.submit_scrollback_search();
        }
        Some(())
    }

    pub(crate) fn paste_scrollback_search(&mut self, text: &str) -> Option<()> {
        let query_changed = self.scrollback.paste_search(text)?;
        if query_changed {
            self.submit_scrollback_search();
        }
        Some(())
    }

    pub(crate) fn poll_scrollback_search(&mut self) -> bool {
        self.scrollback.poll_search()
    }

    pub(crate) fn scrollback_search_pending(&self) -> bool {
        self.scrollback.search_pending()
    }

    fn submit_scrollback_search(&mut self) {
        let generation = self.conversation.content_generation();
        let turns = self
            .scrollback
            .search_needs_corpus(generation)
            .then(|| self.conversation.all_turns());
        self.scrollback.submit_search(generation, turns.as_deref());
    }

    pub(crate) fn scrollback_selection_expiry(&self) -> Option<std::time::Instant> {
        self.scrollback.selection_expiry()
    }

    pub(crate) fn expire_scrollback_selection(&mut self) -> bool {
        self.scrollback.expire_selection(std::time::Instant::now())
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

    pub(crate) fn select_slash(&mut self, index: usize) {
        self.slash.select(index);
    }

    pub fn close_slash(&mut self) {
        self.slash.close();
    }

    pub fn accept_slash_selection(&mut self) -> bool {
        let Some(completion) = self.slash.accept_selection(self.slash_command_state()) else {
            return false;
        };
        self.composer.replace(completion);
        self.slash.close();
        self.refresh_mentions();
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
    let read_only = transcript_view == TranscriptView::ReadOnly;
    if !read_only {
        state.sync_request_states();
    }

    let has_request = !read_only && state.pending_requests.front().is_some();
    let request_focused = !read_only
        && state.pending_requests.front().is_some_and(|request| {
            !crate::request_choice::is_simple_request(request) || !state.scrollback_focused()
        });
    let plan_review = (!read_only).then(|| state.plan_review().cloned()).flatten();
    let history = state.history().clone();
    let prompt_height = if read_only {
        0
    } else {
        state.pending_requests.front().map_or_else(
            || {
                if plan_review
                    .as_ref()
                    .is_some_and(|review| review.focus() == PlanReviewFocus::Decision)
                {
                    0
                } else {
                    let (text, cursor) = if history.open && history.browse {
                        (history.saved_text.as_str(), history.saved_text.len())
                    } else {
                        (state.composer(), state.composer_cursor())
                    };
                    prompt_height(text, cursor, area.width)
                }
            },
            |request| {
                RequestPane::new(
                    request,
                    state.request_choice(),
                    state.request_user_input(),
                    state.mcp_form(),
                    request_focused,
                )
                .height(area.height)
            },
        )
    };
    let slash = state.slash().clone();
    let mentions = state.mentions().clone();
    let max_suggestions = if area.height <= 16 { 2 } else { 6 };
    let completion_rows = if history.open {
        history.matches.len().max(1)
    } else if mentions.open {
        mentions.matches.len()
    } else if slash.open {
        slash.matches.len()
    } else {
        0
    };
    let completion_height =
        if !read_only && !has_request && plan_review.is_none() && completion_rows > 0 {
            u16::try_from(completion_rows.min(max_suggestions))
                .unwrap_or(u16::MAX)
                .saturating_add(2)
        } else {
            0
        };
    let banner_height = plan_review.as_ref().map_or(completion_height, |review| {
        PlanReviewPane { state: review }.height()
    });
    let turn_status = (!has_request && banner_height == 0)
        .then(|| turn_status_line(state, theme))
        .flatten();
    let turn_count = session.thread.turns.len();
    let timeline_width = if read_only {
        0
    } else {
        appearance::timeline_width(state, area.width, turn_count)
    };
    let layout = AgentViewLayout::compute(AgentViewLayoutInput {
        area,
        layout: LayoutConfig::default(),
        scrollbar: ScrollbarConfig::default(),
        panes: PaneHeights {
            prompt: prompt_height,
            turn_status: u16::from(turn_status.is_some()),
            banner: banner_height,
            prompt_gap: u16::from(area.height > 16 && prompt_height > 0),
            shortcuts: 1,
            ..PaneHeights::default()
        },
        timeline_width,
        compact: false,
    });
    let search_rows = state
        .scrollback
        .search_reserved_rows(layout.scrollback.height);
    let scrollback_area = Rect {
        height: layout.scrollback.height.saturating_sub(search_rows),
        ..layout.scrollback
    };
    let scrollback_content = Rect {
        height: layout.scrollback_content.height.saturating_sub(search_rows),
        ..layout.scrollback_content
    };
    let search_area = Rect::new(
        layout.scrollback.x,
        scrollback_area.bottom(),
        layout.scrollback.width,
        search_rows,
    );
    state.observe_pointer_areas(scrollback_area, layout.prompt);

    render_status_bar(state, session, layout.status_bar, buffer, theme);

    let transcript = conversation_layout(state, transcript_view, scrollback_content.width);
    let viewport = match transcript_view {
        TranscriptView::Live => ScrollbackViewport::measure(
            transcript.lines.len(),
            usize::from(scrollback_content.height),
            /*distance_from_bottom*/ 0,
        ),
        TranscriptView::Full | TranscriptView::ReadOnly => state.scrollback.prepare(
            &transcript,
            scrollback_content.width,
            usize::from(scrollback_content.height),
        ),
    };
    let scrollbar_area = Rect::new(
        layout.scrollbar_x,
        scrollback_area.y,
        1,
        scrollback_area.height,
    );
    let viewport = ScrollbackPane {
        lines: &transcript.lines,
        viewport,
    }
    .render(scrollback_content, scrollbar_area, buffer, theme);
    state.scrollback.render_search_highlights(
        &transcript.lines[viewport.first_visible_line..viewport.end_visible_line],
        scrollback_content,
        buffer,
    );
    render_entry_chrome(
        &transcript,
        viewport,
        scrollback_content,
        EntryChromeState {
            selected_id: (transcript_view != TranscriptView::Live)
                .then(|| state.scrollback.selected_id())
                .flatten(),
            hovered_id: state.scrollback.hovered_id(),
            hovered_mode: state.scrollback.hovered_mode(),
        },
        buffer,
        theme,
    );
    if transcript_view != TranscriptView::Live {
        state.scrollback.observe_frame(
            ScrollbackFrame {
                layout: &transcript,
                viewport,
                area: scrollback_content,
                scrollbar_area,
                cwd: &session.thread.cwd,
            },
            buffer,
            theme,
        );
    } else {
        state.scrollback.clear_frame();
    }
    if search_rows == 0 {
        render_follow_indicator(
            viewport,
            scrollback_area,
            scrollback_area.bottom(),
            buffer,
            theme,
        );
    }
    let search_cursor = state.scrollback.render_search(search_area, buffer, theme);
    if layout.timeline_width > 0 {
        appearance::render_timeline(
            buffer,
            theme,
            appearance::TimelineFrame {
                scrollback: scrollback_area,
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
    if let Some(review) = plan_review.as_ref() {
        state
            .plan_review_mouse
            .observe(layout.banner, review.focus());
        state.clear_completion_menu();
        PlanReviewPane { state: review }.render(layout.banner, buffer, theme);
    } else if completion_height > 0 {
        state.plan_review_mouse.clear();
        let hovered = state.completion_hovered();
        let completion_frame = if history.open {
            HistoryMenu {
                snapshot: &history,
                hovered,
            }
            .render(layout.banner, buffer, theme)
        } else if mentions.open {
            MentionMenu {
                snapshot: &mentions,
                hovered,
            }
            .render(layout.banner, buffer, theme)
        } else {
            SlashMenu {
                snapshot: &slash,
                hovered,
            }
            .render(layout.banner, buffer, theme)
        };
        state.observe_completion_menu(completion_frame);
    } else {
        state.plan_review_mouse.clear();
        state.clear_completion_menu();
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
    let request_pane = if read_only {
        None
    } else {
        state.pending_requests.front().map(|request| {
            RequestPane::new(
                request,
                state.request_choice(),
                state.request_user_input(),
                state.mcp_form(),
                request_focused,
            )
        })
    };
    let revising_plan = plan_review
        .as_ref()
        .is_some_and(|review| review.focus() == PlanReviewFocus::Revision);
    let prompt_focused = !read_only
        && ((request_pane.is_some() && request_focused)
            || revising_plan
            || !state.scrollback_focused());
    let cursor = if read_only {
        None
    } else if let Some(pane) = request_pane {
        pane.render(layout.prompt, buffer, theme)
    } else if plan_review
        .as_ref()
        .is_some_and(|review| review.focus() == PlanReviewFocus::Decision)
    {
        None
    } else {
        let flags = [mode];
        PromptChrome {
            text: state.composer(),
            cursor_byte: state.composer_cursor(),
            title: if revising_plan {
                Some("Plan feedback")
            } else {
                session.thread.name.as_deref()
            },
            model: &session.model,
            flags: &flags,
            ghost: (!revising_plan && !history.open)
                .then_some(slash.ghost.as_deref())
                .flatten(),
            focused: prompt_focused,
        }
        .render(layout.prompt, buffer, theme)
    };

    let default_hints = [("Shift+Tab", "mode"), ("Ctrl+.", "shortcuts")];
    let read_only_hints = [
        ("Esc/q", "back"),
        ("↑/↓", "navigate"),
        ("Enter", "open"),
        ("/", "search"),
    ];
    let fold_action = if state.scrollback.selected_mode() == Some(DisplayMode::Expanded) {
        "collapse"
    } else {
        "expand"
    };
    let thinking_action = state.scrollback.thinking_fold_label();
    let group_hints = [
        ("Enter", fold_action),
        ("Ctrl+e", thinking_action),
        ("Tab", "prompt"),
    ];
    let entry_hints = [
        ("e", fold_action),
        ("Enter", "open"),
        ("Ctrl+e", thinking_action),
        ("Tab", "prompt"),
    ];
    let plain_entry_hints = [
        ("Enter", "open"),
        ("Ctrl+e", thinking_action),
        ("Tab", "prompt"),
    ];
    let mut scrollback_hints = if state.scrollback.selected_is_group_header() {
        group_hints.to_vec()
    } else if !state.scrollback.selected_is_foldable() {
        plain_entry_hints.to_vec()
    } else {
        entry_hints.to_vec()
    };
    if state.scrollback.has_visible_links() {
        scrollback_hints.insert(0, ("o/O", "links"));
    }
    if !state.scrollback.selected_is_group_header() {
        let insert_at = scrollback_hints
            .iter()
            .position(|(key, _)| *key == "Ctrl+e")
            .unwrap_or(scrollback_hints.len());
        if state.selected_supports_copy() {
            scrollback_hints.insert(insert_at, ("y", "copy"));
        }
        if let Some(label) = state.selected_copy_meta_label() {
            let insert_at = scrollback_hints
                .iter()
                .position(|(key, _)| *key == "Ctrl+e")
                .unwrap_or(scrollback_hints.len());
            scrollback_hints.insert(insert_at, ("Y", label));
        }
    }
    let mention_hints = [("↑/↓", "navigate"), ("Tab", "select"), ("Esc", "close")];
    let slash_hints = [("↑/↓", "navigate"), ("Tab", "complete"), ("Esc", "close")];
    let history_hints = [
        ("↑/↓", "navigate"),
        ("PgUp/PgDn", "page"),
        ("Enter", "select"),
        ("Esc", "cancel"),
    ];
    let plan_hints = [
        ("↑/↓", "navigate"),
        ("Enter", "select"),
        ("s", "revise"),
        ("Esc", "keep planning"),
    ];
    let revision_hints = [("Enter", "request changes"), ("Esc", "back")];
    let search_composing_hints = [("↑/↓", "matches"), ("Enter", "accept"), ("Esc", "close")];
    let search_browsing_hints = [("n/N", "matches"), ("/", "new search"), ("Esc", "close")];
    let search_hints = if state.scrollback.search_composing() {
        &search_composing_hints
    } else {
        &search_browsing_hints
    };
    let overlay_hints: &[(&str, &str)] = &[];
    ShortcutsBar {
        hints: if state.active_overlay().is_some() {
            overlay_hints
        } else if read_only {
            &read_only_hints
        } else if request_pane.is_some() && !request_focused {
            &scrollback_hints
        } else if let Some(pane) = request_pane {
            pane.shortcuts()
        } else if revising_plan {
            &revision_hints
        } else if plan_review.is_some() {
            &plan_hints
        } else if state.scrollback.search_active() {
            search_hints
        } else if state.scrollback_focused() {
            &scrollback_hints
        } else if history.open {
            &history_hints
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
    let choice_hit_rows = request_pane
        .map(|pane| pane.choice_hit_rows(layout.prompt))
        .unwrap_or_default();
    let user_input_hit_rows = request_pane
        .map(|pane| pane.user_input_hit_rows(layout.prompt))
        .unwrap_or_default();
    let mcp_form_hit_rows = request_pane
        .map(|pane| pane.mcp_form_hit_rows(layout.prompt))
        .unwrap_or_default();
    state.request_choice.observe_rows(choice_hit_rows);
    state.request_user_input.observe_rows(user_input_hit_rows);
    state.mcp_form.observe_rows(mcp_form_hit_rows);
    if appearance::render_overlay(state, area, buffer, theme) {
        None
    } else if search_cursor.is_some() {
        search_cursor
    } else if prompt_focused {
        cursor
    } else {
        None
    }
}

fn conversation_layout(
    state: &mut SurfaceState,
    transcript_view: TranscriptView,
    width: u16,
) -> Arc<TranscriptLayout> {
    let theme = state.theme();
    let mut key = TranscriptCacheKey {
        view: transcript_view,
        content_generation: state.conversation.content_generation(),
        display_revision: state.scrollback.display().render_revision(),
        width,
        theme,
    };
    if let Some(layout) = state.transcript_cache.get(key) {
        return layout;
    }
    let turns = match transcript_view {
        TranscriptView::Live => state.conversation.live_turns(),
        TranscriptView::Full | TranscriptView::ReadOnly => state.conversation.all_turns(),
    };
    state.scrollback.observe_entries(&turns);
    key.display_revision = state.scrollback.display().render_revision();
    let layout = render_transcript(&turns, width, theme, state.scrollback.display());
    state.transcript_cache.store(key, layout)
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
    if state.conversation.skipped_events() > 0 {
        if !spans.is_empty() {
            spans.push(" · ".dim());
        }
        spans.push(format!("{} events skipped", state.conversation.skipped_events()).cyan());
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
