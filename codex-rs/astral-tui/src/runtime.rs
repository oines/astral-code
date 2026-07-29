use std::io;
use std::io::Stdout;
use std::time::Duration;
use std::time::Instant;

use astral_terminal_inline::Terminal;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::DynamicToolCallResponse;
use codex_app_server_protocol::McpServerStatusDetail;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::config_types::ModeKind;
use crossterm::event::Event;
use ratatui::TerminalOptions;
use ratatui::Viewport;
use ratatui::backend::CrosstermBackend;
use tokio::task::JoinSet;

use crate::AstralSession;
use crate::ClientToolError;
use crate::ClientToolRegistry;
use crate::InputAction;
use crate::PendingRequest;
use crate::PendingRequestResponse;
use crate::PromptSubmission;
use crate::SessionError;
use crate::SlashCommandId;
use crate::SurfaceActivity;
use crate::SurfaceState;
use crate::ThreadPickerAction;
use crate::TranscriptView;
use crate::clipboard::copy_to_clipboard;
use crate::committed_height;
use crate::ecosystem::apps_panel;
use crate::ecosystem::hooks_panel;
use crate::ecosystem::mcp_panel;
use crate::ecosystem::plugins_panel;
use crate::ecosystem::skills_panel;
use crate::handle_key;
use crate::handle_paste;
use crate::input::MouseScrollState;
use crate::input::ScrollConfig;
use crate::input::ScrollDirection;
use crate::input::handle_mouse;
use crate::modal::ModalRow;
use crate::modal::ModalState;
use crate::permission_picker::PermissionPickerState;
use crate::render_surface;
use crate::render_surface_with_view;
use crate::session::ThreadSwitchOutcome;
use crate::shortcuts::shortcuts_modal;
use crate::surface::paint_committed_with_theme;
use crate::terminal_guard::TerminalGuard;
use crate::thread_picker::PickerState;
use crate::view::AstralThemeId;
use crate::view::ColorLevel;

mod input_reader;
mod mentions;
mod plan;

use self::input_reader::TerminalEventReader;

type AstralTerminal = Terminal<CrosstermBackend<Stdout>>;

