use super::*;
use codex_protocol::models::ContentItem;
use codex_protocol::models::DEFAULT_IMAGE_DETAIL;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::openai_models::InputModality;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::approx_token_count;
use pretty_assertions::assert_eq;

fn test_tool_context() -> SessionMemoryToolContext {
    SessionMemoryToolContext {
        surface: crate::config::ToolSurface::Claude,
        mode: codex_protocol::openai_models::ToolMode::Direct,
        code_mode_tool_definitions: Vec::new(),
    }
}

fn user_message(text: &str) -> TranscriptItem {
    TranscriptItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
    }
}

fn developer_message(text: &str) -> TranscriptItem {
    TranscriptItem::Message {
        id: None,
        role: "developer".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
    }
}

fn user_image_message() -> TranscriptItem {
    TranscriptItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputImage {
            image_url: "https://example.com/session-memory.png".to_string(),
            detail: Some(DEFAULT_IMAGE_DETAIL),
        }],
        phase: None,
    }
}

fn function_call(call_id: &str) -> TranscriptItem {
    TranscriptItem::FunctionCall {
        id: None,
        name: "shell_command".to_string(),
        namespace: None,
        arguments: "{}".to_string(),
        call_id: call_id.to_string(),
    }
}

fn function_output(call_id: &str) -> TranscriptItem {
    TranscriptItem::FunctionCallOutput {
        call_id: call_id.to_string(),
        output: FunctionCallOutputPayload::from_text("ok".to_string()),
    }
}

fn state_for_boundary(items: &[TranscriptItem], index: usize) -> SessionMemoryState {
    SessionMemoryState {
        last_summary_index: Some(index),
        last_summary_fingerprint: Some(tail::item_fingerprint(&items[index])),
        ..Default::default()
    }
}

fn summary_with_current_state(current_state: &str) -> String {
    tail::DEFAULT_SUMMARY.replace(
        "_What is actively being worked on right now? Pending tasks not yet completed. Immediate next steps._",
        &format!(
            "_What is actively being worked on right now? Pending tasks not yet completed. Immediate next steps._\n{current_state}"
        ),
    )
}

#[test]
fn raw_tail_keeps_function_call_pair_after_boundary() {
    let items = vec![
        user_message("before"),
        function_call("call-1"),
        function_output("call-1"),
    ];
    let state = state_for_boundary(&items, 0);

    let tail = raw_tail_after_summary_boundary(&items, &state).expect("tail is valid");

    assert_eq!(tail, items);
}

#[test]
fn raw_tail_filters_reinjectable_context_when_expanding_before_boundary() {
    let contextual_user = user_message("<environment_context>ctx</environment_context>");
    let contextual_developer = developer_message("<permissions instructions>ctx");
    let real_user = user_message("real user message");
    let items = vec![
        contextual_user.clone(),
        contextual_developer.clone(),
        real_user.clone(),
        function_call("call-1"),
        function_output("call-1"),
    ];
    let state = state_for_boundary(&items, 4);

    let tail = raw_tail_after_summary_boundary(&items, &state).expect("tail is valid");

    assert_eq!(
        tail,
        vec![
            real_user,
            function_call("call-1"),
            function_output("call-1")
        ]
    );
    assert!(!tail.contains(&contextual_user));
    assert!(!tail.contains(&contextual_developer));
}

#[test]
fn raw_tail_rejects_orphan_function_output_after_boundary() {
    let items = vec![user_message("before"), function_output("call-1")];
    let state = state_for_boundary(&items, 0);

    let err = raw_tail_after_summary_boundary(&items, &state).expect_err("tail is invalid");

    assert!(
        err.to_string()
            .contains("session memory raw tail would split a tool call pair")
    );
}

#[test]
fn raw_tail_rejects_boundary_fingerprint_mismatch() {
    let items = vec![user_message("before"), user_message("after")];
    let state = SessionMemoryState {
        last_summary_index: Some(0),
        last_summary_fingerprint: Some("not-the-fingerprint".to_string()),
        ..Default::default()
    };

    let err = raw_tail_after_summary_boundary(&items, &state).expect_err("tail is invalid");

    assert!(
        err.to_string()
            .contains("session memory boundary fingerprint mismatch")
    );
}

