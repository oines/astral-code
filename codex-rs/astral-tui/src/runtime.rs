use std::io;
use std::io::Stdout;

use astral_terminal_inline::Terminal;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::DynamicToolCallResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use crossterm::event::DisableBracketedPaste;
use crossterm::event::EnableBracketedPaste;
use crossterm::event::Event;
use crossterm::event::EventStream;
use crossterm::execute;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
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
use crate::SurfaceActivity;
use crate::SurfaceState;
use crate::committed_height;
use crate::handle_key;
use crate::handle_paste;
use crate::paint_committed;
use crate::render_surface;

type AstralTerminal = Terminal<CrosstermBackend<Stdout>>;

#[derive(Clone)]
pub struct RunOptions {
    pub viewport_rows: u16,
    pub client_tools: ClientToolRegistry,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            viewport_rows: 12,
            client_tools: ClientToolRegistry::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunExitReason {
    UserRequested,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunExit {
    pub thread_id: String,
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
    let thread_id = initial_state.thread.id.clone();
    let mut surface = SurfaceState::from_session(&initial_state);
    let mut guard = TerminalGuard::enter()?;
    let viewport_rows = desired_viewport_rows(options.viewport_rows)?;
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = AstralTerminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Inline(viewport_rows),
        },
    )?;
    terminal.hide_cursor()?;

    let result = run_loop(&mut terminal, &mut session, &mut surface, options).await;
    let _ = terminal.show_cursor();
    drop(terminal);
    guard.restore();

    let reason = result?;
    session.shutdown().await?;
    Ok(RunExit { thread_id, reason })
}

async fn run_loop(
    terminal: &mut AstralTerminal,
    session: &mut AstralSession,
    surface: &mut SurfaceState,
    options: RunOptions,
) -> Result<RunExitReason, RunError> {
    let mut input = EventStream::new();
    let mut client_tool_tasks = JoinSet::new();

    loop {
        draw(terminal, session, surface, options.viewport_rows)?;

        tokio::select! {
            terminal_event = input.next() => {
                let Some(terminal_event) = terminal_event else {
                    surface.set_activity(SurfaceActivity::Disconnected(
                        "terminal input closed".to_string(),
                    ));
                    draw(terminal, session, surface, options.viewport_rows)?;
                    reject_pending(session, surface).await;
                    return Ok(RunExitReason::Disconnected);
                };
                match terminal_event? {
                    Event::Key(key) => {
                        let action = handle_key(surface, key);
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
                    draw(terminal, session, surface, options.viewport_rows)?;
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
    configured_rows: u16,
) -> Result<(), RunError> {
    terminal.autoresize()?;
    let terminal_rows = terminal.size()?.height;
    let viewport_rows = viewport_rows(configured_rows, terminal_rows);
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

    let session_state = session.state().ok_or(RunError::NoThread)?;
    terminal.draw(|frame| {
        if let Some(position) =
            render_surface(surface, session_state, frame.area(), frame.buffer_mut())
        {
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
        InputAction::Resolve(resolution) => {
            if let Err(error) = session.resolve(resolution).await {
                surface.set_notice(error.to_string());
            }
        }
        InputAction::Notice(message) => surface.set_notice(message),
    }
    Ok(None)
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

struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        execute!(std::io::stdout(), EnableBracketedPaste)?;
        if let Err(error) = enable_raw_mode() {
            let _ = execute!(std::io::stdout(), DisableBracketedPaste);
            return Err(error);
        }
        Ok(Self { active: true })
    }

    fn restore(&mut self) {
        if !self.active {
            return;
        }
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), DisableBracketedPaste);
        self.active = false;
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
