use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::UserInput;
use pretty_assertions::assert_eq;

use super::DisplayMode;
use super::EntryDisplayState;
use crate::EntryBlock;
use crate::EntryLifecycle;
use crate::LiveItem;

#[test]
fn long_cjk_user_prompt_uses_grok_fold_cycle() {
    let content = vec![UserInput::Text {
        text: "这是一个用于验证中文宽字符折叠估算的长提示。".repeat(12),
        text_elements: Vec::new(),
    }];
    let block = EntryBlock::User { content: &content };
    let mut state = EntryDisplayState::for_block(&block).expect("user block has display policy");

    assert_eq!(state.mode(), DisplayMode::Collapsed);
    assert!(state.toggle_fold(&block));
    assert_eq!(state.mode(), DisplayMode::Expanded);
    assert!(state.mode_pinned());
    assert!(state.collapse(&block));
    assert_eq!(state.mode(), DisplayMode::Collapsed);
}

#[test]
fn completed_reasoning_collapses_unless_the_user_pinned_it() {
    let item = ThreadItem::Reasoning {
        id: "reasoning".to_string(),
        summary: vec!["summary".to_string()],
        content: Vec::new(),
    };
    let running = EntryBlock::from_parts(&item, &LiveItem::None, running());
    let mut state =
        EntryDisplayState::for_block(&running).expect("reasoning block has display policy");
    assert_eq!(state.mode(), DisplayMode::Truncated);

    let completed = EntryBlock::from_parts(&item, &LiveItem::None, completed());
    assert!(state.reconcile(&completed));
    assert_eq!(state.mode(), DisplayMode::Collapsed);

    let mut pinned =
        EntryDisplayState::for_block(&running).expect("reasoning block has display policy");
    assert!(pinned.toggle_fold(&running));
    assert_eq!(pinned.mode(), DisplayMode::Expanded);
    assert!(!pinned.reconcile(&completed));
    assert_eq!(pinned.mode(), DisplayMode::Expanded);
}

#[test]
fn proposed_plan_is_not_an_assistant_fold_or_running_preview() {
    let plan = EntryBlock::ProposedPlan {
        markdown: "# Plan".into(),
        running: true,
    };
    let mut state = EntryDisplayState::for_block(&plan).expect("plan has display policy");

    assert_eq!(state.mode(), DisplayMode::Expanded);
    assert!(!state.toggle_fold(&plan));
    assert!(state.toggle_raw(&plan));
    assert!(state.raw());
}

#[test]
fn commands_follow_grok_agent_and_user_shell_fold_cycles() {
    let agent_item = command(
        CommandExecutionSource::Agent,
        CommandExecutionStatus::Completed,
        /*output*/ Some("one\ntwo\nthree\nfour"),
    );
    let agent = EntryBlock::from_parts(&agent_item, &LiveItem::None, completed());
    let mut agent_state = EntryDisplayState::for_block(&agent).expect("agent command state");
    assert_eq!(agent_state.mode(), DisplayMode::Collapsed);
    assert!(agent_state.toggle_fold(&agent));
    assert_eq!(agent_state.mode(), DisplayMode::Truncated);

    let running_item = command(
        CommandExecutionSource::UserShell,
        CommandExecutionStatus::InProgress,
        /*output*/ None,
    );
    let live = LiveItem::Command {
        output: "streaming".to_string(),
        terminal_input: Vec::new(),
    };
    let running = EntryBlock::from_parts(&running_item, &live, running());
    let mut user_state = EntryDisplayState::for_block(&running).expect("user shell state");
    assert_eq!(user_state.mode(), DisplayMode::Truncated);

    let completed_item = command(
        CommandExecutionSource::UserShell,
        CommandExecutionStatus::Completed,
        /*output*/ Some("streaming"),
    );
    let completed = EntryBlock::from_parts(&completed_item, &LiveItem::None, completed());
    assert!(user_state.reconcile(&completed));
    assert_eq!(user_state.mode(), DisplayMode::Expanded);
    assert!(user_state.toggle_fold(&completed));
    assert_eq!(user_state.mode(), DisplayMode::Collapsed);
}

fn command(
    source: CommandExecutionSource,
    status: CommandExecutionStatus,
    output: Option<&str>,
) -> ThreadItem {
    ThreadItem::CommandExecution {
        id: "command".to_string(),
        command: "printf test".to_string(),
        cwd: std::path::PathBuf::from("/tmp")
            .try_into()
            .expect("absolute cwd"),
        process_id: None,
        source,
        status,
        command_actions: Vec::new(),
        aggregated_output: output.map(str::to_string),
        exit_code: Some(0),
        duration_ms: Some(250),
    }
}

fn running() -> EntryLifecycle {
    EntryLifecycle::Running { started_at_ms: 1 }
}

fn completed() -> EntryLifecycle {
    EntryLifecycle::Completed {
        started_at_ms: Some(1),
        completed_at_ms: 2,
    }
}
