use std::fs;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use codex_core::CodexThread;
use codex_core::ForkSnapshot;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use codex_utils_path_uri::PathUri;
use core_test_support::responses::ResponsesRequest;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;

const INITIAL_AGENTS: &str = "WORLD_STATE_INITIAL_AGENTS";
const UPDATED_AGENTS: &str = "WORLD_STATE_UPDATED_AGENTS";
const REPLACEMENT_NOTICE: &str =
    "These AGENTS.md instructions replace all previously provided AGENTS.md instructions.";
const REMOVAL_NOTICE: &str = "The previously provided AGENTS.md instructions no longer apply.";

async fn submit_turn(thread: &Arc<CodexThread>, prompt: &str) -> Result<()> {
    thread
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: prompt.to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;
    wait_for_event(thread, |event| matches!(event, EventMsg::TurnComplete(_))).await;
    Ok(())
}

fn user_text_occurrences(request: &ResponsesRequest, needle: &str) -> usize {
    request
        .message_input_texts("user")
        .iter()
        .filter(|text| text.contains(needle))
        .count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agents_world_state_reconciles_resume_fork_and_file_deletion() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let response_mock = mount_sse_sequence(
        &server,
        (1..=4)
            .map(|index| {
                sse(vec![
                    ev_response_created(&format!("resp-{index}")),
                    ev_assistant_message(&format!("msg-{index}"), "done"),
                    ev_completed(&format!("resp-{index}")),
                ])
            })
            .collect(),
    )
    .await;
    let mut initial_builder = test_codex().with_workspace_setup(|cwd, filesystem| async move {
        let agents_md = PathUri::from_abs_path(&cwd.join("AGENTS.md"));
        filesystem
            .write_file(
                &agents_md,
                INITIAL_AGENTS.as_bytes().to_vec(),
                /*sandbox*/ None,
            )
            .await?;
        Ok::<(), anyhow::Error>(())
    });
    let initial = initial_builder.build(&server).await?;
    let cwd = initial.config.cwd.clone();
    let agents_path = cwd.join("AGENTS.md");
    let home = Arc::clone(&initial.home);

    initial.submit_turn("initial world-state turn").await?;
    initial.codex.ensure_rollout_materialized().await;
    initial.codex.flush_rollout().await?;
    let initial_rollout = initial
        .codex
        .rollout_path()
        .context("initial rollout path")?;
    initial.codex.shutdown_and_wait().await?;

    fs::write(&agents_path, UPDATED_AGENTS)?;
    let resume_cwd = cwd.clone();
    let mut resume_builder = test_codex().with_config(move |config| {
        config.cwd = resume_cwd;
    });
    let resumed = resume_builder
        .resume(&server, Arc::clone(&home), initial_rollout)
        .await?;
    resumed
        .submit_turn("turn after AGENTS.md replacement")
        .await?;
    resumed.codex.ensure_rollout_materialized().await;
    resumed.codex.flush_rollout().await?;
    let resumed_rollout = resumed
        .codex
        .rollout_path()
        .context("resumed rollout path")?;
    let thread_manager = Arc::clone(&resumed.thread_manager);
    let fork_config = resumed.config.clone();
    resumed.codex.shutdown_and_wait().await?;

    let forked = Box::pin(thread_manager.fork_thread(
        ForkSnapshot::Interrupted,
        fork_config.clone(),
        resumed_rollout,
        /*thread_source*/ None,
        /*parent_trace*/ None,
    ))
    .await?
    .thread;
    submit_turn(&forked, "turn after fork").await?;
    forked.ensure_rollout_materialized().await;
    forked.flush_rollout().await?;
    let forked_rollout = forked.rollout_path().context("forked rollout path")?;
    forked.shutdown_and_wait().await?;

    fs::remove_file(&agents_path)?;
    let deleted_cwd = cwd;
    let mut deleted_builder = test_codex().with_config(move |config| {
        config.cwd = deleted_cwd;
    });
    let deleted = deleted_builder
        .resume(&server, home, forked_rollout)
        .await?;
    deleted.submit_turn("turn after AGENTS.md deletion").await?;

    let requests = response_mock.requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(user_text_occurrences(&requests[0], INITIAL_AGENTS), 1);
    assert_eq!(user_text_occurrences(&requests[0], UPDATED_AGENTS), 0);

    assert_eq!(user_text_occurrences(&requests[1], INITIAL_AGENTS), 1);
    assert_eq!(user_text_occurrences(&requests[1], UPDATED_AGENTS), 1);
    assert_eq!(user_text_occurrences(&requests[1], REPLACEMENT_NOTICE), 1);

    assert_eq!(user_text_occurrences(&requests[2], UPDATED_AGENTS), 1);
    assert_eq!(user_text_occurrences(&requests[2], REPLACEMENT_NOTICE), 1);

    assert_eq!(user_text_occurrences(&requests[3], UPDATED_AGENTS), 1);
    assert_eq!(user_text_occurrences(&requests[3], REMOVAL_NOTICE), 1);

    Ok(())
}
