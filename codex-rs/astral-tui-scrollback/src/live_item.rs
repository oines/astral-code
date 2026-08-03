use codex_app_server_protocol::FileUpdateChange;

/// Transient deltas for a running item; its completed `ThreadItem` is authoritative.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum LiveItem {
    #[default]
    None,
    AgentMessage(String),
    Plan(String),
    Reasoning {
        summary: Vec<String>,
        content: Vec<String>,
    },
    Command {
        output: String,
        terminal_input: Vec<String>,
    },
    FileChange {
        changes: Vec<FileUpdateChange>,
    },
}

impl LiveItem {
    pub(crate) fn file_changes(&self) -> &[FileUpdateChange] {
        match self {
            Self::FileChange { changes } => changes,
            Self::None
            | Self::AgentMessage(_)
            | Self::Plan(_)
            | Self::Reasoning { .. }
            | Self::Command { .. } => &[],
        }
    }

    pub(crate) fn command_output(&self) -> Option<&str> {
        match self {
            Self::Command { output, .. } if !output.is_empty() => Some(output),
            Self::None
            | Self::AgentMessage(_)
            | Self::Plan(_)
            | Self::Reasoning { .. }
            | Self::Command { .. }
            | Self::FileChange { .. } => None,
        }
    }

    pub(crate) fn terminal_input(&self) -> &[String] {
        match self {
            Self::Command { terminal_input, .. } => terminal_input,
            Self::None
            | Self::AgentMessage(_)
            | Self::Plan(_)
            | Self::Reasoning { .. }
            | Self::FileChange { .. } => &[],
        }
    }

    pub(crate) fn append_agent_message(&mut self, delta: &str) {
        match self {
            Self::AgentMessage(text) => text.push_str(delta),
            _ => *self = Self::AgentMessage(delta.to_owned()),
        }
    }

    pub(crate) fn append_plan(&mut self, delta: &str) {
        match self {
            Self::Plan(text) => text.push_str(delta),
            _ => *self = Self::Plan(delta.to_owned()),
        }
    }

    pub(crate) fn add_reasoning_summary_part(&mut self, index: usize) {
        let Self::Reasoning { summary, .. } = self.reasoning_mut() else {
            unreachable!("reasoning_mut always returns reasoning state");
        };
        ensure_part(summary, index);
    }

    pub(crate) fn append_reasoning_summary(&mut self, index: usize, delta: &str) {
        let Self::Reasoning { summary, .. } = self.reasoning_mut() else {
            unreachable!("reasoning_mut always returns reasoning state");
        };
        append_part(summary, index, delta);
    }

    pub(crate) fn append_reasoning_content(&mut self, index: usize, delta: &str) {
        let Self::Reasoning { content, .. } = self.reasoning_mut() else {
            unreachable!("reasoning_mut always returns reasoning state");
        };
        append_part(content, index, delta);
    }

    pub(crate) fn append_command_output(&mut self, delta: &str) {
        let Self::Command { output, .. } = self.command_mut() else {
            unreachable!("command_mut always returns command state");
        };
        output.push_str(delta);
    }

    pub(crate) fn append_terminal_input(&mut self, stdin: &str) {
        let Self::Command { terminal_input, .. } = self.command_mut() else {
            unreachable!("command_mut always returns command state");
        };
        terminal_input.push(stdin.to_owned());
    }

    pub(crate) fn replace_file_changes(&mut self, changes: Vec<FileUpdateChange>) {
        let Self::FileChange {
            changes: current, ..
        } = self.file_change_mut()
        else {
            unreachable!("file_change_mut always returns file-change state");
        };
        *current = changes;
    }

    fn reasoning_mut(&mut self) -> &mut Self {
        if !matches!(self, Self::Reasoning { .. }) {
            *self = Self::Reasoning {
                summary: Vec::new(),
                content: Vec::new(),
            };
        }
        self
    }

    fn command_mut(&mut self) -> &mut Self {
        if !matches!(self, Self::Command { .. }) {
            *self = Self::Command {
                output: String::new(),
                terminal_input: Vec::new(),
            };
        }
        self
    }

    fn file_change_mut(&mut self) -> &mut Self {
        if !matches!(self, Self::FileChange { .. }) {
            *self = Self::FileChange {
                changes: Vec::new(),
            };
        }
        self
    }
}

fn ensure_part(parts: &mut Vec<String>, index: usize) {
    if parts.len() <= index {
        parts.resize_with(index + 1, String::new);
    }
}

fn append_part(parts: &mut Vec<String>, index: usize, delta: &str) {
    ensure_part(parts, index);
    parts[index].push_str(delta);
}
