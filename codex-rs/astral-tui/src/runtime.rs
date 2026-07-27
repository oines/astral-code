use std::io;
use std::io::Stdout;

use astral_terminal_inline::Terminal;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::DynamicToolCallResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use crossterm::event::Event;
use crossterm::event::EventStream;
use ratatui::TerminalOptions;
use ratatui::Viewport;
use ratatui::backend::CrosstermBackend;
use tokio::task::JoinSet;
use tokio_stream::StreamExt;

use crate::AstralSession;
use crate::ClientToolError;
use crate::ClientToolRegistry;
use crate::InputAction;
use crate::PendingRequestResponse;
use crate::SessionError;
use crate::SlashCommandId;
use crate::SurfaceActivity;
use crate::SurfaceState;
use crate::ThreadPickerAction;
use crate::TranscriptView;
use crate::clipboard::copy_to_clipboard;
use crate::committed_height;
use crate::handle_key;
use crate::handle_paste;
use crate::modal::ModalRow;
use crate::modal::ModalState;
use crate::paint_committed;
use crate::render_surface;
use crate::render_surface_with_view;
use crate::terminal_guard::TerminalGuard;
use crate::thread_picker::PickerState;

type AstralTerminal = Terminal<CrosstermBackend<Stdout>>;

#[derive(Clone)]
pub struct RunOptions {
    pub viewport: RunViewport,
    pub viewport_rows: u16,
    pub client_tools: ClientToolRegistry,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            viewport: RunViewport::Fullscreen,
            viewport_rows: 12,
            client_tools: ClientToolRegistry::default(),
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
    match session.list_models().await {
        Ok(models) => surface.set_model_catalog(
            models,
            initial_state.model.clone(),
            initial_state.model_provider.clone(),
        ),
        Err(error) => surface.set_notice(format!("Could not load model catalog: {error}")),
    }
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

    let result = run_loop(&mut terminal, &mut session, &mut surface, options).await;
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
        reason,
    })
}

async fn run_loop(
    terminal: &mut AstralTerminal,
    session: &mut AstralSession,
    surface: &mut SurfaceState,
    options: RunOptions,
) -> Result<RunExitReason, RunError> {
    let mut input = EventStream::new();
    let mut client_tool_tasks = JoinSet::new();
    let mut _clipboard_lease = None;

    loop {
        draw(terminal, session, surface, &options)?;

        tokio::select! {
            terminal_event = input.next() => {
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
                        let action = match handle_key(surface, key) {
                            InputAction::ScrollUp => {
                                if options.viewport == RunViewport::Fullscreen {
                                    let page_rows =
                                        usize::from(terminal.viewport_area().height.max(1));
                                    surface.scroll_up(page_rows);
                                } else {
                                    surface.set_notice(
                                        "Use the terminal's native scrollback in inline mode",
                                    );
                                }
                                InputAction::None
                            }
                            InputAction::ScrollDown => {
                                if options.viewport == RunViewport::Fullscreen {
                                    let page_rows =
                                        usize::from(terminal.viewport_area().height.max(1));
                                    surface.scroll_down(page_rows);
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
                            action => action,
                        };
                        if let Some(reason) =
                            apply_input_action(session, surface, action).await?
                        {
                            reject_pending(session, surface).await;
                            return Ok(reason);
                        }
                    }
                    Event::Paste(text) => {
                        let _ = handle_paste(surface, &text);
                    }
                    Event::Resize(_, _) => {}
                    Event::FocusGained | Event::FocusLost | Event::Mouse(_) => {}
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
            }
            completion = client_tool_tasks.join_next(), if !client_tool_tasks.is_empty() => {
                if let Some(completion) = completion {
                    let completion = completion.map_err(|error| {
                        RunError::Terminal(io::Error::other(format!(
                            "client tool task failed: {error}"
                        )))
                    })?;
                    resolve_client_tool(session, surface, completion).await?;
                }
            }
        }
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
        for block in surface.drain_committable() {
            let height = committed_height(&block, width);
            if height > 0 {
                terminal.insert_before(height, move |buffer| {
                    paint_committed(&block, buffer);
                })?;
                terminal.insert_before(1, |_buffer| {})?;
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
    action: InputAction,
) -> Result<Option<RunExitReason>, RunError> {
    match action {
        InputAction::None | InputAction::Redraw => {}
        InputAction::Submit(prompt) => {
            surface.scroll_to_bottom();
            surface.set_activity(SurfaceActivity::Working);
            if let Err(error) = session
                .start_turn(vec![UserInput::Text {
                    text: prompt.clone(),
                    text_elements: Vec::new(),
                }])
                .await
            {
                surface.composer_mut().push_str(&prompt);
                surface.set_activity(SurfaceActivity::Ready);
                surface.set_notice(error.to_string());
            }
        }
        InputAction::Interrupt => match session.interrupt().await {
            Ok(()) => surface.set_activity(SurfaceActivity::Interrupted),
            Err(error) => surface.set_notice(error.to_string()),
        },
        InputAction::Exit => return Ok(Some(RunExitReason::UserRequested)),
        InputAction::ScrollUp | InputAction::ScrollDown | InputAction::CopyLastResponse => {}
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
                ThreadPickerAction::Fork => {
                    session
                        .fork(codex_app_server_protocol::ThreadForkParams {
                            thread_id: thread.id,
                            ..codex_app_server_protocol::ThreadForkParams::default()
                        })
                        .await
                }
            };
            match result {
                Ok(_) => reset_surface(session, surface).await,
                Err(error) => surface.set_notice(error.to_string()),
            }
        }
        InputAction::Slash(invocation) => match invocation.command {
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
                Ok(_) => reset_surface(session, surface).await,
                Err(error) => surface.set_notice(error.to_string()),
            },
            SlashCommandId::Resume => match session.list_threads(None).await {
                Ok(page) => {
                    surface.open_thread_picker(PickerState::new(ThreadPickerAction::Resume, page))
                }
                Err(error) => surface.set_notice(error.to_string()),
            },
            SlashCommandId::Fork => match session.fork_current().await {
                Ok(_) => reset_surface(session, surface).await,
                Err(error) => surface.set_notice(error.to_string()),
            },
            SlashCommandId::Rename => {
                let name = invocation.args.trim().to_string();
                match session.rename(name.clone()).await {
                    Ok(()) => surface.set_notice(format!("Renamed conversation to {name}")),
                    Err(error) => surface.set_notice(error.to_string()),
                }
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
                            ModalRow::new("Working directory", state.thread.cwd.to_string_lossy()),
                            ModalRow::new("Context", tokens),
                        ],
                    ));
                }
            }
            command => surface.set_notice(format!(
                "/{} is recognized; its Astral action is not available yet ({command:?})",
                invocation.name
            )),
        },
        InputAction::Resolve(resolution) => {
            if let Err(error) = session.resolve(resolution).await {
                surface.set_notice(error.to_string());
            }
        }
        InputAction::Notice(message) => surface.set_notice(message),
    }
    Ok(None)
}

