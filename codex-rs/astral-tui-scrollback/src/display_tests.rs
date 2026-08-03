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

fn running() -> EntryLifecycle {
    EntryLifecycle::Running { started_at_ms: 1 }
}

fn completed() -> EntryLifecycle {
    EntryLifecycle::Completed {
        started_at_ms: Some(1),
        completed_at_ms: 2,
    }
}
