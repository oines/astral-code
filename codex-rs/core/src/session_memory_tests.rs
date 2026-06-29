use super::*;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_utils_output_truncation::approx_token_count;
use pretty_assertions::assert_eq;

fn user_message(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
    }
}

fn function_call(call_id: &str) -> ResponseItem {
    ResponseItem::FunctionCall {
        id: None,
        name: "shell_command".to_string(),
        namespace: None,
        arguments: "{}".to_string(),
        call_id: call_id.to_string(),
    }
}

fn function_output(call_id: &str) -> ResponseItem {
    ResponseItem::FunctionCallOutput {
        call_id: call_id.to_string(),
        output: FunctionCallOutputPayload::from_text("ok".to_string()),
    }
}

fn state_for_boundary(items: &[ResponseItem], index: usize) -> SessionMemoryState {
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
    let summary =
        summary_with_current_state(&"token ".repeat(tail::MAX_COMPACT_SUMMARY_BODY_TOKENS * 2));

    validate_summary(&summary, tail::DEFAULT_SUMMARY).expect("summary can still be extracted");
    let (truncated, was_truncated) = truncate_summary_for_compact(&summary);

    assert!(was_truncated);
    assert!(truncated.contains("[... section truncated for length ...]"));
    assert!(approx_token_count(&truncated) <= tail::MAX_COMPACT_SUMMARY_BODY_TOKENS);
}

#[test]
fn post_extraction_rejects_tiny_rewrite_of_existing_summary() {
    let previous_summary = summary_with_current_state(&"durable detail ".repeat(3_000));
    let updated_summary = summary_with_current_state("- done");

    let err = tail::validate_post_extraction_summary(
        &previous_summary,
        &updated_summary,
        tail::DEFAULT_SUMMARY,
    )
    .expect_err("tiny rewrite should be rejected");

    assert!(
        err.to_string()
            .contains("session memory extraction collapsed existing summary unexpectedly")
    );
}

#[test]
fn post_extraction_preserves_existing_summary_headings() {
    let template = "# IM State\n_Current chat handoff_\n\n# Follow-ups\n_Open items_";
    let previous =
        "# IM State\n- Waiting for bridge reply.\n\n# Follow-ups\n- Confirm unread handling.";
    let updated = "# IM State\n- Waiting for bridge reply.\n\n# Follow-ups\n- Send result.";

    tail::validate_post_extraction_summary(previous, updated, template)
        .expect("existing custom headings are preserved");

    let err = tail::validate_post_extraction_summary(
        previous,
        "# IM State\n- Missing follow-up.",
        template,
    )
    .expect_err("updated summary should preserve previous headings");
    assert!(
        err.to_string()
            .contains("session memory summary is missing required heading # Follow-ups")
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
fn session_memory_compacted_history_keeps_tail_before_summary() {
    let tail = vec![user_message("recent user"), user_message("latest user")];

    let history = build_session_memory_compacted_history(tail.clone(), "summary".to_string());

    let mut expected = tail;
    expected.push(ResponseItem::Compaction {
        encrypted_content: "summary".to_string(),
    });
    assert_eq!(history, expected);
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
        active_context_tokens: 25_000,
        natural_break: true,
    };
    assert!(should_extract(&state, &candidate));
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
fn should_extract_requires_initial_token_threshold() {
    let state = SessionMemoryState::default();
    let prompt = Prompt {
        input: vec![user_message("small")],
        ..Default::default()
    };
    let candidate = ExtractionCandidate {
        prompt,
        active_context_tokens: MINIMUM_MESSAGE_TOKENS_TO_INIT - 1,
        natural_break: true,
    };

    assert!(!should_extract(&state, &candidate));
}

#[test]
fn should_not_extract_on_natural_break_before_token_threshold() {
    let state = SessionMemoryState {
        last_summary_tokens: Some(MINIMUM_MESSAGE_TOKENS_TO_INIT),
        ..Default::default()
    };
    let candidate = ExtractionCandidate {
        prompt: Prompt::default(),
        active_context_tokens: MINIMUM_MESSAGE_TOKENS_TO_INIT + 1,
        natural_break: true,
    };

    assert!(!should_extract(&state, &candidate));
}

#[test]
fn should_extract_on_natural_break_after_token_threshold() {
    let state = SessionMemoryState {
        last_summary_tokens: Some(MINIMUM_MESSAGE_TOKENS_TO_INIT),
        ..Default::default()
    };
    let candidate = ExtractionCandidate {
        prompt: Prompt::default(),
        active_context_tokens: MINIMUM_MESSAGE_TOKENS_TO_INIT + MINIMUM_TOKENS_BETWEEN_UPDATE,
        natural_break: true,
    };

    assert!(should_extract(&state, &candidate));
}

#[test]
fn should_not_extract_on_tool_calls_without_token_threshold() {
    let state = SessionMemoryState {
        last_summary_tokens: Some(MINIMUM_MESSAGE_TOKENS_TO_INIT),
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
        active_context_tokens: MINIMUM_MESSAGE_TOKENS_TO_INIT + 1,
        natural_break: false,
    };

    assert!(!should_extract(&state, &candidate));
}
