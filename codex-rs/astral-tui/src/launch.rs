use codex_app_server_client::AppServerClient;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::UserInput;

use crate::AstralSession;
use crate::RunError;
use crate::RunExit;
use crate::RunOptions;
use crate::SessionError;
use crate::run;

/// Selects the app-server thread lifecycle request used to enter the TUI.
#[derive(Debug, Clone, PartialEq)]
pub enum ThreadLaunch {
    Start(ThreadStartParams),
    Resume(ThreadResumeParams),
    Fork(ThreadForkParams),
}

/// Fully resolved inputs for one Astral TUI invocation.
///
/// CLI configuration and app-server transport selection intentionally happen
/// outside the TUI crate. This keeps the surface reusable by other clients
/// without teaching it about command-line flags or local daemon policy.
#[derive(Clone)]
pub struct LaunchOptions {
    pub thread: ThreadLaunch,
    pub initial_input: Vec<UserInput>,
    pub runtime: RunOptions,
}

impl LaunchOptions {
    pub fn new(thread: ThreadLaunch) -> Self {
        Self {
            thread,
            initial_input: Vec::new(),
            runtime: RunOptions::default(),
        }
    }
}

#[derive(Debug)]
pub enum LaunchError {
    Session(SessionError),
    Runtime(RunError),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(error) => write!(f, "failed to start Astral session: {error}"),
            Self::Runtime(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LaunchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
            Self::Runtime(error) => Some(error),
        }
    }
}

impl From<SessionError> for LaunchError {
    fn from(value: SessionError) -> Self {
        Self::Session(value)
    }
}

impl From<RunError> for LaunchError {
    fn from(value: RunError) -> Self {
        Self::Runtime(value)
    }
}

/// Activates one app-server thread and runs the native Astral terminal surface.
pub async fn run_main(
    client: AppServerClient,
    options: LaunchOptions,
) -> Result<RunExit, LaunchError> {
    let LaunchOptions {
        thread,
        initial_input,
        runtime,
    } = options;
    let mut session = AstralSession::new(client);

    let activation = match thread {
        ThreadLaunch::Start(params) => session.start(params).await.map(|_| ()),
        ThreadLaunch::Resume(params) => session.resume(params).await.map(|_| ()),
        ThreadLaunch::Fork(params) => session.fork(params).await.map(|_| ()),
    };
    if let Err(error) = activation {
        let _ = session.shutdown().await;
        return Err(error.into());
    }

    if !initial_input.is_empty()
        && let Err(error) = session.start_turn(initial_input).await
    {
        let _ = session.shutdown().await;
        return Err(error.into());
    }

    run(session, runtime).await.map_err(Into::into)
}
