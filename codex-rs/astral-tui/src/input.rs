use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::Thread;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;

use crate::PendingRequest;
use crate::PendingRequestResponse;
use crate::PromptSubmission;
use crate::RequestResolution;
use crate::SlashCommandId;
use crate::SlashInvocation;
use crate::SurfaceActivity;
use crate::SurfaceState;
use crate::ThreadPickerAction;
use crate::actions;
use crate::actions::ActionId;
use crate::actions::When;
use crate::permission_picker::PermissionSelection;
use crate::request_choice::RequestChoiceEvent;
use crate::request_choice::cancel_response;
use crate::request_choice::is_simple_request;
use crate::request_choice::response_for;
use crate::surface::ActiveOverlay;

mod block_viewer;
mod command_palette;
mod completion_popup;
mod content_viewer;
mod file_search_popup;
mod file_viewer;
mod history_popup;
mod mcp_form;
mod mention_popup;
mod models_manager;
mod mouse_scroll;
mod pickers;
mod plan_review;
mod prompt_mouse;
mod queue;
mod scrollback;
mod shortcut_help;
mod subagent;
mod terminal_support;
mod user_input;

pub(crate) use mouse_scroll::MouseScrollState;
pub(crate) use mouse_scroll::ScrollConfig;
pub(crate) use mouse_scroll::ScrollDirection;
pub(crate) use terminal_support::normalize_key;

#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    None,
    Redraw,
    Submit(PromptSubmission),
    SteerPrompt(PromptSubmission),
    SteerQueuedPrompt {
        id: u64,
    },
    Interrupt,
    Exit,
    ScrollUp,
    ScrollDown,
    CopyLastResponse,
    OpenExternalEditor,
    CopyText {
        text: String,
        notice: String,
    },
    OpenLink(crate::LinkTarget),
    OpenSubagent {
        thread_id: String,
    },
    Slash {
        invocation: SlashInvocation,
        submission: PromptSubmission,
    },
    ThreadPickerLoadNext,
    ThreadPickerSelect {
        action: ThreadPickerAction,
        thread: Box<Thread>,
    },
    SelectTheme(String),
    SelectPermission(PermissionSelection),
    Plan(crate::plan_review::PlanReviewAction),
    CycleMode,
    ToggleMultiline,
    RunShellCommand {
        command: String,
    },
    OpenShortcuts,
    DrainQueue,
    Resolve(RequestResolution),
    Notice(String),
}

