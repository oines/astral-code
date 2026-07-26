use codex_app_server_protocol::CommandAction;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::CoreToolCallStatus;
use codex_app_server_protocol::DynamicToolCallStatus;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::PatchChangeKind;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::UserInput;
use codex_utils_absolute_path::AbsolutePathBuf;
use insta::assert_snapshot;
use serde_json::json;

use super::RenderOptions;
use super::render_block;
use crate::PresentationBlock;
use crate::TimelineStream;

fn render(item: ThreadItem, expanded: bool) -> String {
    let block = PresentationBlock::from_item(&item, &crate::TimelineStream::None)
        .expect("fixture should produce a presentation block");
    render_block(
        &block,
        RenderOptions {
            width: 68,
            expanded,
            max_output_lines: 3,
        },
    )
    .lines
    .iter()
    .map(std::string::ToString::to_string)
    .collect::<Vec<_>>()
    .join("\n")
}

#[test]
fn codex_surface_tool_blocks_snapshot() {
    let items = [
        ThreadItem::CommandExecution {
            id: "command-1".to_string(),
            command: "just test -p astral-tui".to_string(),
            cwd: AbsolutePathBuf::try_from("/workspace".to_string()).expect("absolute path"),
            process_id: None,
            source: Default::default(),
            status: CommandExecutionStatus::Completed,
            command_actions: vec![CommandAction::Unknown {
                command: "just test -p astral-tui".to_string(),
            }],
            aggregated_output: Some(
                "PASS timeline reducer\nPASS presentation\nPASS renderer\nPASS app shell"
                    .to_string(),
            ),
            exit_code: Some(0),
            duration_ms: Some(2_440),
        },
        ThreadItem::FileChange {
            id: "patch-1".to_string(),
            changes: vec![FileUpdateChange {
                path: "astral-tui/src/render.rs".to_string(),
                kind: PatchChangeKind::Update { move_path: None },
                diff: "@@\n-old\n+new\n+another".to_string(),
            }],
            status: PatchApplyStatus::Completed,
        },
        ThreadItem::WebSearch {
            id: "search-1".to_string(),
            query: "Ratatui inline viewport".to_string(),
            action: None,
        },
    ];

    let rendered = items
        .into_iter()
        .map(|item| render(item, true))
        .collect::<Vec<_>>()
        .join("\n");
    assert_snapshot!(rendered);
}

#[test]
fn claude_surface_tool_blocks_snapshot() {
    let items = [
        ThreadItem::CoreToolCall {
            id: "read-1".to_string(),
            tool: "Read".to_string(),
            arguments: json!({"file_path": "/workspace/src/main.rs"}),
            status: CoreToolCallStatus::Completed,
            result: Some("fn main() {}".to_string()),
            error: None,
            duration_ms: Some(16),
        },
        ThreadItem::CoreToolCall {
            id: "edit-1".to_string(),
            tool: "Edit".to_string(),
            arguments: json!({"file_path": "/workspace/src/main.rs"}),
            status: CoreToolCallStatus::Failed,
            result: None,
            error: Some("old_string was not unique".to_string()),
            duration_ms: Some(22),
        },
        ThreadItem::CoreToolCall {
            id: "bash-1".to_string(),
            tool: "Bash".to_string(),
            arguments: json!({"command": "cargo check"}),
            status: CoreToolCallStatus::InProgress,
            result: Some("Checking astral-tui".to_string()),
            error: None,
            duration_ms: None,
        },
        ThreadItem::DynamicToolCall {
            id: "glob-1".to_string(),
            namespace: Some("claude".to_string()),
            tool: "Glob".to_string(),
            arguments: json!({"pattern": "**/*.rs"}),
            status: DynamicToolCallStatus::Completed,
            content_items: None,
            success: Some(true),
            duration_ms: Some(8),
        },
    ];

    let rendered = items
        .into_iter()
        .map(|item| render(item, false))
        .collect::<Vec<_>>()
        .join("\n");
    assert_snapshot!(rendered);
}

#[test]
fn background_command_lifecycle_snapshot() {
    let item = ThreadItem::CommandExecution {
        id: "command-bg".to_string(),
        command: "just test -p codex-cli".to_string(),
        cwd: AbsolutePathBuf::try_from("/workspace".to_string()).expect("absolute path"),
        process_id: Some("process-7".to_string()),
        source: Default::default(),
        status: CommandExecutionStatus::InProgress,
        command_actions: vec![CommandAction::Unknown {
            command: "just test -p codex-cli".to_string(),
        }],
        aggregated_output: None,
        exit_code: None,
        duration_ms: None,
    };
    let mut stream = TimelineStream::default();
    stream.append_command_output("Compiling astral-tui\n");
    stream.append_terminal_input("process-7", "continue\n");
    let block = PresentationBlock::from_item(&item, &stream)
        .expect("command should produce a presentation block");
    let rendered = render_block(
        &block,
        RenderOptions {
            width: 68,
            expanded: true,
            max_output_lines: 3,
        },
    )
    .lines
    .iter()
    .map(std::string::ToString::to_string)
    .collect::<Vec<_>>()
    .join("\n");

    assert_snapshot!(rendered);
}