#[test]
fn raw_tail_without_boundary_keeps_recent_text_messages() {
    let items = (0..8)
        .map(|index| user_message(&format!("message {index}")))
        .collect::<Vec<_>>();
    let state = SessionMemoryState::default();

    let tail = raw_tail_after_summary_boundary(&items, &state).expect("tail is valid");

    assert_eq!(tail, items);
}

#[test]
fn raw_tail_expands_before_boundary_to_keep_context() {
    let items = (0..6)
        .map(|index| user_message(&format!("message {index}")))
        .collect::<Vec<_>>();
    let state = state_for_boundary(&items, 3);

    let tail = raw_tail_after_summary_boundary(&items, &state).expect("tail is valid");

    assert_eq!(tail, items);
}

#[test]
fn compact_summary_truncates_overlarge_sections() {
    let summary = summary_with_current_state(&"token ".repeat(5_000));

    validate_summary(&summary, tail::DEFAULT_SUMMARY).expect("summary can still be extracted");
    let (truncated, was_truncated) = truncate_summary_for_compact(&summary);

    assert!(was_truncated);
    assert!(truncated.contains("[... section truncated for length ...]"));
}

#[test]
fn compact_summary_does_not_apply_total_body_cap() {
    let sections = (0..10)
        .map(|index| format!("# Section {index}\n{}\n", "x ".repeat(3_000)))
        .collect::<String>();
    let summary = format!("{sections}\n# Final Section\nFINAL_SESSION_MEMORY_MARKER");

    let (truncated, was_truncated) = truncate_summary_for_compact(&summary);

    assert!(!was_truncated);
    assert!(approx_token_count(&truncated) > 9_500);
    assert!(truncated.contains("FINAL_SESSION_MEMORY_MARKER"));
}

#[test]
fn claude_code_golden_raw_tail_rejects_more_than_40k_tokens() {
    let tail = vec![user_message(&"token ".repeat(45_000))];

    let err = validate_tail_budget(&tail).expect_err("tail should exceed compact budget");

    assert!(
        err.to_string()
            .contains("session memory raw tail exceeds 40000 tokens")
    );
}

#[test]
fn compact_summary_validation_allows_previous_template_headings() {
    let current_template = "# IM State\n_Current chat handoff_\n\n# Follow-ups\n_Open items_";
    let summary = summary_with_current_state("- Old template content.");

    validate_summary(&summary, current_template)
        .expect("compact should not require current template headings");
}

#[test]
fn session_memory_compacted_history_places_summary_before_tail() {
    let tail = vec![user_message("recent user"), user_message("latest user")];

    let history = build_session_memory_compacted_history(tail.clone(), "summary".to_string());

    let mut expected = vec![TranscriptItem::Compaction {
        encrypted_content: "summary".to_string(),
    }];
    expected.extend(tail);
    assert_eq!(history, expected);
}

#[test]
fn session_memory_compacted_history_allows_empty_tail() {
    let history = build_session_memory_compacted_history(Vec::new(), "summary".to_string());

    let expected = vec![TranscriptItem::Compaction {
        encrypted_content: "summary".to_string(),
    }];
    assert_eq!(history, expected);
}

#[test]
fn extraction_candidate_records_raw_boundary_for_normalized_image_history() {
    let image_message = user_image_message();
    let items = vec![user_message("before image"), image_message.clone()];
    let mut history = crate::context_manager::ContextManager::new();
    history.record_items(items.iter(), TruncationPolicy::Tokens(10_000));

    let candidate = ExtractionCandidate::from_history(
        PromptTemplate {
            prompt: Prompt::default(),
            tool_context: test_tool_context(),
        },
        history,
        &[InputModality::Text],
        20_000,
        true,
    );
    let boundary = candidate.raw_boundary.expect("raw boundary is recorded");
    let normalized_boundary_item = candidate
        .prompt
        .input
        .get(boundary.index)
        .expect("normalized boundary item");

    assert_ne!(
        tail::item_fingerprint(normalized_boundary_item),
        tail::item_fingerprint(&image_message)
    );
    assert_eq!(boundary.index, 1);
    assert_eq!(boundary.fingerprint, tail::item_fingerprint(&image_message));

    let state = SessionMemoryState {
        last_summary_index: Some(boundary.index),
        last_summary_fingerprint: Some(boundary.fingerprint),
        ..Default::default()
    };
    let tail = raw_tail_after_summary_boundary(&items, &state).expect("raw boundary matches");
    assert_eq!(tail, items);
}

