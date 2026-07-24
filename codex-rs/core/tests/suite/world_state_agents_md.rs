use std::fs;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use codex_core::CodexThread;
use codex_core::ForkSnapshot;
use codex_protocol::ThreadId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::TranscriptItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::RolloutLine;
use codex_protocol::protocol::SessionMeta;
use codex_protocol::protocol::SessionMetaLine;
use codex_protocol::user_input::UserInput;
use codex_utils_path_uri::PathUri;
use core_test_support::PathBufExt;
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
const PROJECT_AFTER_5_KIB_MARKER: &str = "PROJECT_INSTRUCTIONS_AFTER_5_KIB";

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
async fn agents_world_state_keeps_hot_session_snapshot_until_selection_changes() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let response_mock = mount_sse_sequence(
        &server,
        (1..=2)
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
    let mut builder = test_codex().with_workspace_setup(|cwd, filesystem| async move {
        filesystem
            .write_file(
                &PathUri::from_abs_path(&cwd.join("AGENTS.md")),
                INITIAL_AGENTS.as_bytes().to_vec(),
                /*sandbox*/ None,
            )
            .await?;
        Ok::<(), anyhow::Error>(())
    });
    let test = builder.build(&server).await?;

    test.submit_turn("initial hot-session turn").await?;
    fs::write(test.config.cwd.join("AGENTS.md"), UPDATED_AGENTS)?;
    test.submit_turn("unchanged environment selection").await?;

    let requests = response_mock.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(user_text_occurrences(&requests[1], INITIAL_AGENTS), 1);
    assert_eq!(user_text_occurrences(&requests[1], UPDATED_AGENTS), 0);
    assert_eq!(user_text_occurrences(&requests[1], REPLACEMENT_NOTICE), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agents_world_state_respects_configured_project_doc_budget() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let response_mock = mount_sse_sequence(
        &server,
        vec![sse(vec![
            ev_response_created("resp-1"),
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-1"),
        ])],
    )
    .await;
    let global = "GLOBAL_AGENTS_PREFIX\n".to_string();
    let project = format!("{}\n{PROJECT_AFTER_5_KIB_MARKER}", "p".repeat(6 * 1024));
    let mut builder = test_codex()
        .with_pre_build_hook(move |home| {
            fs::write(home.join("AGENTS.md"), global).expect("write global AGENTS.md");
        })
        .with_workspace_setup(move |cwd, filesystem| async move {
            filesystem
                .write_file(
                    &PathUri::from_abs_path(&cwd.join("AGENTS.md")),
                    project.into_bytes(),
                    /*sandbox*/ None,
                )
                .await?;
            Ok::<(), anyhow::Error>(())
        })
        .with_config(|config| {
            config.project_doc_max_bytes = 8 * 1024;
        });
    let test = builder.build(&server).await?;

    test.submit_turn("verify configured AGENTS.md budget")
        .await?;

    let request = response_mock.single_request();
    let agents_fragment = request
        .message_input_texts("user")
        .into_iter()
        .find(|text| text.starts_with("# AGENTS.md instructions"))
        .context("model request should contain AGENTS.md instructions")?;
    assert!(agents_fragment.contains("GLOBAL_AGENTS_PREFIX"));
    let marker_offset = agents_fragment
        .find(PROJECT_AFTER_5_KIB_MARKER)
        .context("project instructions beyond the former WorldState cap should reach the model")?;
    assert!(marker_offset > 5 * 1024);
    assert!(!agents_fragment.contains("additional world-state content truncated"));
    Ok(())
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_agents_world_state_reconciles_once_across_resume_and_fork() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let response_mock = mount_sse_sequence(
        &server,
        (1..=3)
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
    let codex_home = Arc::new(tempfile::tempdir()?);
    let cwd_dir = tempfile::tempdir()?;
    let cwd = cwd_dir.path().to_path_buf().abs();
    fs::write(cwd.join("AGENTS.md"), UPDATED_AGENTS)?;
    let legacy_fragment = format!(
        "# AGENTS.md instructions for {}\n\n<INSTRUCTIONS>\n{INITIAL_AGENTS}\n</INSTRUCTIONS>",
        cwd.as_path().display()
    );
    let rollout = [
        RolloutLine {
            timestamp: "2024-01-01T00:00:00.000Z".to_string(),
            item: RolloutItem::SessionMeta(SessionMetaLine {
                meta: SessionMeta {
                    id: ThreadId::default(),
                    timestamp: "2024-01-01T00:00:00Z".to_string(),
                    cwd: cwd.to_path_buf(),
                    originator: "test_originator".to_string(),
                    cli_version: "test_version".to_string(),
                    ..Default::default()
                },
                git: None,
            }),
        },
        RolloutLine {
            timestamp: "2024-01-01T00:00:01.000Z".to_string(),
            item: RolloutItem::TranscriptItem(TranscriptItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: legacy_fragment,
                }],
                phase: None,
            }),
        },
    ];
    let legacy_rollout_path = codex_home.path().join("legacy-agents-rollout.jsonl");
    let serialized_rollout = rollout
        .iter()
        .map(serde_json::to_string)
        .collect::<serde_json::Result<Vec<_>>>()?
        .join("\n");
    fs::write(&legacy_rollout_path, format!("{serialized_rollout}\n"))?;

    let resume_cwd = cwd.clone();
    let mut resume_builder = test_codex().with_config(move |config| {
        config.cwd = resume_cwd;
    });
    let resumed = resume_builder
        .resume(&server, Arc::clone(&codex_home), legacy_rollout_path)
        .await?;
    resumed.submit_turn("reconcile legacy AGENTS.md").await?;
    resumed
        .submit_turn("unchanged reconciled AGENTS.md")
        .await?;
    resumed.codex.ensure_rollout_materialized().await;
    resumed.codex.flush_rollout().await?;
    let resumed_rollout = resumed
        .codex
        .rollout_path()
        .context("resumed rollout path")?;
    let persisted_world_state_count = fs::read_to_string(&resumed_rollout)?
        .lines()
        .map(serde_json::from_str::<RolloutLine>)
        .collect::<serde_json::Result<Vec<_>>>()?
        .iter()
        .filter(|line| matches!(line.item, RolloutItem::WorldState(_)))
        .count();
    assert_eq!(persisted_world_state_count, 1);

    let thread_manager = Arc::clone(&resumed.thread_manager);
    let fork_config = resumed.config.clone();
    resumed.codex.shutdown_and_wait().await?;
    let forked = Box::pin(thread_manager.fork_thread(
        ForkSnapshot::Interrupted,
        fork_config,
        resumed_rollout,
        /*thread_source*/ None,
        /*parent_trace*/ None,
    ))
    .await?
    .thread;
    submit_turn(&forked, "turn after legacy reconciliation fork").await?;

    let requests = response_mock.requests();
    assert_eq!(requests.len(), 3);
    for request in &requests {
        assert_eq!(user_text_occurrences(request, INITIAL_AGENTS), 1);
        assert_eq!(user_text_occurrences(request, UPDATED_AGENTS), 1);
        assert_eq!(user_text_occurrences(request, REPLACEMENT_NOTICE), 1);
    }
    Ok(())
}
