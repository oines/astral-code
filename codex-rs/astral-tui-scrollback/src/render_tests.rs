use codex_app_server_protocol::CommandAction;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::CoreToolCallStatus;
use codex_app_server_protocol::DynamicToolCallStatus;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::McpToolCallError;
use codex_app_server_protocol::McpToolCallStatus;
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
use crate::SubagentAction;
use crate::SubagentPresentation;
use crate::TimelineStream;
use crate::ToolStatus;

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
            changes: vec![
                FileUpdateChange {
                    path: "astral-tui/src/render.rs".to_string(),
                    kind: PatchChangeKind::Update { move_path: None },
                    diff: "@@\n-old\n+new\n+another".to_string(),
                },
                FileUpdateChange {
                    path: "astral-tui/src/new.rs".to_string(),
                    kind: PatchChangeKind::Add,
                    diff: "@@\n+new module".to_string(),
                },
                FileUpdateChange {
                    path: "astral-tui/src/obsolete.rs".to_string(),
                    kind: PatchChangeKind::Delete,
                    diff: "@@\n-old module".to_string(),
                },
                FileUpdateChange {
                    path: "astral-tui/src/old_name.rs".to_string(),
                    kind: PatchChangeKind::Update {
                        move_path: Some(std::path::PathBuf::from("astral-tui/src/new_name.rs")),
                    },
                    diff: String::new(),
                },
            ],
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
fn todo_tool_surfaces_snapshot() {
    let items = [
        ThreadItem::CoreToolCall {
            id: "todo-claude".to_string(),
            tool: "TodoWrite".to_string(),
            arguments: json!({
                "explanation": "Keep the checklist current.",
                "todos": [
                    {"content": "Trace events", "status": "completed"},
                    {"content": "Render todos", "status": "in_progress"},
                    {"content": "Run PTY checks", "status": "pending"}
                ]
            }),
            status: CoreToolCallStatus::Completed,
            result: Some("Todos updated".to_string()),
            error: None,
            duration_ms: Some(4),
        },
        ThreadItem::CoreToolCall {
            id: "todo-codex".to_string(),
            tool: "update_plan".to_string(),
            arguments: json!({
                "plan": [
                    {"step": "Map Codex semantics", "status": "completed"},
                    {"step": "Verify snapshots", "status": "in_progress"}
                ]
            }),
            status: CoreToolCallStatus::Completed,
            result: Some("Plan updated".to_string()),
            error: None,
            duration_ms: Some(3),
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
fn background_task_action_tool_surfaces_snapshot() {
    let items = [
        ThreadItem::CoreToolCall {
            id: "task-read".to_string(),
            tool: "ReadTaskOutput".to_string(),
            arguments: json!({"task_id": "task-7"}),
            status: CoreToolCallStatus::Completed,
            result: Some("tests passed".to_string()),
            error: None,
            duration_ms: Some(12),
        },
        ThreadItem::CoreToolCall {
            id: "task-send".to_string(),
            tool: "SendTaskInput".to_string(),
            arguments: json!({"task_id": "task-7", "input": "continue\n"}),
            status: CoreToolCallStatus::Completed,
            result: Some("input sent".to_string()),
            error: None,
            duration_ms: Some(2),
        },
        ThreadItem::CoreToolCall {
            id: "task-list".to_string(),
            tool: "ListBackgroundTasks".to_string(),
            arguments: json!({}),
            status: CoreToolCallStatus::Completed,
            result: Some("task-7 running".to_string()),
            error: None,
            duration_ms: Some(1),
        },
        ThreadItem::CoreToolCall {
            id: "task-stop".to_string(),
            tool: "StopBackgroundTask".to_string(),
            arguments: json!({"task_id": "task-7"}),
            status: CoreToolCallStatus::Failed,
            result: None,
            error: Some("task already exited".to_string()),
            duration_ms: Some(2),
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
fn terminal_tool_states_snapshot() {
    let items = [
        ThreadItem::CommandExecution {
            id: "command-declined".to_string(),
            command: "cargo publish".to_string(),
            cwd: AbsolutePathBuf::try_from("/workspace".to_string()).expect("absolute path"),
            process_id: None,
            source: Default::default(),
            status: CommandExecutionStatus::Declined,
            command_actions: vec![CommandAction::Unknown {
                command: "cargo publish".to_string(),
            }],
            aggregated_output: None,
            exit_code: None,
            duration_ms: None,
        },
        ThreadItem::CoreToolCall {
            id: "bash-interrupted".to_string(),
            tool: "Bash".to_string(),
            arguments: json!({"command": "cargo test"}),
            status: CoreToolCallStatus::Interrupted,
            result: Some("stopped by user".to_string()),
            error: None,
            duration_ms: Some(510),
        },
        ThreadItem::DynamicToolCall {
            id: "dynamic-failed".to_string(),
            namespace: Some("workspace".to_string()),
            tool: "open_artifact".to_string(),
            arguments: json!({"path": "missing.html"}),
            status: DynamicToolCallStatus::Failed,
            content_items: None,
            success: Some(false),
            duration_ms: Some(12),
        },
        ThreadItem::McpToolCall {
            id: "mcp-failed".to_string(),
            server: "docs".to_string(),
            tool: "search".to_string(),
            status: McpToolCallStatus::Failed,
            arguments: json!({"query": "Astral TUI"}),
            mcp_app_resource_uri: None,
            plugin_id: None,
            result: None,
            error: Some(McpToolCallError {
                message: "server unavailable".to_string(),
            }),
            duration_ms: Some(1_200),
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
fn wrapping_preserves_cjk_without_inserting_spaces() {
    let block = PresentationBlock::Assistant {
        text: "你好，我是 Astral，可以帮你读写代码。".to_string(),
    };

    let rendered = render_block(
        &block,
        RenderOptions {
            width: 10,
            expanded: true,
            max_output_lines: 3,
        },
    )
    .lines
    .iter()
    .map(std::string::ToString::to_string)
    .collect::<Vec<_>>();

    assert_eq!(
        rendered,
        vec!["你好，我是", "Astral，可", "以帮你读写", "代码。"]
    );
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

#[test]
fn subagent_action_chrome_snapshot() {
    let blocks = [
        (
            SubagentAction::SendInput,
            ToolStatus::Running,
            "Check tests",
        ),
        (SubagentAction::Resume, ToolStatus::Success, ""),
        (SubagentAction::Wait, ToolStatus::Success, ""),
        (SubagentAction::Close, ToolStatus::Failed, ""),
    ]
    .map(|(action, status, prompt)| {
        PresentationBlock::Subagent(SubagentPresentation {
            action,
            status,
            thread_ids: vec!["agent-review".to_string()],
            prompt: (!prompt.is_empty()).then(|| prompt.to_string()),
            model: None,
            reasoning_effort: None,
            agents: Vec::new(),
        })
    });

    let rendered = blocks
        .iter()
        .map(|block| {
            render_block(block, RenderOptions::compact(68))
                .lines
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_snapshot!(rendered);
}
use std::collections::HashMap;

use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentStatus;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