#[derive(Clone)]
pub struct RunOptions {
    pub viewport: RunViewport,
    pub viewport_rows: u16,
    pub client_tools: ClientToolRegistry,
    pub initial_theme: Option<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            viewport: RunViewport::Fullscreen,
            viewport_rows: 12,
            client_tools: ClientToolRegistry::default(),
            initial_theme: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunViewport {
    Fullscreen,
    Inline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunExitReason {
    UserRequested,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunExit {
    pub thread_id: String,
    pub thread_name: Option<String>,
    pub token_usage: Option<ThreadTokenUsage>,
    pub theme_selection: Option<String>,
    pub reason: RunExitReason,
}

#[derive(Debug)]
pub enum RunError {
    NoThread,
    Terminal(io::Error),
    Session(SessionError),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoThread => f.write_str("Astral TUI requires an active thread"),
            Self::Terminal(error) => write!(f, "terminal error: {error}"),
            Self::Session(error) => write!(f, "session error: {error}"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Terminal(error) => Some(error),
            Self::Session(error) => Some(error),
            Self::NoThread => None,
        }
    }
}

impl From<io::Error> for RunError {
    fn from(value: io::Error) -> Self {
        Self::Terminal(value)
    }
}

impl From<SessionError> for RunError {
    fn from(value: SessionError) -> Self {
        Self::Session(value)
    }
}

pub async fn run(mut session: AstralSession, options: RunOptions) -> Result<RunExit, RunError> {
    let initial_state = session.state().cloned().ok_or(RunError::NoThread)?;
    let mut surface = SurfaceState::from_session(&initial_state);
    if let Some(theme) = configured_theme(options.initial_theme.as_deref()) {
        surface.set_theme(theme);
    }
    surface.set_color_level(ColorLevel::detect());
    match session.list_models().await {
        Ok(models) => surface.set_model_catalog(
            models,
            initial_state.model.clone(),
            initial_state.model_provider.clone(),
        ),
        Err(error) => surface.set_notice(format!("Could not load model catalog: {error}")),
    }
    mentions::refresh_catalog(&mut session, &mut surface).await;
    let mut guard = match options.viewport {
        RunViewport::Fullscreen => TerminalGuard::enter_alternate()?,
        RunViewport::Inline => TerminalGuard::enter_inline()?,
    };
    let viewport = match options.viewport {
        RunViewport::Fullscreen => Viewport::Fullscreen,
        RunViewport::Inline => Viewport::Inline(desired_viewport_rows(options.viewport_rows)?),
    };
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = AstralTerminal::with_options(backend, TerminalOptions { viewport })?;
    terminal.hide_cursor()?;

    let mut theme_selection = None;
    let result = run_loop(
        &mut terminal,
        &mut session,
        &mut surface,
        &mut theme_selection,
        options,
    )
    .await;
    let _ = terminal.show_cursor();
    drop(terminal);
    guard.restore();

    let reason = match result {
        Ok(reason) => reason,
        Err(error) => {
            let _ = session.shutdown().await;
            return Err(error);
        }
    };
    let thread_name = session.state().and_then(|state| state.thread.name.clone());
    let thread_id = session
        .state()
        .map(|state| state.thread.id.clone())
        .ok_or(RunError::NoThread)?;
    let token_usage = surface.token_usage().cloned();
    session.shutdown().await?;
    Ok(RunExit {
        thread_id,
        thread_name,
        token_usage,
        theme_selection,
        reason,
    })
}

async fn run_loop(
    terminal: &mut AstralTerminal,
    session: &mut AstralSession,
    surface: &mut SurfaceState,
    theme_selection: &mut Option<String>,
    options: RunOptions,
) -> Result<RunExitReason, RunError> {
    let mut input = TerminalEventReader::start()?;
    let mut client_tool_tasks = JoinSet::new();
    let mut _clipboard_lease = None;
    let mut mouse_scroll = MouseScrollState::default();
    let mut needs_draw = true;

    loop {
        needs_draw |= surface.poll_scrollback_search();
        if needs_draw {
            draw(terminal, session, surface, &options)?;
            needs_draw = false;
        }
        let selection_expiry = surface
            .scrollback_selection_expiry()
            .map(tokio::time::Instant::from_std);
        let search_pending = surface.scrollback_search_pending();
        let scroll_deadline = mouse_scroll.clock_deadline(Instant::now());

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(16)), if search_pending => {
                needs_draw |= surface.poll_scrollback_search();
            }
            _ = async {
                if let Some(delay) = scroll_deadline {
                    tokio::time::sleep(delay).await;
                }
            }, if scroll_deadline.is_some() => {
                needs_draw |= apply_scroll_lines(surface, mouse_scroll.on_tick());
            }
            _ = async {
                if let Some(expiry) = selection_expiry {
                    tokio::time::sleep_until(expiry).await;
                }
            }, if selection_expiry.is_some() => {
                needs_draw |= surface.expire_scrollback_selection();
            }
            terminal_event = input.recv() => {
                let Some(terminal_event) = terminal_event else {
                    surface.set_activity(SurfaceActivity::Disconnected(
                        "terminal input closed".to_string(),
                    ));
                    draw(terminal, session, surface, &options)?;
                    reject_pending(session, surface).await;
                    return Ok(RunExitReason::Disconnected);
                };
                match terminal_event? {
                    Event::Key(key) => {
                        mouse_scroll.cancel();
                        let action = match handle_key(surface, key) {
                            InputAction::ScrollUp => {
                                if options.viewport == RunViewport::Fullscreen {
                                    surface.page_up();
                                } else {
                                    surface.set_notice(
                                        "Use the terminal's native scrollback in inline mode",
                                    );
                                }
                                InputAction::None
                            }
                            InputAction::ScrollDown => {
                                if options.viewport == RunViewport::Fullscreen {
                                    surface.page_down();
                                }
                                InputAction::None
                            }
                            InputAction::CopyLastResponse => {
                                let response = surface.last_agent_response().map(str::to_string);
                                match response {
                                    Some(response) => match copy_to_clipboard(&response) {
                                        Ok(lease) => {
                                            _clipboard_lease = Some(lease);
                                            surface.set_notice("Copied last agent response");
                                        }
                                        Err(error) => surface.set_notice(error),
                                    },
                                    None => surface.set_notice("No agent response to copy"),
                                }
                                InputAction::None
                            }
                            InputAction::CopyText { text, notice } => {
                                match copy_to_clipboard(&text) {
                                    Ok(lease) => {
                                        _clipboard_lease = Some(lease);
                                        surface.set_notice(notice);
                                    }
                                    Err(error) => surface.set_notice(error),
                                }
                                InputAction::None
                            }
                            action => action,
                        };
                        if let Some(reason) =
                            apply_input_action(session, surface, theme_selection, action).await?
                        {
                            reject_pending(session, surface).await;
                            return Ok(reason);
                        }
                        needs_draw = true;
                    }
                    Event::Paste(text) => {
                        mouse_scroll.cancel();
                        let _ = handle_paste(surface, &text);
                        needs_draw = true;
                    }
                    Event::Resize(_, _) => {
                        mouse_scroll.cancel();
                        needs_draw = true;
                    }
                    Event::Mouse(mouse) => {
                        if options.viewport == RunViewport::Fullscreen {
                            let direction = ScrollDirection::from_mouse_event(mouse);
                            let action = handle_mouse(surface, mouse);
                            match action {
                                InputAction::None
                                    if direction.is_some()
                                        && surface.scrollback_contains(mouse) =>
                                {
                                    let config =
                                        ScrollConfig::detected(surface.scrollback_rows());
                                    let lines = mouse_scroll.on_scroll_event(
                                        direction.expect("checked above"),
                                        config,
                                    );
                                    needs_draw |= apply_scroll_lines(surface, lines);
                                }
                                InputAction::None => {
                                    mouse_scroll.cancel();
                                    if surface.scrollback_contains(mouse) {
                                        if let Some(selection) =
                                            surface.handle_scrollback_mouse(mouse)
                                        {
                                            match copy_to_clipboard(&selection) {
                                                Ok(lease) => {
                                                    _clipboard_lease = Some(lease);
                                                    surface.set_notice("Copied selection");
                                                }
                                                Err(error) => surface.set_notice(error),
                                            }
                                        }
                                        needs_draw = true;
                                    }
                                }
                                InputAction::CopyText { text, notice } => {
                                    mouse_scroll.cancel();
                                    match copy_to_clipboard(&text) {
                                        Ok(lease) => {
                                            _clipboard_lease = Some(lease);
                                            surface.set_notice(notice);
                                        }
                                        Err(error) => surface.set_notice(error),
                                    }
                                    needs_draw = true;
                                }
                                action => {
                                    mouse_scroll.cancel();
                                    if let Some(reason) = apply_input_action(
                                        session,
                                        surface,
                                        theme_selection,
                                        action,
                                    )
                                    .await?
                                    {
                                        reject_pending(session, surface).await;
                                        return Ok(reason);
                                    }
                                    needs_draw = true;
                                }
                            }
                        }
                    }
                    Event::FocusGained | Event::FocusLost => {
                        mouse_scroll.cancel();
                    }
                }
            }
            app_event = session.next_event() => {
                let Some(app_event) = app_event else {
                    surface.set_activity(SurfaceActivity::Disconnected(
                        "app-server event stream closed".to_string(),
                    ));
                    draw(terminal, session, surface, &options)?;
                    reject_pending(session, surface).await;
                    return Ok(RunExitReason::Disconnected);
                };
                handle_app_event(
                    session,
                    surface,
                    &options.client_tools,
                    &mut client_tool_tasks,
                    app_event,
                )
                .await?;
                needs_draw = true;
            }
            completion = client_tool_tasks.join_next(), if !client_tool_tasks.is_empty() => {
                if let Some(completion) = completion {
                    let completion = completion.map_err(|error| {
                        RunError::Terminal(io::Error::other(format!(
                            "client tool task failed: {error}"
                        )))
                    })?;
                    resolve_client_tool(session, surface, completion).await?;
                    needs_draw = true;
                }
            }
        }
    }
}