pub fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    if key.kind == KeyEventKind::Release {
        return InputAction::None;
    }
    if state.consume_pending_action(&key) == Some(ActionId::NewSession) {
        if !matches!(
            state.activity(),
            SurfaceActivity::Ready | SurfaceActivity::Interrupted
        ) {
            return InputAction::Notice(
                "Starting a new session is unavailable while Astral is working".to_string(),
            );
        }
        state.record_slash(SlashCommandId::New);
        return InputAction::Slash {
            invocation: SlashInvocation {
                command: SlashCommandId::New,
                name: "new",
                args: String::new(),
            },
            submission: PromptSubmission::text_only(String::new()),
        };
    }
    if let Some(overlay) = state.active_overlay() {
        return handle_overlay_key(state, overlay, key);
    }
    if actions::matches(ActionId::CommandPalette, &key)
        || (state.scrollback_focused()
            && key.code == KeyCode::Char('?')
            && !key
                .modifiers
                .contains(KeyModifiers::CONTROL | KeyModifiers::ALT))
    {
        state.open_command_palette();
        return InputAction::Redraw;
    }
    if let Some(request) = state.pending_requests().front().cloned() {
        state.sync_request_states();
        if state.scrollback_focused() {
            return scrollback::handle_key(state, key);
        }
        return handle_request_key(state, request, key);
    }
    if state.queue_editing() {
        return handle_composer_key(state, key);
    }
    if state.plan_review().is_some() && !state.scrollback_focused() {
        return plan_review::handle_key(state, key);
    }
    if state.plan_review().is_none()
        && actions::lookup(&key, When::PromptFocused) == Some(ActionId::ToggleQueue)
    {
        return if state.toggle_queue_focus() {
            InputAction::Redraw
        } else {
            InputAction::Notice("No follow-ups queued".to_string())
        };
    }
    if state.queue_focused() {
        return queue::handle_key(state, key);
    }
    if actions::matches(ActionId::OpenSessions, &key) {
        if !matches!(
            state.activity(),
            SurfaceActivity::Ready | SurfaceActivity::Interrupted
        ) {
            return InputAction::Notice(
                "Session selection is unavailable while Astral is working".to_string(),
            );
        }
        state.record_slash(SlashCommandId::Resume);
        return InputAction::Slash {
            invocation: SlashInvocation {
                command: SlashCommandId::Resume,
                name: "resume",
                args: String::new(),
            },
            submission: PromptSubmission::text_only(String::new()),
        };
    }
    if actions::matches(ActionId::NewSession, &key) {
        if !matches!(
            state.activity(),
            SurfaceActivity::Ready | SurfaceActivity::Interrupted
        ) {
            return InputAction::Notice(
                "Starting a new session is unavailable while Astral is working".to_string(),
            );
        }
        state.arm_pending_action(ActionId::NewSession);
        return InputAction::Redraw;
    }
    if key.code == KeyCode::Esc && state.clear_scrollback_selection() {
        return InputAction::Redraw;
    }
    if state.scrollback_focused() {
        let registered = actions::lookup(&key, When::ScrollbackFocused).is_some();
        let action = scrollback::handle_key(state, key);
        if action == InputAction::None
            && !registered
            && matches!(key.code, KeyCode::Char(_))
            && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT)
        {
            state.focus_prompt();
            return handle_composer_key(state, key);
        }
        return action;
    }
    handle_composer_key(state, key)
}

pub fn handle_paste(state: &mut SurfaceState, text: &str) -> InputAction {
    if let Some(overlay) = state.active_overlay() {
        return handle_overlay_paste(state, overlay, text);
    }
    if state.paste_scrollback_search(text).is_some() {
        return InputAction::Redraw;
    }
    if state.history().open {
        return history_popup::handle_paste(state, text);
    }
    let user_input = state
        .pending_requests()
        .front()
        .and_then(|request| match request {
            PendingRequest::UserInput { params, .. } => Some(params.clone()),
            _ => None,
        });
    if let Some(params) = user_input {
        return if state.request_user_input_mut().handle_paste(&params, text) {
            InputAction::Redraw
        } else {
            InputAction::None
        };
    }
    let mcp_schema = state
        .pending_requests()
        .front()
        .and_then(|request| match request {
            PendingRequest::McpElicitation { params, .. } => match &params.request {
                McpServerElicitationRequest::Form {
                    requested_schema, ..
                } => Some(requested_schema.clone()),
                McpServerElicitationRequest::Url { .. } => None,
            },
            _ => None,
        });
    if let Some(schema) = mcp_schema {
        return if state.mcp_form_mut().handle_paste(&schema, text) {
            InputAction::Redraw
        } else {
            InputAction::None
        };
    }
    if state.plan_review().is_some() {
        return plan_review::handle_paste(state, text);
    }
    if state.shell_input_mode() {
        state.composer_state_mut().insert_text(text);
        return InputAction::Redraw;
    }
    if state.composer().is_empty()
        && let Some(command) = text.strip_prefix("! ")
        && state.enter_shell_input_mode()
    {
        state.composer_state_mut().insert_text(command);
        return InputAction::Redraw;
    }
    let notice = state.composer_state_mut().insert_paste_payload(text);
    state.refresh_composer_completions();
    notice.map_or(InputAction::Redraw, InputAction::Notice)
}

