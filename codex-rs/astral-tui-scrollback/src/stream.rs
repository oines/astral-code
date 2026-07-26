use codex_app_server_protocol::FileUpdateChange;

/// Best-effort content received while a timeline item is still running.
///
/// The stream is discarded when the authoritative `item/completed` value
/// arrives. Keeping it separate avoids rewriting app-server history while
/// still allowing the TUI to render when `item/started` was dropped.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum TimelineStream {
    #[default]
    None,
    AgentMessage(String),
    Plan(String),
    Reasoning {
        summary: Vec<String>,
        content: Vec<String>,
    },
    Command {
        process_id: Option<String>,
        output: String,
        terminal_input: Vec<String>,
    },
    FileChange {
        output: String,
        changes: Vec<FileUpdateChange>,
    },
}

impl TimelineStream {
    pub fn append_agent_message(&mut self, delta: &str) {
        match self {
            Self::AgentMessage(text) => text.push_str(delta),
            _ => *self = Self::AgentMessage(delta.to_owned()),
        }
    }

    pub fn append_plan(&mut self, delta: &str) {
        match self {
            Self::Plan(text) => text.push_str(delta),
            _ => *self = Self::Plan(delta.to_owned()),
        }
    }

    pub fn append_reasoning_summary(&mut self, index: usize, delta: &str) {
        let Self::Reasoning { summary, .. } = self.ensure_reasoning() else {
            unreachable!("ensure_reasoning always returns reasoning state");
        };
        append_indexed(summary, index, delta);
    }

    pub fn append_reasoning_content(&mut self, index: usize, delta: &str) {
        let Self::Reasoning { content, .. } = self.ensure_reasoning() else {
            unreachable!("ensure_reasoning always returns reasoning state");
        };
        append_indexed(content, index, delta);
    }

    pub fn append_command_output(&mut self, delta: &str) {
        let Self::Command { output, .. } = self.ensure_command() else {
            unreachable!("ensure_command always returns command state");
        };
        output.push_str(delta);
    }

    pub fn append_terminal_input(&mut self, process_id: &str, stdin: &str) {
        let Self::Command {
            process_id: current_process_id,
            terminal_input,
            ..
        } = self.ensure_command()
        else {
            unreachable!("ensure_command always returns command state");
        };
        *current_process_id = Some(process_id.to_owned());
        terminal_input.push(stdin.to_owned());
    }

    pub fn append_file_change_output(&mut self, delta: &str) {
        let Self::FileChange { output, .. } = self.ensure_file_change() else {
            unreachable!("ensure_file_change always returns file-change state");
        };
        output.push_str(delta);
    }

    pub fn replace_file_changes(&mut self, changes: Vec<FileUpdateChange>) {
        let Self::FileChange {
            changes: current, ..
        } = self.ensure_file_change()
        else {
            unreachable!("ensure_file_change always returns file-change state");
        };
        *current = changes;
    }

    fn ensure_reasoning(&mut self) -> &mut Self {
        if !matches!(self, Self::Reasoning { .. }) {
            *self = Self::Reasoning {
                summary: Vec::new(),
                content: Vec::new(),
            };
        }
        self
    }

    fn ensure_command(&mut self) -> &mut Self {
        if !matches!(self, Self::Command { .. }) {
            *self = Self::Command {
                process_id: None,
                output: String::new(),
                terminal_input: Vec::new(),
            };
        }
        self
    }

    fn ensure_file_change(&mut self) -> &mut Self {
        if !matches!(self, Self::FileChange { .. }) {
            *self = Self::FileChange {
                output: String::new(),
                changes: Vec::new(),
            };
        }
        self
    }
}

fn append_indexed(parts: &mut Vec<String>, index: usize, delta: &str) {
    if parts.len() <= index {
        parts.resize_with(index + 1, String::new);
    }
    parts[index].push_str(delta);
}