fn apply_scroll_lines(surface: &mut SurfaceState, lines: i32) -> bool {
    if lines < 0 {
        surface.scroll_up(lines.unsigned_abs() as usize);
        true
    } else if lines > 0 {
        surface.scroll_down(lines.unsigned_abs() as usize);
        true
    } else {
        false
    }
}

fn draw(
    terminal: &mut AstralTerminal,
    session: &AstralSession,
    surface: &mut SurfaceState,
    options: &RunOptions,
) -> Result<(), RunError> {
    terminal.autoresize()?;
    if options.viewport == RunViewport::Inline {
        let terminal_rows = terminal.size()?.height;
        let viewport_rows = viewport_rows(options.viewport_rows, terminal_rows);
        if terminal.viewport_area().height != viewport_rows {
            terminal.set_viewport_height(viewport_rows)?;
        }

        let width = terminal.viewport_area().width;
        let theme = surface.theme();
        for block in surface.drain_committable() {
            let height = committed_height(&block, width);
            if height > 0 {
                terminal.insert_before(height, move |buffer| {
                    paint_committed_with_theme(&block, buffer, theme);
                })?;
            }
        }
    }

    let session_state = session.state().ok_or(RunError::NoThread)?;
    terminal.draw(|frame| {
        let position = match options.viewport {
            RunViewport::Fullscreen => render_surface_with_view(
                surface,
                session_state,
                TranscriptView::Full,
                frame.area(),
                frame.buffer_mut(),
            ),
            RunViewport::Inline => {
                render_surface(surface, session_state, frame.area(), frame.buffer_mut())
            }
        };
        if let Some(position) = position {
            frame.set_cursor_position(position);
        }
    })?;
    terminal.show_cursor()?;
    Ok(())
}