pub(crate) fn handle_mouse(state: &mut SurfaceState, mouse: MouseEvent) -> InputAction {
    if let Some(overlay) = state.active_overlay() {
        return handle_overlay_mouse(state, overlay, mouse);
    }
    let navigation_hover_changed = matches!(mouse.kind, MouseEventKind::Moved)
        && state.update_transcript_navigation_hover(mouse);
    let action = handle_main_mouse(state, mouse);
    if action == InputAction::None && navigation_hover_changed {
        InputAction::Redraw
    } else {
        action
    }
}

fn handle_main_mouse(state: &mut SurfaceState, mouse: MouseEvent) -> InputAction {
    if (state.composer_mouse_drag_active()
        && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)))
        || (state.composer_mouse_active()
            && matches!(
                mouse.kind,
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
            ))
    {
        return prompt_mouse::handle(state, mouse);
    }
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
        && let Some(changed) = state.handle_transcript_navigation_click(mouse)
    {
        return if changed {
            InputAction::Redraw
        } else {
            InputAction::None
        };
    }
    if state.pending_requests().front().is_some() && scrollback_owns_pointer(state, mouse) {
        return InputAction::None;
    }
    if let Some(request) = state.pending_requests().front().cloned()
        && is_simple_request(&request)
    {
        state.sync_request_states();
        let event = state.request_choice_mut().handle_mouse(mouse);
        if matches!(
            mouse.kind,
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left)
        ) && event != RequestChoiceEvent::None
        {
            state.focus_prompt();
        }
        return handle_request_choice_event(state, request, event);
    }
    if let Some(request) = state.pending_requests().front().cloned()
        && let PendingRequest::UserInput { params, .. } = &request
    {
        let params = params.clone();
        return user_input::handle_mouse(state, request, &params, mouse);
    }
    if let Some(request) = state.pending_requests().front().cloned()
        && let PendingRequest::McpElicitation { params, .. } = &request
        && let McpServerElicitationRequest::Form {
            requested_schema, ..
        } = &params.request
    {
        let schema = requested_schema.clone();
        return mcp_form::handle_mouse(state, request, &schema, mouse);
    }
    if state.plan_review().is_none()
        && (state.queue_contains(mouse)
            || (matches!(mouse.kind, MouseEventKind::Moved) && state.queue_hovered().is_some()))
    {
        return queue::handle_mouse(state, mouse);
    }
    if scrollback_owns_pointer(state, mouse) {
        return InputAction::None;
    }
    if state.plan_review().is_some() {
        let action = plan_review::handle_mouse(state, mouse);
        if action != InputAction::None {
            return action;
        }
        if state.prompt_contains(mouse)
            && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
        {
            return prompt_mouse::handle(state, mouse);
        }
        return InputAction::None;
    }
    if state.history().open
        || state.file_search().open
        || state.slash().open
        || state.mentions().open
    {
        return completion_popup::handle_mouse(state, mouse);
    }
    if state.prompt_contains(mouse) && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
    {
        return prompt_mouse::handle(state, mouse);
    }
    InputAction::None
}

fn handle_overlay_key(
    state: &mut SurfaceState,
    overlay: ActiveOverlay,
    key: KeyEvent,
) -> InputAction {
    match overlay {
        ActiveOverlay::Subagent => subagent::handle_key(state, key),
        ActiveOverlay::FileViewer => file_viewer::handle_key(state, key),
        ActiveOverlay::BlockViewer => block_viewer::handle_key(state, key),
        ActiveOverlay::ThemePicker => pickers::handle_theme_picker_key(state, key),
        ActiveOverlay::PermissionPicker => pickers::handle_permission_picker_key(state, key),
        ActiveOverlay::ThreadPicker => pickers::handle_thread_picker_key(state, key),
        ActiveOverlay::ModelPicker => pickers::handle_model_picker_key(state, key),
        ActiveOverlay::ModelsManager => models_manager::handle_key(state, key),
        ActiveOverlay::CommandPalette => command_palette::handle_key(state, key),
        ActiveOverlay::ShortcutHelp => shortcut_help::handle_key(state, key),
        ActiveOverlay::InfoModal => pickers::handle_info_modal_key(state, key),
    }
}

