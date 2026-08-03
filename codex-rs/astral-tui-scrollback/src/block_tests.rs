use std::borrow::Cow;

use codex_app_server_protocol::ThreadItem;
use pretty_assertions::assert_eq;

use super::EntryBlock;
use super::ReasoningVisibility;
use crate::EntryLifecycle;
use crate::LiveItem;

#[test]
fn assistant_and_plan_streams_remain_separate_entries() {
    let assistant = ThreadItem::AgentMessage {
        id: "assistant".to_string(),
        text: "answer ".to_string(),
        phase: None,
        memory_citation: None,
    };
    let assistant_live = LiveItem::AgentMessage("tail".to_string());
    let plan = ThreadItem::Plan {
        id: "plan".to_string(),
        text: "# Plan\n".to_string(),
    };
    let plan_live = LiveItem::Plan("- step".to_string());

    assert_eq!(
        EntryBlock::from_parts(&assistant, &assistant_live, running()),
        EntryBlock::Assistant {
            markdown: Cow::Owned("answer tail".to_string()),
            running: true,
        }
    );
    assert_eq!(
        EntryBlock::from_parts(&plan, &plan_live, running()),
        EntryBlock::ProposedPlan {
            markdown: Cow::Owned("# Plan\n- step".to_string()),
            running: true,
        }
    );
}

#[test]
fn reasoning_visibility_never_creates_an_empty_viewer_body() {
    let reasoning = ThreadItem::Reasoning {
        id: "reasoning".to_string(),
        summary: vec!["summary".to_string()],
        content: vec![String::new()],
    };
    let live = LiveItem::Reasoning {
        summary: vec![" tail".to_string()],
        content: vec![String::new()],
    };
    let EntryBlock::Reasoning(block) = EntryBlock::from_parts(&reasoning, &live, running()) else {
        panic!("reasoning item must remain a reasoning block");
    };

    assert_eq!(
        block.summary(),
        &[Cow::<str>::Owned("summary tail".to_string())]
    );
    assert_eq!(
        block.visible_parts(ReasoningVisibility::Raw),
        block.summary()
    );
    assert_eq!(block.elapsed_ms(), None);
    assert!(block.has_visible_body(ReasoningVisibility::Summary));

    let opaque = ThreadItem::Reasoning {
        id: "opaque".to_string(),
        summary: vec![String::new()],
        content: Vec::new(),
    };
    let EntryBlock::Reasoning(opaque) =
        EntryBlock::from_parts(&opaque, &LiveItem::None, completed())
    else {
        panic!("reasoning item must remain a reasoning block");
    };
    assert!(!opaque.has_visible_body(ReasoningVisibility::Summary));
    assert!(!opaque.has_visible_body(ReasoningVisibility::Raw));
    assert_eq!(opaque.elapsed_ms(), Some(1));
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