async fn apply_input_action(
    session: &mut AstralSession,
    surface: &mut SurfaceState,
    theme_selection: &mut Option<String>,
    action: InputAction,
) -> Result<Option<RunExitReason>, RunError> {
    match action {
        InputAction::None | InputAction::Redraw => {}
        InputAction::Submit(submission) => {
            start_submission(session, surface, submission).await;
        }
        InputAction::Interrupt => match session.interrupt().await {
            Ok(()) => surface.set_activity(SurfaceActivity::Interrupted),
            Err(error) => surface.set_notice(error.to_string()),
        },
        InputAction::Exit => return Ok(Some(RunExitReason::UserRequested)),
        InputAction::ScrollUp
        | InputAction::ScrollDown
        | InputAction::CopyLastResponse
        | InputAction::CopyText { .. } => {}
        InputAction::SelectTheme(name) => {
            *theme_selection = Some(name.clone());
            surface.set_notice(format!("Switched to {name}"));
        }
        InputAction::ThreadPickerLoadNext => {
            let cursor = surface
                .thread_picker()
                .and_then(|picker| picker.next_cursor())
                .map(str::to_string);
            if let Some(cursor) = cursor {
                match session.list_threads(Some(cursor)).await {
                    Ok(page) => {
                        if let Some(picker) = surface.thread_picker_mut() {
                            picker.append(page);
                            picker.move_down();
                        }
                    }
                    Err(error) => {
                        if let Some(picker) = surface.thread_picker_mut() {
                            picker.set_notice(error.to_string());
                        }
                    }
                }
            }
        }
        InputAction::ThreadPickerSelect { action, thread } => {
            let result = match action {
                ThreadPickerAction::Resume => session.resume_thread(thread.id).await,
                ThreadPickerAction::Fork => session.fork_thread(thread.id).await,
            };
            match result {
                Ok(outcome) => reset_surface_after_switch(session, surface, outcome).await,
                Err(error) => surface.set_notice(error.to_string()),
            }
        }
        InputAction::SelectPermission(selection) => {
            let label = selection.label();
            match session.update_permissions(selection).await {
                Ok(()) => surface.set_notice(format!("Switching permissions to {label}")),
                Err(error) => surface.set_notice(error.to_string()),
            }
        }
        InputAction::Plan(action) => plan::apply_action(session, surface, action).await,
        InputAction::CycleMode => {
            let mode = session
                .state()
                .map(|state| {
                    if state.collaboration_mode.mode == ModeKind::Plan {
                        ModeKind::Default
                    } else {
                        ModeKind::Plan
                    }
                })
                .unwrap_or(ModeKind::Default);
            set_collaboration_mode(session, surface, mode).await;
        }
        InputAction::OpenShortcuts => surface.open_modal(shortcuts_modal()),
        InputAction::Slash {
            invocation,
            submission,
        } => match invocation.command {
            SlashCommandId::Exit | SlashCommandId::Quit => {
                return Ok(Some(RunExitReason::UserRequested));
            }
            SlashCommandId::Copy => {
                if let Some(response) = surface.last_agent_response().map(str::to_string) {
                    match copy_to_clipboard(&response) {
                        Ok(_) => surface.set_notice("Copied last agent response"),
                        Err(error) => surface.set_notice(error),
                    }
                } else {
                    surface.set_notice("No agent response to copy");
                }
            }
            SlashCommandId::Model => match surface.resolve_model(&invocation.args) {
                Ok(selection) => match session.update_model(&selection).await {
                    Ok(()) => surface.set_notice(format!(
                        "Switching to {} ({})",
                        selection.display_name, selection.effort
                    )),
                    Err(error) => surface.set_notice(error.to_string()),
                },
                Err(error) => surface.set_notice(error.to_string()),
            },
            SlashCommandId::Compact => match session.compact().await {
                Ok(()) => surface.set_notice("Compacting conversation…"),
                Err(error) => surface.set_notice(error.to_string()),
            },
            SlashCommandId::New => match session.start_new().await {
                Ok(outcome) => reset_surface_after_switch(session, surface, outcome).await,
                Err(error) => surface.set_notice(error.to_string()),
            },
            SlashCommandId::Resume => match session.list_threads(None).await {
                Ok(page) => {
                    surface.open_thread_picker(PickerState::new(ThreadPickerAction::Resume, page))
                }
                Err(error) => surface.set_notice(error.to_string()),
            },
            SlashCommandId::Fork => match session.fork_current().await {
                Ok(outcome) => reset_surface_after_switch(session, surface, outcome).await,
                Err(error) => surface.set_notice(error.to_string()),
            },
            SlashCommandId::Rename => {
                let name = invocation.args.trim().to_string();
                match session.rename(name.clone()).await {
                    Ok(()) => surface.set_notice(format!("Renamed conversation to {name}")),
                    Err(error) => surface.set_notice(error.to_string()),
                }
            }
            SlashCommandId::Plan => {
                let switched = set_collaboration_mode(session, surface, ModeKind::Plan).await;
                if switched && !invocation.args.is_empty() {
                    let submission =
                        submission.into_slash_args(invocation.name, invocation.args.clone());
                    start_submission(session, surface, submission).await;
                } else if !switched && !invocation.args.is_empty() {
                    surface.restore_submission(submission);
                }
            }
            SlashCommandId::Permissions => {
                let current_profile = session
                    .state()
                    .and_then(|state| state.active_permission_profile.as_ref())
                    .map(|profile| profile.id.clone());
                surface.open_permission_picker(PermissionPickerState::new(current_profile));
            }
            SlashCommandId::Theme => {
                let name = invocation.args.trim();
                if name.is_empty() {
                    surface.open_theme_picker();
                } else if let Some(theme) = AstralThemeId::from_name(name) {
                    surface.set_theme(theme);
                    *theme_selection = Some(theme.config_name().to_string());
                    surface.set_notice(format!("Switched to {}", theme.label()));
                } else {
                    surface
                        .set_notice("Unknown theme. Available: astral-night, astral-day, terminal");
                }
            }
            SlashCommandId::Timeline => {
                let visible = surface.toggle_timeline();
                surface.set_notice(if visible {
                    "Timeline rail enabled"
                } else {
                    "Timeline rail hidden"
                });
            }
            SlashCommandId::Status => {
                if let Some(state) = session.state() {
                    let tokens = surface.token_usage().map_or_else(
                        || "not reported".to_string(),
                        |usage| {
                            usage.model_context_window.map_or_else(
                                || usage.last.total_tokens.to_string(),
                                |window| format!("{} / {window}", usage.last.total_tokens),
                            )
                        },
                    );
                    surface.open_modal(ModalState::info(
                        "Session status",
                        vec![
                            ModalRow::new("Thread", state.thread.id.clone()),
                            ModalRow::new(
                                "Name",
                                state
                                    .thread
                                    .name
                                    .clone()
                                    .unwrap_or_else(|| "untitled".to_string()),
                            ),
                            ModalRow::new(
                                "Model",
                                format!("{} · {}", state.model, state.model_provider),
                            ),
                            ModalRow::new(
                                "Mode",
                                format!("{:?}", state.collaboration_mode.mode).to_lowercase(),
                            ),
                            ModalRow::new(
                                "Permissions",
                                crate::permission_picker::display_permission_mode(
                                    state
                                        .active_permission_profile
                                        .as_ref()
                                        .map(|profile| profile.id.as_str()),
                                ),
                            ),
                            ModalRow::new("Working directory", state.thread.cwd.to_string_lossy()),
                            ModalRow::new("Context", tokens),
                        ],
                    ));
                }
            }
            SlashCommandId::Mcp => {
                let detail = match invocation.args.trim() {
                    "" => McpServerStatusDetail::ToolsAndAuthOnly,
                    "verbose" => McpServerStatusDetail::Full,
                    _ => {
                        surface.set_notice("Usage: /mcp [verbose]");
                        return Ok(None);
                    }
                };
                match session.list_mcp_servers(detail).await {
                    Ok(response) => surface.open_modal(mcp_panel(response, detail)),
                    Err(error) => surface.set_notice(error.to_string()),
                }
            }
            SlashCommandId::Skills => match session.list_skills().await {
                Ok(response) => surface.open_modal(skills_panel(response)),
                Err(error) => surface.set_notice(error.to_string()),
            },
            SlashCommandId::Hooks => match session.list_hooks().await {
                Ok(response) => surface.open_modal(hooks_panel(response)),
                Err(error) => surface.set_notice(error.to_string()),
            },
            SlashCommandId::Apps => match session.list_apps().await {
                Ok(response) => surface.open_modal(apps_panel(response)),
                Err(error) => surface.set_notice(error.to_string()),
            },
            SlashCommandId::Plugins => match session.list_plugins().await {
                Ok(response) => surface.open_modal(plugins_panel(response)),
                Err(error) => surface.set_notice(error.to_string()),
            },
        },
        InputAction::Resolve(resolution) => {
            let request_id = resolution.request_id().clone();
            match session.resolve(resolution).await {
                Ok(()) => {
                    surface.remove_pending_request(&request_id);
                }
                Err(error) => surface.set_notice(format!(
                    "Could not send response; the request remains open: {error}"
                )),
            }
        }
        InputAction::Notice(message) => surface.set_notice(message),
    }
    Ok(None)
}