fn handle_overlay_paste(
    state: &mut SurfaceState,
    overlay: ActiveOverlay,
    text: &str,
) -> InputAction {
    match overlay {
        ActiveOverlay::Subagent => subagent::handle_paste(state, text),
        ActiveOverlay::FileViewer => file_viewer::handle_paste(state, text),
        ActiveOverlay::BlockViewer => block_viewer::handle_paste(state, text),
        ActiveOverlay::ThreadPicker => {
            let Some(picker) = state.thread_picker_mut() else {
                return InputAction::None;
            };
            picker.paste(text);
            InputAction::Redraw
        }
        ActiveOverlay::ModelPicker => pickers::handle_model_picker_paste(state, text),
        ActiveOverlay::ModelsManager => models_manager::handle_paste(state, text),
        ActiveOverlay::CommandPalette => command_palette::handle_paste(state, text),
        ActiveOverlay::ShortcutHelp => shortcut_help::handle_paste(state, text),
        ActiveOverlay::ThemePicker | ActiveOverlay::PermissionPicker | ActiveOverlay::InfoModal => {
            InputAction::None
        }
    }
}

fn handle_overlay_mouse(
    state: &mut SurfaceState,
    overlay: ActiveOverlay,
    mouse: MouseEvent,
) -> InputAction {
    match overlay {
        ActiveOverlay::Subagent => subagent::handle_mouse(state, mouse),
        ActiveOverlay::FileViewer => file_viewer::handle_mouse(state, mouse),
        ActiveOverlay::BlockViewer => block_viewer::handle_mouse(state, mouse),
        ActiveOverlay::ThemePicker => pickers::handle_theme_picker_mouse(state, mouse),
        ActiveOverlay::PermissionPicker => pickers::handle_permission_picker_mouse(state, mouse),
        ActiveOverlay::ThreadPicker => pickers::handle_thread_picker_mouse(state, mouse),
        ActiveOverlay::ModelPicker => pickers::handle_model_picker_mouse(state, mouse),
        ActiveOverlay::ModelsManager => models_manager::handle_mouse(state, mouse),
        ActiveOverlay::CommandPalette => command_palette::handle_mouse(state, mouse),
        ActiveOverlay::ShortcutHelp => shortcut_help::handle_mouse(state, mouse),
        ActiveOverlay::InfoModal => pickers::handle_info_modal_mouse(state, mouse),
    }
}

fn scrollback_owns_pointer(state: &mut SurfaceState, mouse: MouseEvent) -> bool {
    if !state.scrollback_contains(mouse) {
        return false;
    }
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        state.focus_scrollback();
    }
    true
}

