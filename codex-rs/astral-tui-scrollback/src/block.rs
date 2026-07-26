use codex_app_server_protocol::FileUpdateChange;

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
    Tool(ToolPresentation),
    System {
        title: String,
        detail: Option<String>,
    },
}

/// Tool behavior as understood by the UI, independent of provider naming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Execute,
    Read,
    Edit,
    List,
    Search,
    WebFetch,
    WebSearch,
    Mcp,
    Skill,
    Collab,
    Media,
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

#[derive(Debug, Clone, PartialEq)]
pub struct ToolPresentation {
    pub kind: ToolKind,
    pub status: ToolStatus,
    pub name: String,
    pub title: String,
    pub details: Vec<String>,
    pub output: Option<String>,
    pub changes: Vec<FileUpdateChange>,
    pub duration_ms: Option<i64>,
}