async fn reset_surface(session: &mut AstralSession, surface: &mut SurfaceState) {
    let Some(state) = session.state().cloned() else {
        surface.set_notice("No Astral thread is active");
        return;
    };
    *surface = SurfaceState::from_session(&state);
    match session.list_models().await {
        Ok(models) => {
            surface.set_model_catalog(models, state.model.clone(), state.model_provider.clone())
        }
        Err(error) => surface.set_notice(format!("Could not load model catalog: {error}")),
    }
}

async fn handle_app_event(
    session: &AstralSession,
    surface: &mut SurfaceState,
    client_tools: &ClientToolRegistry,
    client_tool_tasks: &mut JoinSet<ClientToolCompletion>,
    event: AppServerEvent,
) -> Result<(), RunError> {
    match event {
        AppServerEvent::Lagged { skipped } => surface.conversation_mut().record_lag(skipped),
        AppServerEvent::ServerNotification(notification) => {
            handle_notification(surface, &notification);
        }
        AppServerEvent::ServerRequest(request) => match request {
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
                let resolution = surface.pending_requests_mut().resolve(
                    &request_id,
                    PendingRequestResponse::Reject {
                        code: -32601,
                        message: "request is not supported by the Astral v2 surface".to_string(),
                    },
                );
                if let Ok(resolution) = resolution {
                    session.resolve(resolution).await?;
                }
            }
            request => surface.pending_requests_mut().note(request),
        },
        AppServerEvent::Disconnected { message } => {
            surface.set_activity(SurfaceActivity::Disconnected(message));
        }
    }
    Ok(())
}

fn handle_notification(surface: &mut SurfaceState, notification: &ServerNotification) {
    surface.conversation_mut().apply(notification);
    match notification {
        ServerNotification::TurnStarted(_) => {
            surface.clear_notice();
            surface.set_activity(SurfaceActivity::Working);
        }
        ServerNotification::TurnCompleted(params) => match &params.turn.status {
            TurnStatus::Completed => surface.set_activity(SurfaceActivity::Ready),
            TurnStatus::Interrupted => surface.set_activity(SurfaceActivity::Interrupted),
            TurnStatus::Failed => {
                surface.set_activity(SurfaceActivity::Ready);
                if let Some(error) = &params.turn.error {
                    surface.set_notice(error.message.clone());
                }
            }
            TurnStatus::InProgress => surface.set_activity(SurfaceActivity::Working),
        },
        ServerNotification::ServerRequestResolved(params) => {
            surface
                .pending_requests_mut()
                .remove_resolved(&params.request_id);
        }
        ServerNotification::ThreadTokenUsageUpdated(params)
            if params.thread_id == surface.conversation().timeline().thread_id() =>
        {
            surface.set_token_usage(params.token_usage.clone());
        }
        ServerNotification::ThreadSettingsUpdated(params) => {
            surface.update_current_model(
                params.thread_settings.model.clone(),
                params.thread_settings.model_provider.clone(),
            );
            surface.set_notice(format!("Model changed to {}", params.thread_settings.model));
        }
        ServerNotification::ContextCompacted(_) => {
            surface.set_notice("Conversation compacted");
        }
        ServerNotification::Error(params) => surface.set_notice(params.error.message.clone()),
        ServerNotification::Warning(params) => surface.set_notice(params.message.clone()),
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
        .pending_requests_mut()
        .resolve(&completion.request_id, response);
    if let Ok(resolution) = resolution {
        session.resolve(resolution).await?;
    }
    Ok(())
}

async fn reject_pending(session: &AstralSession, surface: &mut SurfaceState) {
    while let Some(request_id) = surface
        .pending_requests()
        .front()
        .map(|request| request.request_id().clone())
    {
        let Ok(resolution) = surface.pending_requests_mut().resolve(
            &request_id,
            PendingRequestResponse::Reject {
                code: -32000,
                message: "Astral TUI closed".to_string(),
            },
        ) else {
            break;
        };
        let _ = session.resolve(resolution).await;
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
