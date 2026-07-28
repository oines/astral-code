//! Astral permission-mode selection using the shared Grok-style modal chrome.

use codex_app_server_protocol::AskForApproval;
use codex_protocol::models::BUILT_IN_PERMISSION_PROFILE_DANGER_FULL_ACCESS;
use codex_protocol::models::BUILT_IN_PERMISSION_PROFILE_READ_ONLY;
use codex_protocol::models::BUILT_IN_PERMISSION_PROFILE_WORKSPACE;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::MouseEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::modal::ModalPointerAction;
use crate::modal::ModalPointerState;
use crate::modal::ModalRowHit;
use crate::view::AstralTheme;
use crate::view::ModalHeight;
use crate::view::modal_choice_style;
use crate::view::render_modal_close_button;
use crate::view::render_modal_frame_with_geometry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionSelection {
    ReadOnly,
    Workspace,
    FullAccess,
}

impl PermissionSelection {
    const ALL: [Self; 3] = [Self::ReadOnly, Self::Workspace, Self::FullAccess];

    pub(crate) fn profile_id(self) -> &'static str {
        match self {
            Self::ReadOnly => BUILT_IN_PERMISSION_PROFILE_READ_ONLY,
            Self::Workspace => BUILT_IN_PERMISSION_PROFILE_WORKSPACE,
            Self::FullAccess => BUILT_IN_PERMISSION_PROFILE_DANGER_FULL_ACCESS,
        }
    }

    pub(crate) fn approval_policy(self) -> AskForApproval {
        match self {
            Self::ReadOnly | Self::Workspace => AskForApproval::OnRequest,
            Self::FullAccess => AskForApproval::Never,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "Read only",
            Self::Workspace => "Workspace",
            Self::FullAccess => "Full access",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::ReadOnly => "Read files; ask before edits or network",
            Self::Workspace => "Edit workspace; ask for external access",
            Self::FullAccess => "Edit anywhere; use network without asking",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermissionStage {
    Select,
    ConfirmFullAccess,
}

#[derive(Debug)]
pub(crate) struct PermissionPickerState {
    selected: usize,
    current_profile: Option<String>,
    stage: PermissionStage,
    pointer: ModalPointerState,
}

impl PermissionPickerState {
    pub(crate) fn new(current_profile: Option<String>) -> Self {
        let selected = current_profile
            .as_deref()
            .and_then(|profile| {
                PermissionSelection::ALL
                    .iter()
                    .position(|selection| selection.profile_id() == profile)
            })
            .unwrap_or(1);
        Self {
            selected,
            current_profile,
            stage: PermissionStage::Select,
            pointer: ModalPointerState::default(),
        }
    }

    fn selection(&self) -> PermissionSelection {
        PermissionSelection::ALL[self.selected.min(PermissionSelection::ALL.len() - 1)]
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn move_down(&mut self) {
        self.selected = (self.selected + 1).min(PermissionSelection::ALL.len() - 1);
    }

    fn move_by(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(PermissionSelection::ALL.len() - 1);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermissionPickerInput {
    None,
    Redraw,
    Select(PermissionSelection),
    Cancel,
}

pub(crate) fn handle_key(
    state: &mut PermissionPickerState,
    key: KeyEvent,
) -> PermissionPickerInput {
    if key.kind == KeyEventKind::Release {
        return PermissionPickerInput::None;
    }
    if state.stage == PermissionStage::ConfirmFullAccess {
        return match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                PermissionPickerInput::Select(PermissionSelection::FullAccess)
            }
            KeyCode::Esc | KeyCode::Char('n') => {
                state.stage = PermissionStage::Select;
                PermissionPickerInput::Redraw
            }
            _ => PermissionPickerInput::None,
        };
    }
    match key.code {
        KeyCode::Esc => PermissionPickerInput::Cancel,
        KeyCode::Up | KeyCode::Char('k') => {
            state.move_up();
            PermissionPickerInput::Redraw
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.move_down();
            PermissionPickerInput::Redraw
        }
        KeyCode::Enter => activate_selection(state),
        _ => PermissionPickerInput::None,
    }
}

pub(crate) fn handle_mouse(
    state: &mut PermissionPickerState,
    mouse: MouseEvent,
) -> PermissionPickerInput {
    match state.pointer.handle_mouse(mouse) {
        ModalPointerAction::Ignored => PermissionPickerInput::None,
        ModalPointerAction::Redraw | ModalPointerAction::Hover(None) => {
            PermissionPickerInput::Redraw
        }
        ModalPointerAction::Close => PermissionPickerInput::Cancel,
        ModalPointerAction::Hover(Some(index)) => {
            if state.stage == PermissionStage::Select {
                state.selected = index.min(PermissionSelection::ALL.len() - 1);
            }
            PermissionPickerInput::Redraw
        }
        ModalPointerAction::Activate(index) => {
            if state.stage != PermissionStage::Select {
                return PermissionPickerInput::Redraw;
            }
            state.selected = index.min(PermissionSelection::ALL.len() - 1);
            activate_selection(state)
        }
        ModalPointerAction::Scroll(delta) => {
            if state.stage == PermissionStage::Select {
                state.move_by(delta);
            }
            PermissionPickerInput::Redraw
        }
    }
}

fn activate_selection(state: &mut PermissionPickerState) -> PermissionPickerInput {
    let selection = state.selection();
    if selection == PermissionSelection::FullAccess {
        state.stage = PermissionStage::ConfirmFullAccess;
        PermissionPickerInput::Redraw
    } else {
        PermissionPickerInput::Select(selection)
    }
}

pub(crate) fn render_picker(
    state: &mut PermissionPickerState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    if state.stage == PermissionStage::ConfirmFullAccess {
        render_confirmation(state, area, buffer, theme);
        return;
    }
    let Some(frame) = render_modal_frame_with_geometry(
        area,
        buffer,
        theme,
        "Update Astral permissions",
        "↑/↓ navigate · Enter select · Esc cancel",
        ModalHeight::MinimumContent(8),
    ) else {
        return;
    };
    render_modal_close_button(
        buffer,
        frame.close_button,
        theme,
        state.pointer.close_hovered(),
    );
    let content = frame.content;
    let mut row_hits = Vec::new();
    for (index, selection) in PermissionSelection::ALL.iter().enumerate() {
        let y = content.y + u16::try_from(index * 2).unwrap_or(u16::MAX);
        if y >= content.bottom() {
            break;
        }
        row_hits.push(ModalRowHit {
            id: index,
            area: Rect::new(
                content.x,
                y,
                content.width,
                content.bottom().saturating_sub(y).min(2),
            ),
        });
        let selected = index == state.selected;
        let current = state.current_profile.as_deref() == Some(selection.profile_id());
        let marker = if selected { "❯ " } else { "  " };
        let suffix = if current { " (current)" } else { "" };
        let row_style = modal_choice_style(theme, selected);
        buffer.set_style(Rect::new(content.x, y, content.width, 1), row_style);
        buffer.set_stringn(
            content.x,
            y,
            format!("{marker}{}{suffix}", selection.label()),
            usize::from(content.width),
            row_style,
        );
        if y + 1 < content.bottom() {
            buffer.set_stringn(
                content.x + 4,
                y + 1,
                selection.description(),
                usize::from(content.width.saturating_sub(4)),
                Style::default().fg(theme.gray).bg(theme.bg_base),
            );
        }
    }
    state
        .pointer
        .observe_frame(frame.popup, frame.close_button, row_hits);
}

fn render_confirmation(
    state: &mut PermissionPickerState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    let Some(frame) = render_modal_frame_with_geometry(
        area,
        buffer,
        theme,
        "Enable full access?",
        "Enter/Y enable · N/Esc back",
        ModalHeight::MinimumContent(6),
    ) else {
        return;
    };
    render_modal_close_button(
        buffer,
        frame.close_button,
        theme,
        state.pointer.close_hovered(),
    );
    state
        .pointer
        .observe_frame(frame.popup, frame.close_button, Vec::new());
    let content = frame.content;
    let lines = [
        "Astral will be able to edit files outside this workspace",
        "and access the network without asking for approval.",
        "",
        "Only enable this for work you trust.",
    ];
    for (index, line) in lines.iter().enumerate() {
        let y = content.y + u16::try_from(index).unwrap_or(u16::MAX);
        if y >= content.bottom() {
            break;
        }
        buffer.set_stringn(
            content.x,
            y,
            *line,
            usize::from(content.width),
            if index == 3 {
                Style::default().fg(theme.accent_error).bg(theme.bg_base)
            } else {
                Style::default().fg(theme.text_primary).bg(theme.bg_base)
            },
        );
    }
}

pub(crate) fn display_permission_mode(profile: Option<&str>) -> &'static str {
    match profile {
        Some(BUILT_IN_PERMISSION_PROFILE_READ_ONLY) => "read-only",
        Some(BUILT_IN_PERMISSION_PROFILE_WORKSPACE) => "workspace",
        Some(BUILT_IN_PERMISSION_PROFILE_DANGER_FULL_ACCESS) => "full-access",
        Some(_) => "custom",
        None => "default",
    }
}

#[cfg(test)]
#[path = "permission_picker_tests.rs"]
mod tests;
