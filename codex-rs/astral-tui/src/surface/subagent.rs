//! Read-only child-thread view opened from an Astral subagent transcript node.

use astral_tui_scrollback::PresentationBlock;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnStatus;
use crossterm::event::MouseEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;

use super::SurfaceState;
use super::TranscriptView;
use super::render_surface_with_view;
use crate::SessionState;
use crate::modal::ModalPointerAction;
use crate::modal::ModalPointerState;
use crate::view::AstralTheme;
use crate::view::ModalHeight;
use crate::view::render_modal_close_button;
use crate::view::render_modal_frame_with_geometry;

#[derive(Debug)]
pub(super) struct SubagentViewState {
    child: Box<SurfaceState>,
    session: SessionState,
    pointer: ModalPointerState,
}

impl SubagentViewState {
    fn new(thread: Thread, parent_session: &SessionState, parent_surface: &SurfaceState) -> Self {
        let mut session = parent_session.clone();
        session.model_provider.clone_from(&thread.model_provider);
        session.active_turn_id = thread
            .turns
            .iter()
            .rev()
            .find(|turn| turn.status == TurnStatus::InProgress)
            .map(|turn| turn.id.clone());
        session.thread = thread;

        let mut child = SurfaceState::from_session(&session);
        child.set_theme(parent_surface.theme_id());
        child.set_color_level(parent_surface.color_level());
        let turns = child.conversation.all_turns();
        child.scrollback.observe_entries(&turns);
        child.focus_scrollback();
        Self {
            child: Box::new(child),
            session,
            pointer: ModalPointerState::default(),
        }
    }

    fn title(&self) -> String {
        let thread = &self.session.thread;
        let label = thread
            .name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .or_else(|| {
                thread
                    .agent_nickname
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
            })
            .or_else(|| {
                thread
                    .agent_role
                    .as_deref()
                    .filter(|role| !role.trim().is_empty())
            })
            .unwrap_or_else(|| compact_id(&thread.id));
        format!("Subagent · {label}")
    }

    fn render(&mut self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        Clear.render(area, buffer);
        buffer.set_style(area, Style::default().bg(theme.bg_base));
        let Some(frame) = render_modal_frame_with_geometry(
            area,
            buffer,
            theme,
            &self.title(),
            "",
            ModalHeight::FullViewport,
        ) else {
            return;
        };
        let _ = render_surface_with_view(
            &mut self.child,
            &self.session,
            TranscriptView::ReadOnly,
            frame.content,
            buffer,
        );
        self.pointer
            .observe_frame(frame.popup, frame.close_button, Vec::new());
        render_modal_close_button(
            buffer,
            frame.close_button,
            theme,
            self.pointer.close_hovered(),
        );
    }

    fn observe_notification(&mut self, notification: &ServerNotification) {
        self.child.observe_subagent_notification(notification);
        let thread_id = self.session.thread.id.as_str();
        self.child.conversation_mut().apply(notification);
        match notification {
            ServerNotification::TurnStarted(params) if params.thread_id == thread_id => {
                self.session.active_turn_id = Some(params.turn.id.clone());
                self.child.set_activity(super::SurfaceActivity::Working);
            }
            ServerNotification::TurnCompleted(params) if params.thread_id == thread_id => {
                self.session.active_turn_id = None;
                self.child.set_activity(match params.turn.status {
                    TurnStatus::Interrupted => super::SurfaceActivity::Interrupted,
                    TurnStatus::InProgress => super::SurfaceActivity::Working,
                    TurnStatus::Completed | TurnStatus::Failed => super::SurfaceActivity::Ready,
                });
            }
            ServerNotification::ItemStarted(params)
                if params.thread_id == thread_id
                    && matches!(&params.item, ThreadItem::ContextCompaction { .. }) =>
            {
                self.child.set_activity(super::SurfaceActivity::Compacting);
            }
            ServerNotification::ItemCompleted(params)
                if params.thread_id == thread_id
                    && matches!(&params.item, ThreadItem::ContextCompaction { .. }) =>
            {
                self.child.set_activity(super::SurfaceActivity::Working);
            }
            ServerNotification::ThreadTokenUsageUpdated(params)
                if params.thread_id == thread_id =>
            {
                self.child.set_token_usage(params.token_usage.clone());
            }
            ServerNotification::ThreadNameUpdated(params) if params.thread_id == thread_id => {
                self.session.thread.name.clone_from(&params.thread_name);
            }
            ServerNotification::ThreadStatusChanged(params) if params.thread_id == thread_id => {
                self.session.thread.status.clone_from(&params.status);
            }
            _ => {}
        }
    }
}