fn handle_composer_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    if state.queue_editing() {
        if key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c')
                && key.modifiers == KeyModifiers::CONTROL
                && state.composer().is_empty())
        {
            state.cancel_queue_edit();
            return InputAction::DrainQueue;
        }
        if terminal_support::is_modified_enter(&key) {
            state.composer_state_mut().insert_char('\n');
            state.refresh_composer_completions();
            return InputAction::Redraw;
        }
        if key.code == KeyCode::Enter && key.modifiers == KeyModifiers::NONE {
            if state.composer().trim().is_empty() {
                return InputAction::Notice("Queued follow-up cannot be empty".to_string());
            }
            state.save_queue_edit();
            return InputAction::DrainQueue;
        }
    }
    if state.history().open {
        return history_popup::handle_key(state, key);
    }
    if state.file_search().open
        && let Some(action) = file_search_popup::handle_key(state, key)
    {
        return action;
    }
    if state.mentions().open
        && let Some(action) = mention_popup::handle_key(state, key)
    {
        return action;
    }
    if state.shell_input_mode() && state.composer().is_empty() && is_shell_mode_exit_key(key) {
        state.exit_shell_input_mode();
        return InputAction::Redraw;
    }
    if key.code == KeyCode::Char('l')
        && key.modifiers == KeyModifiers::CONTROL
        && state.open_file_reference_viewer(false)
    {
        return InputAction::Redraw;
    }
    if key.code == KeyCode::Char(':')
        && key.modifiers == KeyModifiers::NONE
        && state.open_file_reference_viewer(true)
    {
        return InputAction::Redraw;
    }
    let modified_enter = terminal_support::is_modified_enter(&key);
    let shell_mode_key = key.code == KeyCode::Char('!')
        && !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
    let mut prompt_action = if state.multiline_mode() && modified_enter {
        Some(ActionId::SendPrompt)
    } else if shell_mode_key {
        Some(ActionId::ShellMode)
    } else {
        actions::lookup(&key, When::PromptFocused)
    };
    if !state.multiline_mode() && modified_enter && prompt_action == Some(ActionId::SendPrompt) {
        prompt_action = None;
    }
    if prompt_action == Some(ActionId::ShellMode) {
        if state.enter_shell_input_mode() {
            return InputAction::Redraw;
        }
        prompt_action = None;
    }
    if key.code == KeyCode::Esc && state.restore_palette_draft() {
        return InputAction::Redraw;
    }
    if prompt_action == Some(ActionId::InterjectPrompt) {
        if state.shell_input_mode() {
            return state
                .take_shell_command()
                .map_or(InputAction::None, |command| InputAction::RunShellCommand {
                    command,
                });
        }
        if state.slash().active {
            return InputAction::Notice("Run slash commands with Enter".to_string());
        }
        if !matches!(state.activity(), SurfaceActivity::Working) {
            return InputAction::Notice("No active turn to steer".to_string());
        }
        if state.composer().trim().is_empty() {
            return state.next_follow_up_id().map_or(InputAction::None, |id| {
                InputAction::SteerQueuedPrompt { id }
            });
        }
        return InputAction::SteerPrompt(state.take_submission());
    }
    if prompt_action == Some(ActionId::CycleMode) {
        return InputAction::CycleMode;
    }
    if prompt_action == Some(ActionId::ToggleMultiline) {
        return InputAction::ToggleMultiline;
    }
    if prompt_action == Some(ActionId::ShortcutsHelp) {
        return InputAction::OpenShortcuts;
    }
    if state.slash().open {
        match key.code {
            KeyCode::Esc => {
                state.close_slash();
                return InputAction::Redraw;
            }
            KeyCode::Up => {
                state.move_slash_selection(-1);
                return InputAction::Redraw;
            }
            KeyCode::Down => {
                state.move_slash_selection(1);
                return InputAction::Redraw;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                state.accept_slash_selection();
                return InputAction::Redraw;
            }
            KeyCode::Enter if !state.slash().recognized => {
                state.accept_slash_selection();
                return InputAction::Redraw;
            }
            _ => {}
        }
    }
    if prompt_action == Some(ActionId::FocusScrollback) && state.focus_scrollback() {
        return InputAction::Redraw;
    }
    if key.code == KeyCode::Up
        && key.modifiers == KeyModifiers::NONE
        && !state.shell_input_mode()
        && state.composer().is_empty()
    {
        state.open_history_browse();
        return InputAction::Redraw;
    }
    if prompt_action == Some(ActionId::SendPrompt)
        && !state.slash().active
        && state.open_composer_image_at_cursor()
    {
        return InputAction::Redraw;
    }
    if prompt_action == Some(ActionId::SendPrompt)
        && !state.slash().active
        && state.composer_state_mut().expand_paste_at_cursor()
    {
        state.refresh_composer_completions();
        return InputAction::Redraw;
    }
    match prompt_action {
        Some(ActionId::PromptCancel) => {
            if state.restore_palette_draft() {
                InputAction::Redraw
            } else if matches!(state.activity(), SurfaceActivity::Working) {
                InputAction::Interrupt
            } else if state.composer().is_empty() {
                InputAction::Exit
            } else {
                state.composer_state_mut().clear();
                state.refresh_composer_completions();
                InputAction::Redraw
            }
        }
        Some(ActionId::ExitEmptyPrompt) if state.composer().is_empty() => InputAction::Exit,
        Some(ActionId::CopyLastResponse) => InputAction::CopyLastResponse,
        Some(ActionId::OpenExternalEditor) => {
            if state.slash().open || state.mentions().open || state.file_search().open {
                InputAction::None
            } else if state.composer_has_structured_elements() {
                InputAction::Notice(
                    "External editing is unavailable while the draft has structured prompt items"
                        .to_string(),
                )
            } else {
                InputAction::OpenExternalEditor
            }
        }
        Some(ActionId::PageUp) => InputAction::ScrollUp,
        Some(ActionId::PageDown) => InputAction::ScrollDown,
        Some(ActionId::SendPrompt) => {
            if state.shell_input_mode() {
                return state
                    .take_shell_command()
                    .map_or(InputAction::None, |command| InputAction::RunShellCommand {
                        command,
                    });
            }
            if state.slash().active {
                return match state.slash_invocation() {
                    Ok(Some(invocation)) => {
                        let submission = state.take_submission();
                        state.restore_palette_draft();
                        state.record_slash(invocation.command);
                        InputAction::Slash {
                            invocation,
                            submission,
                        }
                    }
                    Ok(None) => InputAction::Notice("Choose a slash command".to_string()),
                    Err(error) => InputAction::Notice(error.to_string()),
                };
            }
            if state.multiline_mode()
                && key.code == KeyCode::Enter
                && key.modifiers == KeyModifiers::NONE
            {
                if state.composer().trim().is_empty()
                    && matches!(state.activity(), SurfaceActivity::Working)
                    && let Some(id) = state.next_follow_up_id()
                {
                    return InputAction::SteerQueuedPrompt { id };
                }
                state.composer_state_mut().insert_char('\n');
                state.refresh_composer_completions();
                return InputAction::Redraw;
            }
            if state.palette_draft_pending() {
                state.discard_palette_draft();
            }
            if state.composer().trim().is_empty()
                && matches!(state.activity(), SurfaceActivity::Working)
                && let Some(id) = state.next_follow_up_id()
            {
                return InputAction::SteerQueuedPrompt { id };
            }
            if state.composer_state_mut().apply_backslash_continuation() {
                state.refresh_composer_completions();
                return InputAction::Redraw;
            }
            let submission = state.take_submission();
            if submission.text().trim().is_empty() {
                InputAction::None
            } else {
                InputAction::Submit(submission)
            }
        }
        Some(ActionId::ExitEmptyPrompt) | None if modified_enter => {
            state.composer_state_mut().insert_char('\n');
            state.refresh_composer_completions();
            InputAction::Redraw
        }
        Some(ActionId::ExitEmptyPrompt) | None if state.composer_state_mut().edit_key(key) => {
            state.refresh_composer_completions();
            InputAction::Redraw
        }
        Some(
            ActionId::CycleMode
            | ActionId::ToggleMultiline
            | ActionId::ModelPicker
            | ActionId::OpenSessions
            | ActionId::NewSession
            | ActionId::ShellMode
            | ActionId::CommandPalette
            | ActionId::ShortcutsHelp
            | ActionId::ToggleQueue
            | ActionId::InterjectPrompt
            | ActionId::FocusScrollback
            | ActionId::OpenTranscriptSearch
            | ActionId::FocusPrompt
            | ActionId::PreviousTurn
            | ActionId::NextTurn
            | ActionId::NextResponse
            | ActionId::PreviousResponse
            | ActionId::GoToTop
            | ActionId::GoToBottom
            | ActionId::ScrollLineUp
            | ActionId::ScrollLineDown
            | ActionId::HalfPageUp
            | ActionId::HalfPageDown
            | ActionId::SelectNext
            | ActionId::SelectPrevious
            | ActionId::CollapseEntry
            | ActionId::ExpandEntry
            | ActionId::ToggleEntry
            | ActionId::ToggleAllEntries
            | ActionId::ToggleAllReasoning
            | ActionId::ToggleRawMarkdown
            | ActionId::CopyBlockContent
            | ActionId::CopyBlockMetadata
            | ActionId::NextLink
            | ActionId::PreviousLink
            | ActionId::OpenEntry
            | ActionId::ScrollbackCancel,
        )
        | Some(ActionId::ExitEmptyPrompt)
        | None => InputAction::None,
    }
}