fn configured_theme(name: Option<&str>) -> Option<AstralThemeId> {
    name.and_then(AstralThemeId::from_name)
}

async fn set_collaboration_mode(
    session: &mut AstralSession,
    surface: &mut SurfaceState,
    mode: ModeKind,
) -> bool {
    match session.update_collaboration_mode(mode).await {
        Ok(()) => {
            let mode = format!("{mode:?}").to_lowercase();
            surface.set_notice(format!("Switched to {mode} mode"));
            true
        }
        Err(error) => {
            surface.set_notice(error.to_string());
            false
        }
    }
}

async fn start_submission(
    session: &mut AstralSession,
    surface: &mut SurfaceState,
    submission: PromptSubmission,
) {
    // Follow mode already tracks the new turn. Keep a manual reading anchor
    // in place while the submitted turn appends below it.
    surface.set_activity(SurfaceActivity::Working);
    if let Err(error) = session.start_turn(submission.user_input()).await {
        surface.restore_submission(submission);
        surface.set_activity(SurfaceActivity::Ready);
        surface.set_notice(error.to_string());
    }
}

async fn reset_surface(session: &mut AstralSession, surface: &mut SurfaceState) {
    let Some(state) = session.state().cloned() else {
        surface.set_notice("No Astral thread is active");
        return;
    };
    let theme = surface.theme_id();
    let color_level = surface.color_level();
    let timeline_visible = surface.timeline_visible();
    *surface = SurfaceState::from_session(&state);
    surface.set_theme(theme);
    surface.set_color_level(color_level);
    surface.set_timeline_visible(timeline_visible);
    match session.list_models().await {
        Ok(models) => {
            surface.set_model_catalog(models, state.model.clone(), state.model_provider.clone())
        }
        Err(error) => surface.set_notice(format!("Could not load model catalog: {error}")),
    }
    mentions::refresh_catalog(session, surface).await;
}