impl SurfaceState {
    pub(crate) fn open_subagent_view(&mut self, thread: Thread, session: &SessionState) {
        self.block_viewer = None;
        self.file_viewer = None;
        self.pending_file_viewer_request = None;
        self.subagent_view = Some(Box::new(SubagentViewState::new(thread, session, self)));
    }

    pub(crate) fn open_subagent_view_on_active(
        &mut self,
        thread: Thread,
        root_session: &SessionState,
    ) {
        if let Some(view) = self.subagent_view.as_deref_mut() {
            view.child
                .open_subagent_view_on_active(thread, &view.session);
        } else {
            self.open_subagent_view(thread, root_session);
        }
    }

    pub(crate) fn close_subagent_view(&mut self) {
        self.subagent_view = None;
    }

    pub(crate) fn subagent_view_open(&self) -> bool {
        self.subagent_view.is_some()
    }

    pub(crate) fn subagent_surface_mut(&mut self) -> Option<&mut SurfaceState> {
        self.subagent_view
            .as_deref_mut()
            .map(|view| view.child.as_mut())
    }

    pub(crate) fn handle_subagent_frame_mouse(&mut self, mouse: MouseEvent) -> ModalPointerAction {
        self.subagent_view
            .as_deref_mut()
            .map(|view| view.pointer.handle_mouse(mouse))
            .unwrap_or(ModalPointerAction::Ignored)
    }

    pub(crate) fn selected_subagent_thread_id(&self) -> Option<String> {
        let entry_id = self.scrollback.selected_id()?;
        self.subagent_thread_id_for_entry(entry_id)
    }

    pub(crate) fn subagent_thread_id_for_entry(&self, entry_id: &str) -> Option<String> {
        let PresentationBlock::Subagent(subagent) = self.presentation_block(entry_id)? else {
            return None;
        };
        let mut thread_ids = subagent.thread_ids.clone();
        for agent in &subagent.agents {
            if !thread_ids.contains(&agent.thread_id) {
                thread_ids.push(agent.thread_id.clone());
            }
        }
        match thread_ids.as_slice() {
            [thread_id] => Some(thread_id.clone()),
            [] | [_, _, ..] => None,
        }
    }

    pub(crate) fn observe_subagent_notification(&mut self, notification: &ServerNotification) {
        if let Some(view) = self.subagent_view.as_deref_mut() {
            view.observe_notification(notification);
        }
    }

    pub(crate) fn set_notice_on_active_surface(&mut self, notice: impl Into<String>) {
        let notice = notice.into();
        if let Some(view) = self.subagent_view.as_deref_mut() {
            view.child.set_notice_on_active_surface(notice);
        } else {
            self.set_notice(notice);
        }
    }

    pub(super) fn render_subagent_overlay(
        &mut self,
        area: Rect,
        buffer: &mut Buffer,
        theme: AstralTheme,
    ) -> bool {
        let Some(view) = self.subagent_view.as_deref_mut() else {
            return false;
        };
        view.render(area, buffer, theme);
        true
    }
}

fn compact_id(thread_id: &str) -> &str {
    thread_id.get(..8).unwrap_or(thread_id)
}