#[test]
fn compact_baseline_resets_to_post_compact_tokens() {
    let mut state = SessionMemoryState {
        last_summary_index: Some(7),
        last_summary_fingerprint: Some("fingerprint".to_string()),
        last_summary_tokens: Some(180_000),
        last_summary_tool_calls: Some(40),
        ..Default::default()
    };

    state.record_post_compact_baseline(20_000, 2);

    assert_eq!(state.last_summary_index, None);
    assert_eq!(state.last_summary_fingerprint, None);
    assert_eq!(state.last_summary_tokens, Some(20_000));
    assert_eq!(state.last_summary_tool_calls, Some(2));

    let candidate = ExtractionCandidate {
        prompt: Prompt::default(),
        tool_context: test_tool_context(),
        raw_boundary: None,
        active_context_tokens: 25_000,
        natural_break: true,
    };
    assert!(should_extract(&state, &candidate, test_thresholds()));
}

#[test]
fn legacy_auto_compact_breaker_state_is_ignored() {
    let state: SessionMemoryState = serde_json::from_value(serde_json::json!({
        "last_summary_tokens": 42_000,
        "consecutive_auto_compact_failures": 99
    }))
    .expect("legacy state should deserialize");

    let expected = SessionMemoryState {
        last_summary_tokens: Some(42_000),
        ..Default::default()
    };
    assert_eq!(state, expected);

    let serialized = serde_json::to_value(&state).expect("serialize state");
    assert!(
        serialized
            .get("consecutive_auto_compact_failures")
            .is_none()
    );
}

fn test_thresholds() -> ExtractionThresholds {
    ExtractionThresholds {
        minimum_message_tokens_to_init: 10_000,
        minimum_tokens_between_update: 5_000,
        tool_calls_between_updates: 3,
    }
}

#[tokio::test]
async fn ensure_preserves_existing_summary_when_template_changes() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = SessionMemoryStore {
        thread_key: "thread".to_string(),
        dir: temp.path().to_path_buf(),
        summary_path: temp.path().join("summary.md"),
        state_path: temp.path().join("state.json"),
    };
    let old_template = tail::DEFAULT_SUMMARY;
    let new_template = "# IM State\n_Current chat handoff_\n\n# Follow-ups\n_Open items_";
    store.ensure(old_template).await.expect("initialize store");
    let existing_summary = summary_with_current_state("- Old template content.");
    tokio::fs::write(&store.summary_path, existing_summary.as_bytes())
        .await
        .expect("write old summary");
    let mut state = store.read_state().await.expect("read state");
    state.last_summary_tokens = Some(42_000);
    store.write_state(&state).await.expect("write state");

    store
        .ensure(new_template)
        .await
        .expect("template change should not reset store");

    let summary = store.read_summary().await.expect("read summary");
    assert_eq!(summary, existing_summary);
    let state = store.read_state().await.expect("read preserved state");
    assert_eq!(state.last_summary_tokens, Some(42_000));
}