fn is_shell_mode_exit_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Esc
        || key.code == KeyCode::Backspace
        || matches!(
            (key.code, key.modifiers),
            (KeyCode::Char('c' | 'u' | 'w'), KeyModifiers::CONTROL)
        )
}

fn handle_request_key(
    state: &mut SurfaceState,
    request: PendingRequest,
    key: KeyEvent,
) -> InputAction {
    let response = match request.clone() {
        PendingRequest::CommandExecution { .. }
        | PendingRequest::FileChange { .. }
        | PendingRequest::Permissions { .. } => {
            let event = state.request_choice_mut().handle_key(key);
            return handle_request_choice_event(state, request, event);
        }
        PendingRequest::UserInput { params, .. } => {
            return user_input::handle_key(state, request, &params, key);
        }
        PendingRequest::McpElicitation { params, .. } => match &params.request {
            McpServerElicitationRequest::Form {
                requested_schema, ..
            } => return mcp_form::handle_key(state, request, requested_schema, key),
            McpServerElicitationRequest::Url { .. } => {
                let event = state.request_choice_mut().handle_key(key);
                return handle_request_choice_event(state, request, event);
            }
        },
        PendingRequest::DynamicTool { .. } | PendingRequest::Attestation { .. } => None,
        PendingRequest::LegacyApplyPatch { .. } | PendingRequest::LegacyExecCommand { .. } => {
            Some(PendingRequestResponse::Reject {
                code: -32601,
                message: "Astral TUI accepts app-server v2 requests only".to_string(),
            })
        }
    };

    response.map_or(InputAction::None, |response| {
        resolve_request(state, &request, response)
    })
}

