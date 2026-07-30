use codex_app_server_protocol::CommandAction;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::DynamicToolCallOutputContentItem;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::UserInput;
use serde_json::Value;

use crate::PresentationBlock;
use crate::SubagentPresentation;
use crate::TimelineStream;
use crate::TodoPresentation;
use crate::ToolKind;
use crate::ToolOrigin;
use crate::ToolPresentation;
use crate::ToolStatus;
use crate::tool_semantics::classify_tool_name;
use crate::tool_semantics::command_presentation;
use crate::tool_semantics::command_status;
use crate::tool_semantics::compact_path;
use crate::tool_semantics::core_tool_status;
use crate::tool_semantics::dynamic_status;
use crate::tool_semantics::edit_title;
use crate::tool_semantics::mcp_status;
use crate::tool_semantics::patch_status;
use crate::tool_semantics::status_from_text;
use crate::tool_semantics::tool_call_title;

impl PresentationBlock {
    pub fn from_item(item: &ThreadItem, stream: &TimelineStream) -> Option<Self> {
        match item {
            ThreadItem::UserMessage { content, .. } => {
                let (text, attachments) = user_content(content);
                Some(Self::User { text, attachments })
            }
            ThreadItem::HookPrompt { fragments, .. } => Some(Self::System {
                title: "Hook context".to_string(),
                detail: Some(
                    fragments
                        .iter()
                        .map(|fragment| fragment.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            }),
            ThreadItem::AgentMessage { text, .. } => {
                let delta = match stream {
                    TimelineStream::AgentMessage(delta) => delta,
                    _ => "",
                };
                Some(Self::Assistant {
                    text: format!("{text}{delta}"),
                })
            }
            ThreadItem::Plan { text, .. } => {
                let delta = match stream {
                    TimelineStream::Plan(delta) => delta,
                    _ => "",
                };
                Some(Self::Plan {
                    text: format!("{text}{delta}"),
                    running: !matches!(stream, TimelineStream::None),
                })
            }
            ThreadItem::Reasoning {
                summary, content, ..
            } => {
                let mut parts = summary.clone();
                let mut streamed_content = &[][..];
                if let TimelineStream::Reasoning {
                    summary: streamed_summary,
                    content,
                } = stream
                {
                    append_stream_parts(&mut parts, streamed_summary);
                    streamed_content = content;
                }
                if parts.iter().all(|part| part.trim().is_empty()) {
                    parts.clone_from(content);
                    append_stream_parts(&mut parts, streamed_content);
                }
                Some(Self::Thinking {
                    text: parts.join("\n"),
                    running: !matches!(stream, TimelineStream::None),
                })
            }
            ThreadItem::CommandExecution {
                command,
                cwd,
                process_id,
                status,
                command_actions,
                aggregated_output,
                exit_code,
                duration_ms,
                source,
                ..
            } => {
                let origin = match source {
                    CommandExecutionSource::UserShell => ToolOrigin::UserShell,
                    CommandExecutionSource::Agent
                    | CommandExecutionSource::UnifiedExecStartup
                    | CommandExecutionSource::UnifiedExecInteraction => ToolOrigin::Agent,
                };
                let (mut kind, title) = if origin == ToolOrigin::UserShell {
                    let title = match command_actions.as_slice() {
                        [
                            CommandAction::Read { command, .. }
                            | CommandAction::ListFiles { command, .. }
                            | CommandAction::Search { command, .. }
                            | CommandAction::Unknown { command },
                        ] => command.clone(),
                        _ => command.clone(),
                    };
                    (ToolKind::Execute, title)
                } else {
                    command_presentation(command, command_actions)
                };
                let (stream_process_id, streamed_output, terminal_input) = match stream {
                    TimelineStream::Command {
                        process_id,
                        output,
                        terminal_input,
                    } => (
                        process_id.as_deref(),
                        output.as_str(),
                        terminal_input.as_slice(),
                    ),
                    _ => (None, "", &[][..]),
                };
                let process_id = process_id.as_deref().or(stream_process_id);
                if process_id.is_some() && origin == ToolOrigin::Agent {
                    kind = ToolKind::Background;
                }
                let output = merge_output(aggregated_output.as_deref(), streamed_output);
                let mut details = vec![format!("cwd {}", cwd.as_path().display())];
                if let Some(process_id) = process_id {
                    details.push(format!("process {process_id}"));
                }
                if let Some(detail) = terminal_input_detail(terminal_input) {
                    details.push(detail);
                }
                if let Some(exit_code) = exit_code {
                    details.push(format!("exit {exit_code}"));
                }
                Some(Self::Tool(ToolPresentation {
                    kind,
                    origin,
                    status: command_status(status),
                    name: "command".to_string(),
                    title,
                    details,
                    output,
                    changes: Vec::new(),
                    duration_ms: *duration_ms,
                }))
            }
            ThreadItem::FileChange {
                changes, status, ..
            } => {
                let streamed_changes = match stream {
                    TimelineStream::FileChange { changes, .. } if !changes.is_empty() => changes,
                    _ => changes,
                };
                Some(Self::Tool(ToolPresentation {
                    kind: ToolKind::Edit,
                    origin: ToolOrigin::Agent,
                    status: patch_status(status),
                    name: "edit".to_string(),
                    title: edit_title(streamed_changes),
                    details: Vec::new(),
                    output: None,
                    changes: streamed_changes.clone(),
                    duration_ms: None,
                }))
            }
            ThreadItem::McpToolCall {
                server,
                tool,
                status,
                arguments,
                result,
                error,
                duration_ms,
                ..
            } => {
                let title = tool_call_title(ToolKind::Mcp, tool, arguments);
                let output = error
                    .as_ref()
                    .map(|error| error.message.clone())
                    .or_else(|| result.as_deref().and_then(mcp_result_text));
                Some(Self::Tool(ToolPresentation {
                    kind: ToolKind::Mcp,
                    origin: ToolOrigin::Agent,
                    status: mcp_status(status),
                    name: format!("{server}/{tool}"),
                    title,
                    details: vec![format!("MCP · {server}")],
                    output,
                    changes: Vec::new(),
                    duration_ms: *duration_ms,
                }))
            }
            ThreadItem::DynamicToolCall {
                namespace,
                tool,
                arguments,
                status,
                content_items,
                success,
                duration_ms,
                ..
            } => {
                let kind = classify_tool_name(tool);
                let status = dynamic_status(status, *success);
                if kind == ToolKind::Todo
                    && status != ToolStatus::Failed
                    && let Some(todo) = TodoPresentation::from_tool_arguments(arguments)
                {
                    return Some(Self::Todo(todo));
                }
                Some(Self::Tool(ToolPresentation {
                    kind,
                    origin: ToolOrigin::Agent,
                    status,
                    name: namespace
                        .as_ref()
                        .map_or_else(|| tool.clone(), |namespace| format!("{namespace}/{tool}")),
                    title: tool_call_title(kind, tool, arguments),
                    details: namespace
                        .as_ref()
                        .map(|namespace| vec![namespace.clone()])
                        .unwrap_or_default(),
                    output: dynamic_output(content_items.as_deref()),
                    changes: Vec::new(),
                    duration_ms: *duration_ms,
                }))
            }
            ThreadItem::CoreToolCall {
                tool,
                arguments,
                status,
                result,
                error,
                duration_ms,
                ..
            } => {
                let kind = classify_tool_name(tool);
                let status = core_tool_status(*status);
                if kind == ToolKind::Todo
                    && !matches!(status, ToolStatus::Failed | ToolStatus::Interrupted)
                    && let Some(todo) = TodoPresentation::from_tool_arguments(arguments)
                {
                    return Some(Self::Todo(todo));
                }
                Some(Self::Tool(ToolPresentation {
                    kind,
                    origin: ToolOrigin::Agent,
                    status,
                    name: tool.clone(),
                    title: tool_call_title(kind, tool, arguments),
                    details: Vec::new(),
                    output: error.clone().or_else(|| result.clone()),
                    changes: Vec::new(),
                    duration_ms: *duration_ms,
                }))
            }
            ThreadItem::CollabAgentToolCall {
                tool,
                status,
                receiver_thread_ids,
                prompt,
                model,
                reasoning_effort,
                agents_states,
                ..
            } => Some(Self::Subagent(SubagentPresentation::from_collab(
                tool,
                status,
                receiver_thread_ids,
                prompt.as_deref(),
                model.as_deref(),
                reasoning_effort.as_ref().map(ToString::to_string),
                agents_states,
            ))),
            ThreadItem::WebSearch { query, .. } => Some(Self::Tool(ToolPresentation {
                kind: ToolKind::WebSearch,
                origin: ToolOrigin::Agent,
                status: ToolStatus::Success,
                name: "web_search".to_string(),
                title: query.clone(),
                details: Vec::new(),
                output: None,
                changes: Vec::new(),
                duration_ms: None,
            })),
            ThreadItem::ImageView { path, .. } => Some(Self::Tool(ToolPresentation {
                kind: ToolKind::ImageView,
                origin: ToolOrigin::Agent,
                status: ToolStatus::Success,
                name: "view_image".to_string(),
                title: compact_path(path.as_path()),
                details: vec![path.as_path().display().to_string()],
                output: None,
                changes: Vec::new(),
                duration_ms: None,
            })),
            ThreadItem::ImageGeneration {
                status,
                saved_path,
                revised_prompt,
                ..
            } => Some(Self::Tool(ToolPresentation {
                kind: ToolKind::ImageGeneration,
                origin: ToolOrigin::Agent,
                status: status_from_text(status),
                name: "image_generation".to_string(),
                title: saved_path.as_ref().map_or_else(
                    || "Generated image".to_string(),
                    |path| compact_path(path.as_path()),
                ),
                details: revised_prompt.iter().cloned().collect(),
                output: None,
                changes: Vec::new(),
                duration_ms: None,
            })),
            ThreadItem::EnteredReviewMode { review, .. } => Some(Self::System {
                title: "Entered review mode".to_string(),
                detail: Some(review.clone()),
            }),
            ThreadItem::ExitedReviewMode { review, .. } => Some(Self::System {
                title: "Exited review mode".to_string(),
                detail: Some(review.clone()),
            }),
            ThreadItem::ContextCompaction { .. } => Some(Self::System {
                title: "Context compacted".to_string(),
                detail: None,
            }),
        }
    }

    pub fn from_stream(stream: &TimelineStream) -> Option<Self> {
        match stream {
            TimelineStream::None => None,
            TimelineStream::AgentMessage(text) => Some(Self::Assistant { text: text.clone() }),
            TimelineStream::Plan(text) => Some(Self::Plan {
                text: text.clone(),
                running: true,
            }),
            TimelineStream::Reasoning { summary, content } => {
                let parts = if summary.iter().any(|part| !part.trim().is_empty()) {
                    summary
                } else {
                    content
                };
                Some(Self::Thinking {
                    text: parts.join("\n"),
                    running: true,
                })
            }
            TimelineStream::Command {
                process_id,
                output,
                terminal_input,
            } => {
                let mut details = process_id
                    .as_ref()
                    .map(|process_id| vec![format!("process {process_id}")])
                    .unwrap_or_default();
                if let Some(detail) = terminal_input_detail(terminal_input) {
                    details.push(detail);
                }
                Some(Self::Tool(ToolPresentation {
                    kind: if process_id.is_some() {
                        ToolKind::Background
                    } else {
                        ToolKind::Execute
                    },
                    origin: ToolOrigin::Agent,
                    status: ToolStatus::Running,
                    name: "command".to_string(),
                    title: "Running command".to_string(),
                    details,
                    output: non_empty(output.clone()),
                    changes: Vec::new(),
                    duration_ms: None,
                }))
            }
            TimelineStream::FileChange {
                output, changes, ..
            } => Some(Self::Tool(ToolPresentation {
                kind: ToolKind::Edit,
                origin: ToolOrigin::Agent,
                status: ToolStatus::Running,
                name: "edit".to_string(),
                title: edit_title(changes),
                details: changes.iter().map(|change| change.path.clone()).collect(),
                output: non_empty(output.clone()),
                changes: changes.clone(),
                duration_ms: None,
            })),
        }
    }
}

fn terminal_input_detail(terminal_input: &[String]) -> Option<String> {
    terminal_input.last().map(|stdin| {
        if stdin.is_empty() {
            "waiting for background output".to_string()
        } else {
            let mut escaped = stdin.escape_debug();
            let preview = escaped.by_ref().take(80).collect::<String>();
            if escaped.next().is_some() {
                format!("stdin {preview}…")
            } else {
                format!("stdin {preview}")
            }
        }
    })
}

fn user_content(content: &[UserInput]) -> (String, Vec<String>) {
    let mut text = Vec::new();
    let mut attachments = Vec::new();
    for input in content {
        match input {
            UserInput::Text { text: value, .. } => text.push(value.clone()),
            UserInput::Image { url, .. } => attachments.push(url.clone()),
            UserInput::LocalImage { path, .. } => {
                attachments.push(path.display().to_string());
            }
            UserInput::Skill { name, path } => {
                attachments.push(format!("skill {name} ({})", path.display()));
            }
            UserInput::Mention { name, path } => attachments.push(format!("@{name} ({path})")),
        }
    }
    (text.join("\n"), attachments)
}

fn append_stream_parts(base: &mut Vec<String>, delta: &[String]) {
    for (index, value) in delta.iter().enumerate() {
        if base.len() <= index {
            base.resize_with(index + 1, String::new);
        }
        base[index].push_str(value);
    }
}

fn merge_output(base: Option<&str>, streamed: &str) -> Option<String> {
    match (base.filter(|value| !value.is_empty()), streamed.is_empty()) {
        (Some(base), true) => Some(base.to_string()),
        (Some(base), false) => Some(format!("{base}{streamed}")),
        (None, false) => Some(streamed.to_string()),
        (None, true) => None,
    }
}

fn dynamic_output(items: Option<&[DynamicToolCallOutputContentItem]>) -> Option<String> {
    let text = items?
        .iter()
        .map(|item| match item {
            DynamicToolCallOutputContentItem::InputText { text } => text.clone(),
            DynamicToolCallOutputContentItem::InputImage { image_url } => image_url.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    non_empty(text)
}

fn mcp_result_text(result: &codex_app_server_protocol::McpToolCallResult) -> Option<String> {
    let text = result
        .content
        .iter()
        .filter_map(|value| {
            value
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| value.as_str().map(str::to_string))
        })
        .collect::<Vec<_>>()
        .join("\n");
    non_empty(text)
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}