async fn reset_surface_after_switch(
    session: &mut AstralSession,
    surface: &mut SurfaceState,
    outcome: ThreadSwitchOutcome,
) {
    reset_surface(session, surface).await;
    if let Some(warning) = outcome.unsubscribe_warning {
        surface.set_notice(warning);
    }
}

async fn handle_app_event(
    session: &AstralSession,
    surface: &mut SurfaceState,
    client_tools: &ClientToolRegistry,
    client_tool_tasks: &mut JoinSet<ClientToolCompletion>,
    event: AppServerEvent,
) -> Result<(), RunError> {
    let active_thread_id = session
        .state()
        .map(|state| state.thread.id.as_str())
        .ok_or(RunError::NoThread)?;
    match event {
        AppServerEvent::Lagged { skipped } => surface.conversation_mut().record_lag(skipped),
        AppServerEvent::ServerNotification(notification) => {
            handle_notification(surface, &notification);
            let mode = session
                .state()
                .map(|state| state.collaboration_mode.mode)
                .unwrap_or(ModeKind::Default);
            plan::handle_notification(surface, &notification, mode);
        }
        AppServerEvent::ServerRequest(request)
            if PendingRequest::from(request.clone())
                .thread_id()
                .is_none_or(|thread_id| thread_id == active_thread_id) =>
        {
            match request {
                ServerRequest::DynamicToolCall { request_id, params } => {
                    surface
                        .pending_requests_mut()
                        .note(ServerRequest::DynamicToolCall {
                            request_id: request_id.clone(),
                            params: params.clone(),
                        });
                    let client_tools = client_tools.clone();
                    client_tool_tasks.spawn(async move {
                        ClientToolCompletion {
                            request_id,
                            result: client_tools.call(params).await,
                        }
                    });
                }
                request @ (ServerRequest::AttestationGenerate { .. }
                | ServerRequest::ApplyPatchApproval { .. }
                | ServerRequest::ExecCommandApproval { .. }) => {
                    let request_id = request.id().clone();
                    surface.pending_requests_mut().note(request);
                    let resolution = surface.pending_requests().prepare_resolution(
                        &request_id,
                        PendingRequestResponse::Reject {
                            code: -32601,
                            message: "request is not supported by the Astral v2 surface"
                                .to_string(),
                        },
                    );
                    if let Ok(resolution) = resolution {
                        match session.resolve(resolution).await {
                            Ok(()) => {
                                surface.remove_pending_request(&request_id);
                            }
                            Err(error) => surface.set_notice(format!(
                                "Could not reject unsupported request; it remains open: {error}"
                            )),
                        }
                    }
                }
                request => surface.pending_requests_mut().note(request),
            }
        }
        AppServerEvent::ServerRequest(_) => {}
        AppServerEvent::Disconnected { message } => {
            surface.set_activity(SurfaceActivity::Disconnected(message));
        }
    }
    Ok(())
}