fn handle_request_choice_event(
    state: &mut SurfaceState,
    request: PendingRequest,
    event: RequestChoiceEvent,
) -> InputAction {
    match event {
        RequestChoiceEvent::None => InputAction::None,
        RequestChoiceEvent::Redraw => InputAction::Redraw,
        RequestChoiceEvent::FocusScrollback => {
            if state.focus_scrollback() {
                InputAction::Redraw
            } else {
                InputAction::None
            }
        }
        RequestChoiceEvent::OpenUrl(url) => InputAction::OpenLink(crate::LinkTarget::Url(url)),
        RequestChoiceEvent::Notice(message) => InputAction::Notice(message),
        RequestChoiceEvent::Activate(choice) => response_for(&request, choice)
            .map_or(InputAction::None, |response| {
                resolve_request(state, &request, response)
            }),
        RequestChoiceEvent::Cancel => cancel_response(&request)
            .map_or(InputAction::None, |response| {
                resolve_request(state, &request, response)
            }),
    }
}

fn resolve_request(
    state: &SurfaceState,
    request: &PendingRequest,
    response: PendingRequestResponse,
) -> InputAction {
    let request_id = request.request_id().clone();
    match state
        .pending_requests()
        .prepare_resolution(&request_id, response)
    {
        Ok(resolution) => InputAction::Resolve(resolution),
        Err(error) => InputAction::Notice(error.to_string()),
    }
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "input_editor_tests.rs"]
mod editor_tests;
