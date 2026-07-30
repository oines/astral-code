use codex_app_server_protocol::FileUpdateChange;

use crate::SubagentPresentation;
use crate::TodoPresentation;

/// Stable, renderer-facing transcript block.
#[derive(Debug, Clone, PartialEq)]
pub enum PresentationBlock {
    User {
        text: String,
        attachments: Vec<String>,
    },
    Assistant {
        text: String,
    },
    Thinking {
        text: String,
        running: bool,
    },
    Plan {
        text: String,
        running: bool,
    },
    Todo(TodoPresentation),
    Tool(ToolPresentation),
    Subagent(SubagentPresentation),
    System {
        title: String,
        detail: Option<String>,
    },
}

/// Tool behavior as understood by the UI, independent of provider naming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Execute,
    Background,
    BackgroundPoll,
    BackgroundInput,
    BackgroundList,
    BackgroundStop,
    Read,
    Edit,
    List,
    Search,
    WebFetch,
    WebSearch,
    Mcp,
    Skill,
    Collab,
    ImageView,
    ImageGeneration,
    Todo,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Success,
    Failed,
    Declined,
    Interrupted,
}

/// Who initiated a tool entry from the user's point of view.
///
/// User shell commands intentionally render like an interactive terminal,
/// while agent tools remain compact until the user opens them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOrigin {
    Agent,
    UserShell,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolPresentation {
    pub kind: ToolKind,
    pub origin: ToolOrigin,
    pub status: ToolStatus,
    pub name: String,
    pub title: String,
    pub details: Vec<String>,
    pub output: Option<String>,
    pub changes: Vec<FileUpdateChange>,
    pub duration_ms: Option<i64>,
}

impl ToolPresentation {
    pub fn is_user_shell(&self) -> bool {
        self.origin == ToolOrigin::UserShell
    }
}