#[tokio::test]
async fn write_state_persists_via_atomic_tempfile_without_leaving_temps() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = SessionMemoryStore {
        thread_key: "thread".to_string(),
        dir: temp.path().to_path_buf(),
        summary_path: temp.path().join("summary.md"),
        state_path: temp.path().join("state.json"),
    };
    store
        .ensure(tail::DEFAULT_SUMMARY)
        .await
        .expect("initialize store");
    let state = SessionMemoryState {
        last_summary_tokens: Some(42_000),
        last_summary_tool_calls: Some(7),
        ..Default::default()
    };

    store.write_state(&state).await.expect("write state");

    assert_eq!(store.read_state().await.expect("read state"), state);
    let mut file_names = std::fs::read_dir(temp.path())
        .expect("read session memory dir")
        .map(|entry| {
            entry
                .expect("read dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    file_names.sort();
    assert_eq!(
        file_names,
        vec!["state.json".to_string(), "summary.md".to_string()]
    );
}

#[tokio::test]
async fn wait_for_extraction_completion_observes_finished_state() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = SessionMemoryStore {
        thread_key: "thread".to_string(),
        dir: temp.path().to_path_buf(),
        summary_path: temp.path().join("summary.md"),
        state_path: temp.path().join("state.json"),
    };
    store
        .ensure(tail::DEFAULT_SUMMARY)
        .await
        .expect("initialize store");
    let state = SessionMemoryState {
        extraction_started_at_unix: None,
        last_summary_tokens: Some(12_000),
        ..Default::default()
    };
    store.write_state(&state).await.expect("write state");

    wait_for_extraction_completion(&store, Duration::from_millis(50))
        .await
        .expect("finished extraction should not block");
}

#[tokio::test]
async fn wait_for_extraction_completion_clears_started_state_on_timeout() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = SessionMemoryStore {
        thread_key: "thread".to_string(),
        dir: temp.path().to_path_buf(),
        summary_path: temp.path().join("summary.md"),
        state_path: temp.path().join("state.json"),
    };
    store
        .ensure(tail::DEFAULT_SUMMARY)
        .await
        .expect("initialize store");
    let state = SessionMemoryState {
        extraction_started_at_unix: Some(now_unix_seconds()),
        ..Default::default()
    };
    store.write_state(&state).await.expect("write state");

    let err = wait_for_extraction_completion(&store, Duration::from_millis(1))
        .await
        .expect_err("unfinished extraction should time out");

    assert!(
        err.to_string()
            .contains("session memory extraction did not finish before shutdown timeout")
    );
    let state = store.read_state().await.expect("read state");
    let expected = SessionMemoryState {
        extraction_started_at_unix: None,
        last_error: Some("session memory extraction interrupted during shutdown".to_string()),
        ..Default::default()
    };
    assert_eq!(state, expected);
}

#[tokio::test]
async fn wait_for_running_extraction_timeout_continues_and_clears_marker() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = SessionMemoryStore {
        thread_key: "thread".to_string(),
        dir: temp.path().to_path_buf(),
        summary_path: temp.path().join("summary.md"),
        state_path: temp.path().join("state.json"),
    };
    store
        .ensure(tail::DEFAULT_SUMMARY)
        .await
        .expect("initialize store");
    let original = SessionMemoryState {
        extraction_started_at_unix: Some(now_unix_seconds()),
        ..Default::default()
    };
    store.write_state(&original).await.expect("write state");
    let mut state = original;

    wait_for_running_extraction_with_timeout(&store, &mut state, Duration::from_millis(1))
        .await
        .expect("compact wait timeout should continue");

    let expected = SessionMemoryState {
        extraction_started_at_unix: None,
        last_error: Some(
            "session memory extraction did not finish before compact timeout".to_string(),
        ),
        ..Default::default()
    };
    assert_eq!(state, expected);
    let stored = store.read_state().await.expect("read state");
    assert_eq!(stored, expected);
}

#[test]
fn prompt_variable_substitution_is_single_pass() {
    let substituted = super::sidechain::substitute_prompt_variables(
        "notes={{currentNotes}}\npath={{notesPath}}\nunknown={{missing}}",
        "/tmp/summary.md",
        "literal {{notesPath}}",
    );

    assert_eq!(
        substituted,
        "notes=literal {{notesPath}}\npath=/tmp/summary.md\nunknown={{missing}}"
    );
}