#[test]
fn web_and_image_tool_blocks_snapshot() {
    let items = [
        ThreadItem::WebSearch {
            id: "search-1".to_string(),
            query: "Ratatui inline viewport".to_string(),
            action: None,
        },
        ThreadItem::DynamicToolCall {
            id: "fetch-1".to_string(),
            namespace: Some("web".to_string()),
            tool: "WebFetch".to_string(),
            arguments: json!({"url": "https://ratatui.rs"}),
            status: DynamicToolCallStatus::Completed,
            content_items: None,
            success: Some(true),
            duration_ms: Some(82),
        },
        ThreadItem::ImageView {
            id: "image-view-1".to_string(),
            path: AbsolutePathBuf::try_from("/workspace/diagram.png".to_string())
                .expect("absolute path"),
        },
        ThreadItem::ImageGeneration {
            id: "image-generation-1".to_string(),
            status: "completed".to_string(),
            revised_prompt: Some("A terminal interface under a night sky".to_string()),
            result: "image-data".to_string(),
            saved_path: Some(
                AbsolutePathBuf::try_from("/workspace/astral.png".to_string())
                    .expect("absolute path"),
            ),
        },
    ];

    let rendered = items
        .into_iter()
        .map(|item| render(item, true))
        .collect::<Vec<_>>()
        .join("\n");
    assert_snapshot!(rendered);
}

#[test]
fn conversation_blocks_snapshot() {
    let items = [
        ThreadItem::UserMessage {
            id: "user-1".to_string(),
            client_id: None,
            content: vec![
                UserInput::Text {
                    text: "把两个工具面统一渲染到 Astral TUI".to_string(),
                    text_elements: Vec::new(),
                },
                UserInput::Mention {
                    name: "render.rs".to_string(),
                    path: "astral-tui/src/render.rs".to_string(),
                },
            ],
        },
        ThreadItem::Reasoning {
            id: "reasoning-1".to_string(),
            summary: vec!["先把协议事件归约成稳定 UI 语义。".to_string()],
            content: Vec::new(),
        },
        ThreadItem::AgentMessage {
            id: "agent-1".to_string(),
            text: "已经统一：渲染器不再识别 Claude 或 Codex 的原始工具名。".to_string(),
            phase: None,
            memory_citation: None,
        },
    ];

    let rendered = items
        .into_iter()
        .map(|item| render(item, true))
        .collect::<Vec<_>>()
        .join("\n\n");
    assert_snapshot!(rendered);
}

#[test]
fn subagent_lifecycle_blocks_snapshot() {
    let items = [
        ThreadItem::CollabAgentToolCall {
            id: "spawn-1".to_string(),
            tool: CollabAgentTool::SpawnAgent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: "root".to_string(),
            receiver_thread_ids: vec!["agent-research".to_string()],
            prompt: Some("Inspect the Grok subagent rendering semantics".to_string()),
            model: Some("grok-code-fast".to_string()),
            reasoning_effort: None,
            agents_states: HashMap::from([(
                "agent-research".to_string(),
                CollabAgentState {
                    status: CollabAgentStatus::Running,
                    message: None,
                },
            )]),
        },
        ThreadItem::CollabAgentToolCall {
            id: "wait-1".to_string(),
            tool: CollabAgentTool::Wait,
            status: CollabAgentToolCallStatus::Failed,
            sender_thread_id: "root".to_string(),
            receiver_thread_ids: vec!["agent-research".to_string(), "agent-tests".to_string()],
            prompt: None,
            model: None,
            reasoning_effort: None,
            agents_states: HashMap::from([
                (
                    "agent-research".to_string(),
                    CollabAgentState {
                        status: CollabAgentStatus::Completed,
                        message: Some("Mapped spawn, wait, resume and close.".to_string()),
                    },
                ),
                (
                    "agent-tests".to_string(),
                    CollabAgentState {
                        status: CollabAgentStatus::Errored,
                        message: Some("snapshot mismatch".to_string()),
                    },
                ),
            ]),
        },
    ];

    let rendered = items
        .into_iter()
        .map(|item| render(item, false))
        .collect::<Vec<_>>()
        .join("\n");
    assert_snapshot!(rendered);
}
use std::collections::HashMap;

use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentStatus;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
