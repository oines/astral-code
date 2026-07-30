use std::collections::HashMap;

use astral_tui_scrollback::PresentationBlock;
use astral_tui_scrollback::TimelineStream;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnPlanUpdatedNotification;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EntryPhase {
    Running,
    Settling,
    Stable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MutationSource {
    Live,
    Replay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TextStreamKind {
    Agent,
    Plan,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum TranscriptMutation {
    ItemStarted {
        item: ThreadItem,
        started_at_ms: i64,
    },
    ItemCompleted {
        item: ThreadItem,
        completed_at_ms: i64,
    },
    AgentMessageDelta {
        item_id: String,
        delta: String,
    },
    PlanDelta {
        item_id: String,
        delta: String,
    },
    ReasoningSummaryDelta {
        item_id: String,
        index: usize,
        delta: String,
    },
    ReasoningContentDelta {
        item_id: String,
        index: usize,
        delta: String,
    },
    CommandOutputDelta {
        item_id: String,
        delta: String,
    },
    TerminalInteraction {
        item_id: String,
        process_id: String,
        stdin: String,
    },
    FileChangeOutputDelta {
        item_id: String,
        delta: String,
    },
    FileChangePatchUpdated {
        item_id: String,
        changes: Vec<FileUpdateChange>,
    },
    TurnPlanUpdated(TurnPlanUpdatedNotification),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct TurnTiming {
    pub(super) started_at_ms: Option<i64>,
    pub(super) completed_at_ms: Option<i64>,
    pub(super) duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EntryLocation {
    pub(super) turn: usize,
    pub(super) entry: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ConversationEntry {
    pub(super) local_id: u64,
    pub(super) provider_id: Option<String>,
    pub(super) item: Option<ThreadItem>,
    pub(super) presentation: Option<PresentationBlock>,
    pub(super) stream: TimelineStream,
    pub(super) phase: EntryPhase,
    pub(super) completion_observed: bool,
    pub(super) started_at_ms: Option<i64>,
    pub(super) completed_at_ms: Option<i64>,
}

impl ConversationEntry {
    pub(super) fn new(local_id: u64, provider_id: Option<String>) -> Self {
        Self {
            local_id,
            provider_id,
            item: None,
            presentation: None,
            stream: TimelineStream::None,
            phase: EntryPhase::Running,
            completion_observed: false,
            started_at_ms: None,
            completed_at_ms: None,
        }
    }

    pub(super) fn render_id(&self) -> String {
        format!("entry-{}", self.local_id)
    }
}

#[derive(Debug, Clone)]
pub(super) struct ConversationTurn {
    pub(super) id: String,
    pub(super) entries: Vec<ConversationEntry>,
    pub(super) provider_indices: HashMap<String, usize>,
    pub(super) todo_entry: Option<usize>,
    pub(super) active_text: Option<(usize, TextStreamKind)>,
    pub(super) active_reasoning: Option<usize>,
    pub(super) committed_entries: usize,
    pub(super) timing: TurnTiming,
    pub(super) sealed: bool,
}

impl ConversationTurn {
    pub(super) fn new(id: String) -> Self {
        Self {
            id,
            entries: Vec::new(),
            provider_indices: HashMap::new(),
            todo_entry: None,
            active_text: None,
            active_reasoning: None,
            committed_entries: 0,
            timing: TurnTiming::default(),
            sealed: false,
        }
    }
}