fn handle_notification(surface: &mut SurfaceState, notification: &ServerNotification) {
    let active_thread_id = surface.conversation().thread_id().to_string();
    surface.conversation_mut().apply(notification);
    match notification {
        ServerNotification::TurnStarted(params) if params.thread_id == active_thread_id => {
            surface.clear_notice();
            surface.set_activity(SurfaceActivity::Working);
        }
        ServerNotification::TurnCompleted(params) if params.thread_id == active_thread_id => {
            match &params.turn.status {
                TurnStatus::Completed => surface.set_activity(SurfaceActivity::Ready),
                TurnStatus::Interrupted => surface.set_activity(SurfaceActivity::Interrupted),
                TurnStatus::Failed => {
                    surface.set_activity(SurfaceActivity::Ready);
                    if let Some(error) = &params.turn.error {
                        surface.set_notice(error.message.clone());
                    }
                }
                TurnStatus::InProgress => surface.set_activity(SurfaceActivity::Working),
            }
        }
        ServerNotification::ServerRequestResolved(params)
            if params.thread_id == active_thread_id =>
        {
            let belongs_to_thread = surface
                .pending_requests()
                .get(&params.request_id)
                .and_then(PendingRequest::thread_id)
                == Some(params.thread_id.as_str());
            if belongs_to_thread {
                surface.remove_pending_request(&params.request_id);
            }
        }
        ServerNotification::ThreadTokenUsageUpdated(params)
            if params.thread_id == active_thread_id =>
        {
            surface.set_token_usage(params.token_usage.clone());
        }
        ServerNotification::ThreadSettingsUpdated(params)
            if params.thread_id == active_thread_id =>
        {
            surface.update_current_model(
                params.thread_settings.model.clone(),
                params.thread_settings.model_provider.clone(),
            );
            surface.set_notice(format!("Model changed to {}", params.thread_settings.model));
        }
        ServerNotification::ContextCompacted(params) if params.thread_id == active_thread_id => {
            surface.set_notice("Conversation compacted");
        }
        ServerNotification::Error(params) if params.thread_id == active_thread_id => {
            surface.set_notice(params.error.message.clone());
        }
        ServerNotification::Warning(params)
            if params
                .thread_id
                .as_deref()
                .is_none_or(|thread_id| thread_id == active_thread_id) =>
        {
            surface.set_notice(params.message.clone());
        }
        _ => {}
    }
}