#[test]
fn updater_prompt_includes_full_current_notes_for_condensation() {
    let current_notes = format!(
        "start\n{}\nFINAL_SESSION_MEMORY_MARKER",
        "token ".repeat(20_000)
    );
    let prompt = super::sidechain::updater_prompt(
        None,
        super::sidechain::SummaryUpdateTool::Edit,
        std::path::Path::new("/tmp/summary.md"),
        &current_notes,
        "",
    );

    assert!(prompt.contains("FINAL_SESSION_MEMORY_MARKER"));
    assert!(!prompt.contains("IMPORTANT PRE-READ NOTE"));
}

#[test]
fn custom_updater_prompt_matches_custom_template_with_substitution() {
    let prompt = super::sidechain::updater_prompt(
        Some("CUSTOM\n{{currentNotes}}\n{{notesPath}}"),
        super::sidechain::SummaryUpdateTool::ApplyPatch,
        std::path::Path::new("/tmp/summary.md"),
        "current notes",
        "",
    );

    assert_eq!(prompt, "CUSTOM\ncurrent notes\n/tmp/summary.md");
}

#[test]
fn should_extract_requires_initial_token_threshold() {
    let state = SessionMemoryState::default();
    let prompt = Prompt {
        input: vec![user_message("small")],
        ..Default::default()
    };
    let candidate = ExtractionCandidate {
        prompt,
        tool_context: test_tool_context(),
        raw_boundary: None,
        active_context_tokens: test_thresholds().minimum_message_tokens_to_init - 1,
        natural_break: true,
    };

    assert!(!should_extract(&state, &candidate, test_thresholds()));
}

#[test]
fn should_not_extract_on_natural_break_before_token_threshold() {
    let state = SessionMemoryState {
        last_summary_tokens: Some(test_thresholds().minimum_message_tokens_to_init),
        ..Default::default()
    };
    let candidate = ExtractionCandidate {
        prompt: Prompt::default(),
        tool_context: test_tool_context(),
        raw_boundary: None,
        active_context_tokens: test_thresholds().minimum_message_tokens_to_init + 1,
        natural_break: true,
    };

    assert!(!should_extract(&state, &candidate, test_thresholds()));
}

#[test]
fn should_extract_on_natural_break_after_token_threshold() {
    let state = SessionMemoryState {
        last_summary_tokens: Some(test_thresholds().minimum_message_tokens_to_init),
        ..Default::default()
    };
    let candidate = ExtractionCandidate {
        prompt: Prompt::default(),
        tool_context: test_tool_context(),
        raw_boundary: None,
        active_context_tokens: test_thresholds().minimum_message_tokens_to_init
            + test_thresholds().minimum_tokens_between_update,
        natural_break: true,
    };

    assert!(should_extract(&state, &candidate, test_thresholds()));
}

#[test]
fn should_not_extract_on_tool_calls_without_token_threshold() {
    let state = SessionMemoryState {
        last_summary_tokens: Some(test_thresholds().minimum_message_tokens_to_init),
        last_summary_tool_calls: Some(0),
        ..Default::default()
    };
    let candidate = ExtractionCandidate {
        prompt: Prompt {
            input: vec![
                function_call("call-1"),
                function_call("call-2"),
                function_call("call-3"),
            ],
            ..Default::default()
        },
        tool_context: test_tool_context(),
        raw_boundary: None,
        active_context_tokens: test_thresholds().minimum_message_tokens_to_init + 1,
        natural_break: false,
    };

    assert!(!should_extract(&state, &candidate, test_thresholds()));
}

#[test]
fn should_extract_honors_custom_thresholds() {
    let state = SessionMemoryState {
        last_summary_tokens: Some(1_000),
        last_summary_tool_calls: Some(0),
        ..Default::default()
    };
    let candidate = ExtractionCandidate {
        prompt: Prompt {
            input: vec![function_call("call-1"), function_call("call-2")],
            ..Default::default()
        },
        tool_context: test_tool_context(),
        raw_boundary: None,
        active_context_tokens: 3_000,
        natural_break: false,
    };
    let thresholds = ExtractionThresholds {
        minimum_message_tokens_to_init: 500,
        minimum_tokens_between_update: 2_000,
        tool_calls_between_updates: 2,
    };

    assert!(should_extract(&state, &candidate, thresholds));
}
