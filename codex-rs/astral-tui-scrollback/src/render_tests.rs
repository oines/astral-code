use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::CommandExecutionStatus;
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

#[test]
fn command_execution_uses_grok_folded_and_output_views() {
    let item = command(
        CommandExecutionSource::Agent,
        CommandExecutionStatus::Completed,
        "printf '\\e[32mone\\e[0m\\ntwo\\nthree\\nfour\\nfive'",
        /*output*/ Some("\u{1b}[32mone\u{1b}[0m\ntwo\nthree\nfour\nfive"),
        /*exit_code*/ Some(0),
        /*duration_ms*/ Some(1_250),
    );
    let block = EntryBlock::from_parts(
        &item,
        &LiveItem::None,
        completed(/*started_at_ms*/ 1_000, /*completed_at_ms*/ 2_250),
    );
    let mut state = EntryDisplayState::for_block(&block).expect("command state");
    let options = EntryRenderOptions::new(/*width*/ 52)
        .with_max_truncated_lines(/*max_truncated_lines*/ 3);

    let collapsed = render_entry(&block, state, options).expect("collapsed command");
    assert!(state.toggle_fold(&block));
    let truncated = render_entry(&block, state, options).expect("truncated command");
    assert!(state.expand(&block));
    let expanded = render_entry(&block, state, options).expect("expanded command");

    assert_snapshot!(format!(
        "COLLAPSED\n{}\n\nTRUNCATED\n{}\n\nEXPANDED\n{}",
        plain(&collapsed),
        plain(&truncated),
        plain(&expanded),
    ), @r###"
    COLLAPSED
    ◆ Run printf '\e[32mone\e[0m\ntwo\nthree\nfour\nfi …

    TRUNCATED
    ◆ Run printf '\e[32mone\e[0m\ntwo\nthree\nfour\nfive'  1.2s
      │ one
      │ two
      │ … 2 hidden lines
      │ five

    EXPANDED
    ◆ Run printf '\e[32mone\e[0m\ntwo\nthree\nfour\nfive'  1.2s
      │ one
      │ two
      │ three
      │ four
      │ five
    "###);
}

#[test]
fn running_user_shell_streams_live_output_and_terminal_input() {
    let item = command(
        CommandExecutionSource::UserShell,
        CommandExecutionStatus::InProgress,
        "python -i",
        /*output*/ None,
        /*exit_code*/ None,
        /*duration_ms*/ None,
    );
    let live = LiveItem::Command {
        output: ">>> 2\n".to_string(),
        terminal_input: vec!["1 + 1\n".to_string()],
    };
    let block = EntryBlock::from_parts(&item, &live, EntryLifecycle::Running { started_at_ms: 1 });
    let state = EntryDisplayState::for_block(&block).expect("user command state");
    let rendered =
        render_entry(&block, state, EntryRenderOptions::new(/*width*/ 40)).expect("rendered");

    assert_snapshot!(plain(&rendered), @r###"
    ◇ Run (user) python -i
      ↳ 1 + 1
      │ >>> 2
    "###);
}

fn command(
    source: CommandExecutionSource,
    status: CommandExecutionStatus,
    command: &str,
    output: Option<&str>,
    exit_code: Option<i32>,
    duration_ms: Option<i64>,
) -> ThreadItem {
    ThreadItem::CommandExecution {
        id: "command".to_string(),
        command: command.to_string(),
        cwd: std::path::PathBuf::from("/tmp")
            .try_into()
            .expect("absolute cwd"),
        process_id: None,
        source,
        status,
        command_actions: Vec::new(),
        aggregated_output: output.map(str::to_string),
        exit_code,
        duration_ms,
    }
}

fn completed(started_at_ms: i64, completed_at_ms: i64) -> EntryLifecycle {
    EntryLifecycle::Completed {
        started_at_ms: Some(started_at_ms),
        completed_at_ms,
    }
}