struct ClientToolCompletion {
    request_id: RequestId,
    result: Result<DynamicToolCallResponse, ClientToolError>,
}

async fn resolve_client_tool(
    session: &AstralSession,
    surface: &mut SurfaceState,
    completion: ClientToolCompletion,
) -> Result<(), RunError> {
    let response = match completion.result {
        Ok(response) => PendingRequestResponse::DynamicTool(response),
        Err(error) => {
            surface.set_notice(error.message.clone());
            PendingRequestResponse::Reject {
                code: -32601,
                message: error.message,
            }
        }
    };
    let resolution = surface
        .pending_requests()
        .prepare_resolution(&completion.request_id, response);
    if let Ok(resolution) = resolution {
        match session.resolve(resolution).await {
            Ok(()) => {
                surface.remove_pending_request(&completion.request_id);
            }
            Err(error) => surface.set_notice(format!(
                "Could not send client tool result; the request remains open: {error}"
            )),
        }
    }
    Ok(())
}

async fn reject_pending(session: &AstralSession, surface: &mut SurfaceState) {
    while let Some(request_id) = surface
        .pending_requests()
        .front()
        .map(|request| request.request_id().clone())
    {
        let Ok(resolution) = surface.pending_requests().prepare_resolution(
            &request_id,
            PendingRequestResponse::Reject {
                code: -32000,
                message: "Astral TUI closed".to_string(),
            },
        ) else {
            break;
        };
        let _ = session.resolve(resolution).await;
        surface.remove_pending_request(&request_id);
    }
}

fn desired_viewport_rows(configured_rows: u16) -> io::Result<u16> {
    let (_, terminal_rows) = crossterm::terminal::size()?;
    Ok(viewport_rows(configured_rows, terminal_rows))
}

fn viewport_rows(configured_rows: u16, terminal_rows: u16) -> u16 {
    configured_rows
        .max(5)
        .min(terminal_rows.saturating_sub(1).max(3))
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
