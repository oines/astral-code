//! Collaboration cards preserve Astral's spawn/send/wait/resume/close
//! operations while borrowing Grok's compact, foldable visual grammar.

use std::collections::HashSet;

use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentStatus;
use codex_app_server_protocol::CollabAgentTool;

use crate::CollabAgentToolCallBlock;
use crate::DisplayMode;
use crate::EntryDisplayState;
use crate::MarkdownLine;

use super::EntryRenderOptions;
use super::tool_card::ToolCardHeader;
use super::tool_card::ToolCardStatus;
use super::tool_card::append_section;
use super::tool_card::bounded_output_lines;
use super::tool_card::render_body;
use super::tool_card::render_header;

const MAX_PROMPT_CHARS: usize = 512;
const MAX_MESSAGE_CHARS: usize = 240;
const MAX_AGENT_ROWS: usize = 8;
const SHORT_THREAD_ID_CHARS: usize = 8;

pub(super) fn render(
    call: &CollabAgentToolCallBlock<'_>,
    state: EntryDisplayState,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let status = status(call);
    let (title, detail) = header_parts(call);
    let mut lines = render_header(
        ToolCardHeader {
            title: Some(title),
            detail,
            status,
            duration_ms: None,
        },
        state.mode(),
        options,
    );
    if state.mode() == DisplayMode::Collapsed {
        return lines;
    }

    append_section(&mut lines, render_body(detail_lines(call), status, options));
    lines
}

fn status(call: &CollabAgentToolCallBlock<'_>) -> ToolCardStatus {
    if call.failed() {
        ToolCardStatus::Failed
    } else if call.running() {
        ToolCardStatus::Running
    } else {
        ToolCardStatus::Succeeded
    }
}

fn header_parts(call: &CollabAgentToolCallBlock<'_>) -> (String, String) {
    let target = call
        .receiver_thread_ids()
        .first()
        .map(String::as_str)
        .map(agent_label);
    let spawn_config = spawn_config(call);
    match call.tool() {
        CollabAgentTool::SpawnAgent if call.running() => {
            ("Spawning agent".to_string(), spawn_config)
        }
        CollabAgentTool::SpawnAgent if call.failed() => {
            ("Agent spawn failed".to_string(), spawn_config)
        }
        CollabAgentTool::SpawnAgent => (
            target
                .as_ref()
                .map_or_else(|| "Spawned agent".to_string(), |_| "Spawned".to_string()),
            join_detail(target, spawn_config),
        ),
        CollabAgentTool::SendInput => target_header(
            phase_label(
                call,
                "Sending input to",
                "Sent input to",
                "Failed to send input to",
            ),
            target,
        ),
        CollabAgentTool::ResumeAgent => target_header(
            phase_label(call, "Resuming", "Resumed", "Failed to resume"),
            target,
        ),
        CollabAgentTool::Wait if call.running() => wait_header(call, "Waiting for"),
        CollabAgentTool::Wait if call.failed() => ("Waiting failed".to_string(), String::new()),
        CollabAgentTool::Wait => ("Finished waiting".to_string(), String::new()),
        CollabAgentTool::CloseAgent => target_header(
            phase_label(call, "Closing", "Closed", "Failed to close"),
            target,
        ),
    }
}

fn phase_label<'a>(
    call: &CollabAgentToolCallBlock<'_>,
    running: &'a str,
    completed: &'a str,
    failed: &'a str,
) -> &'a str {
    if call.failed() {
        failed
    } else if call.running() {
        running
    } else {
        completed
    }
}

fn target_header(prefix: &str, target: Option<String>) -> (String, String) {
    target.map_or_else(
        || {
            (
                prefix.strip_suffix(" to").unwrap_or(prefix).to_string(),
                String::new(),
            )
        },
        |target| (prefix.to_string(), target),
    )
}

fn wait_header(call: &CollabAgentToolCallBlock<'_>, prefix: &str) -> (String, String) {
    match call.receiver_thread_ids() {
        [receiver] => (prefix.to_string(), agent_label(receiver)),
        [] => ("Waiting for agents".to_string(), String::new()),
        receivers => (
            format!("Waiting for {} agents", receivers.len()),
            String::new(),
        ),
    }
}

fn spawn_config(call: &CollabAgentToolCallBlock<'_>) -> String {
    let config = [call.model(), call.reasoning_effort()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if config.is_empty() {
        config
    } else {
        format!("({config})")
    }
}

fn join_detail(target: Option<String>, suffix: String) -> String {
    match (target, suffix.is_empty()) {
        (Some(target), false) => format!("{target} {suffix}"),
        (Some(target), true) => target,
        (None, false) => suffix,
        (None, true) => String::new(),
    }
}

fn detail_lines(call: &CollabAgentToolCallBlock<'_>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(prompt) = call
        .prompt()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        let prompt = truncate_chars(prompt, MAX_PROMPT_CHARS);
        let mut prompt_lines = bounded_output_lines(&prompt).into_iter();
        if let Some(first) = prompt_lines.next() {
            lines.push(format!("prompt: {first}"));
            lines.extend(prompt_lines.map(|line| format!("  {line}")));
        }
    }
    append_agent_lines(&mut lines, call);
    lines
}

fn append_agent_lines(lines: &mut Vec<String>, call: &CollabAgentToolCallBlock<'_>) {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for receiver in call.receiver_thread_ids() {
        if seen.insert(receiver.as_str()) {
            entries.push((receiver.as_str(), call.agents_states().get(receiver)));
        }
    }
    let mut extras = call
        .agents_states()
        .iter()
        .filter(|(thread_id, _)| seen.insert(thread_id.as_str()))
        .map(|(thread_id, state)| (thread_id.as_str(), Some(state)))
        .collect::<Vec<_>>();
    extras.sort_by(|left, right| left.0.cmp(right.0));
    entries.extend(extras);

    let hidden = entries.len().saturating_sub(MAX_AGENT_ROWS);
    for (thread_id, state) in entries.into_iter().take(MAX_AGENT_ROWS) {
        let mut line = full_agent_label(thread_id);
        if let Some(state) = state {
            line.push_str(": ");
            line.push_str(agent_state_summary(state).as_str());
        }
        lines.push(line);
    }
    if hidden > 0 {
        lines.push(format!("… {hidden} more agents"));
    }
}

fn agent_state_summary(state: &CollabAgentState) -> String {
    let label = match state.status {
        CollabAgentStatus::PendingInit => "Pending init",
        CollabAgentStatus::Running => "Running",
        CollabAgentStatus::Interrupted => "Interrupted",
        CollabAgentStatus::Completed => "Completed",
        CollabAgentStatus::Errored => "Error",
        CollabAgentStatus::Shutdown => "Shutdown",
        CollabAgentStatus::NotFound => "Not found",
    };
    let Some(message) = state.message.as_deref() else {
        return label.to_string();
    };
    let message = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if message.is_empty() {
        label.to_string()
    } else {
        format!("{label} - {}", truncate_chars(&message, MAX_MESSAGE_CHARS))
    }
}

fn agent_label(thread_id: &str) -> String {
    format!("Agent {}", truncate_chars(thread_id, SHORT_THREAD_ID_CHARS))
}

fn full_agent_label(thread_id: &str) -> String {
    format!("agent {}", truncate_chars(thread_id, MAX_MESSAGE_CHARS))
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}
