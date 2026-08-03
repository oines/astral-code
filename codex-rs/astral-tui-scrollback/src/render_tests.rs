use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::UserInput;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;

use super::EntryRenderOptions;
use super::RenderedEntry;
use super::render_entry;
use crate::EntryBlock;
use crate::EntryDisplayState;
use crate::EntryLifecycle;
use crate::LiveItem;

fn plain(rendered: &RenderedEntry) -> String {
    rendered
        .lines()
        .iter()
        .map(|line| line.line.to_string().trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn conversation_entries_keep_distinct_grok_chrome_and_source_order() {
    let user_item = ThreadItem::UserMessage {
        id: "user".to_string(),
        client_id: None,
        content: vec![
            UserInput::Text {
                text: "请检查这个 **问题**。".to_string(),
                text_elements: Vec::new(),
            },
            UserInput::LocalImage {
                detail: None,
                path: "/tmp/example.png".into(),
            },
        ],
    };
    let user = EntryBlock::from_parts(&user_item, &LiveItem::None, EntryLifecycle::Restored);
    let user_state = EntryDisplayState::for_block(&user).expect("user display state");

    let assistant_item = ThreadItem::AgentMessage {
        id: "assistant".to_string(),
        text: "**结论**\n\n| 项目 | 状态 |\n|---|---|\n| 顺序 | 正常 |".to_string(),
        phase: None,
        memory_citation: None,
    };
    let assistant =
        EntryBlock::from_parts(&assistant_item, &LiveItem::None, completed(1_000, 1_500));
    let assistant_state = EntryDisplayState::for_block(&assistant).expect("assistant state");

    let reasoning_item = ThreadItem::Reasoning {
        id: "reasoning".to_string(),
        summary: vec![
            "Checked **source**.".to_string(),
            "Found issue.".to_string(),
        ],
        content: Vec::new(),
    };
    let reasoning =
        EntryBlock::from_parts(&reasoning_item, &LiveItem::None, completed(2_000, 4_000));
    let mut reasoning_state =
        EntryDisplayState::for_block(&reasoning).expect("reasoning display state");
    assert!(reasoning_state.expand(&reasoning));

    let plan_item = ThreadItem::Plan {
        id: "plan".to_string(),
        text: "# Plan\n\n- inspect\n- implement".to_string(),
    };
    let plan = EntryBlock::from_parts(&plan_item, &LiveItem::None, completed(5_000, 6_000));
    let plan_state = EntryDisplayState::for_block(&plan).expect("plan display state");

    let options = EntryRenderOptions::new(48);
    let output = [
        ("USER", &user, &user_state),
        ("ASSISTANT", &assistant, &assistant_state),
        ("REASONING", &reasoning, &reasoning_state),
        ("PLAN", &plan, &plan_state),
    ]
    .into_iter()
    .map(|(label, block, state)| {
        let rendered = render_entry(block, *state, options).expect("conversation renderer");
        format!("{label}\n{}", plain(&rendered))
    })
    .collect::<Vec<_>>()
    .join("\n\n");

    assert_snapshot!(output, @r###"
    USER
    › 请检查这个 **问题**。
      ↳ [Image #1]

    ASSISTANT
    结论

    ┌──────┬──────┐
    │ 项目 │ 状态 │
    ├──────┼──────┤
    │ 顺序 │ 正常 │
    └──────┴──────┘

    REASONING
    ◆ Thought for 2.0s

      Checked source.

      Found issue.

    PLAN
    • Proposed Plan

      Plan

      • inspect
      • implement
    "###);
}

#[test]
fn raw_assistant_shows_source_without_changing_the_entry() {
    let item = ThreadItem::AgentMessage {
        id: "assistant".to_string(),
        text: "**bold**".to_string(),
        phase: None,
        memory_citation: None,
    };
    let block = EntryBlock::from_parts(&item, &LiveItem::None, EntryLifecycle::Restored);
    let mut state = EntryDisplayState::for_block(&block).expect("assistant state");
    assert!(state.toggle_raw(&block));

    let rendered = render_entry(&block, state, EntryRenderOptions::new(40)).expect("rendered");
    assert_eq!(plain(&rendered), "**bold**");
}

#[test]
fn opaque_reasoning_never_opens_an_empty_body() {
    let item = ThreadItem::Reasoning {
        id: "reasoning".to_string(),
        summary: Vec::new(),
        content: Vec::new(),
    };
    let block = EntryBlock::from_parts(&item, &LiveItem::None, EntryLifecycle::Restored);
    let mut state = EntryDisplayState::for_block(&block).expect("reasoning state");
    assert!(state.expand(&block));

    let rendered = render_entry(&block, state, EntryRenderOptions::new(40)).expect("rendered");
    assert_eq!(plain(&rendered), "◆ Thought");
}

fn completed(started_at_ms: i64, completed_at_ms: i64) -> EntryLifecycle {
    EntryLifecycle::Completed {
        started_at_ms: Some(started_at_ms),
        completed_at_ms,
    }
}
