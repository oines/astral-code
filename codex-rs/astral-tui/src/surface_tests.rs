use chrono::Local;
use chrono::TimeZone;
use codex_app_server_protocol::ActivePermissionProfile;
use codex_app_server_protocol::ApprovalsReviewer;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::CoreToolCallStatus;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::FuzzyFileSearchMatchType;
use codex_app_server_protocol::FuzzyFileSearchResult;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::PatchChangeKind;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_app_server_protocol::TokenUsageBreakdown;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnPlanStep;
use codex_app_server_protocol::TurnPlanStepStatus;
use codex_app_server_protocol::TurnPlanUpdatedNotification;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::models::BUILT_IN_PERMISSION_PROFILE_WORKSPACE;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use serde_json::json;

use super::SurfaceActivity;
use super::SurfaceState;
use super::TranscriptView;
use super::render_surface;
use super::render_surface_with_view;
use crate::InputAction;
use crate::SessionState;
use crate::handle_key;
use crate::handle_paste;
use crate::input::handle_mouse as handle_input_mouse;
use crate::mention::MentionCandidate;
use crate::mention::MentionCatalog;
use crate::mention::MentionKind;
use crate::mention::MentionTarget;
use crate::modal::ModalRow;
use crate::modal::ModalState;
use crate::plan_review::PlanReviewAction;
use crate::view::AstralThemeId;
use crate::view::ColorLevel;

fn session_state() -> SessionState {
    let started_at = local_timestamp_seconds(/*hour*/ 0, /*minute*/ 0, /*second*/ 1);
    let thread: Thread = serde_json::from_value(json!({
        "id": "thread-1",
        "sessionId": "session-1",
        "forkedFromId": null,
        "parentThreadId": null,
        "preview": "inspect this repo",
        "ephemeral": false,
        "modelProvider": "anthropic",
        "createdAt": 1,
        "updatedAt": 2,
        "status": {"type": "idle"},
        "path": null,
        "cwd": "/workspace",
        "cliVersion": "0.0.0",
        "source": "cli",
        "threadSource": "user",
        "agentNickname": null,
        "agentRole": null,
        "gitInfo": {
            "sha": "0123456789abcdef",
            "branch": "main",
            "originUrl": "https://example.com/astral-code.git"
        },
        "name": null,
        "turns": [{
            "id": "turn-1",
            "items": [
                {
                    "type": "userMessage",
                    "id": "user-1",
                    "content": [{
                        "type": "text",
                        "text": "inspect this repo",
                        "text_elements": []
                    }]
                },
                {
                    "type": "agentMessage",
                    "id": "agent-1",
                    "text": "I’m tracing the relevant data flow.",
                    "phase": null,
                    "memoryCitation": null
                }
            ],
            "itemsView": "full",
            "status": "inProgress",
            "error": null,
            "startedAt": started_at,
            "completedAt": null,
            "durationMs": null
        }]
    }))
    .expect("valid thread");
    SessionState {
        thread,
        model: "claude-sonnet-4".to_string(),
        model_provider: "anthropic".to_string(),
        service_tier: None,
        active_turn_id: Some("turn-1".to_string()),
        collaboration_mode: CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model: "claude-sonnet-4".to_string(),
                reasoning_effort: None,
                developer_instructions: None,
            },
        },
        approval_policy: AskForApproval::OnRequest,
        approvals_reviewer: ApprovalsReviewer::User,
        active_permission_profile: Some(ActivePermissionProfile::new(
            BUILT_IN_PERMISSION_PROFILE_WORKSPACE,
        )),
    }
}

fn local_timestamp_seconds(hour: u32, minute: u32, second: u32) -> i64 {
    Local
        .with_ymd_and_hms(2023, 1, 15, hour, minute, second)
        .single()
        .expect("unambiguous local test timestamp")
        .timestamp()
}

#[test]
fn working_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Working);
    state.set_token_usage(ThreadTokenUsage {
        total: TokenUsageBreakdown {
            total_tokens: 12_345,
            input_tokens: 10_000,
            cached_input_tokens: 4_000,
            output_tokens: 2_000,
            reasoning_output_tokens: 345,
        },
        last: TokenUsageBreakdown {
            total_tokens: 9_200,
            input_tokens: 8_000,
            cached_input_tokens: 4_000,
            output_tokens: 1_000,
            reasoning_output_tokens: 200,
        },
        model_context_window: Some(500_000),
    });
    state.set_composer("follow the projection");
    let area = Rect::new(0, 0, 72, 12);
    let mut buffer = Buffer::empty(area);
    render_surface(&mut state, &session, area, &mut buffer);

    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn compacting_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Compacting);

    insta::assert_snapshot!(render_at_size(&mut state, &session, 72, 12));
}

#[test]
fn queued_follow_ups_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Working);
    state.enqueue_follow_up(crate::PromptSubmission::text_only(
        "Run the focused tests after this finishes",
    ));
    let paste = "Summarize the result\nand mention any remaining failures";
    let placeholder = "[Pasted: 2 lines]";
    state.enqueue_follow_up(crate::PromptSubmission {
        text: placeholder.to_string(),
        elements: vec![crate::composer::ComposerElement::paste(
            0..placeholder.len(),
            placeholder.to_string(),
            paste.to_string(),
        )],
    });
    state.toggle_queue_focus();
    state.move_queue_selection(1);

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn grok_view_80x24_snapshot() {
    let (mut state, session) = named_working_surface();
    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn grok_view_120x32_snapshot() {
    let (mut state, session) = named_working_surface();
    insta::assert_snapshot!(render_at_size(&mut state, &session, 120, 32));
}

#[test]
fn grok_view_narrow_snapshot() {
    let (mut state, session) = named_working_surface();
    insta::assert_snapshot!(render_at_size(&mut state, &session, 48, 16));
}

#[test]
fn slash_command_menu_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Ready);
    state.set_composer("/mo");

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn completion_pointer_accepts_the_rendered_row() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Ready);
    state.set_composer("/co");
    let area = Rect::new(0, 0, 80, 24);
    let mut buffer = Buffer::empty(area);
    render_surface(&mut state, &session, area, &mut buffer);
    let (column, row) =
        find_text(&buffer, "/compact").expect("compact completion should be visible");

    assert_eq!(
        handle_input_mouse(&mut state, mouse(MouseEventKind::Moved, column, row),),
        InputAction::Redraw
    );
    assert_eq!(
        handle_input_mouse(
            &mut state,
            mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        ),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "/compact");
    assert!(!state.slash().open);
}

#[test]
fn unclaimed_scrollback_text_returns_to_the_prompt() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    let area = Rect::new(0, 0, 80, 24);
    let mut buffer = Buffer::empty(area);
    render_surface(&mut state, &session, area, &mut buffer);
    assert!(state.focus_scrollback());

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        ),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "a");
    assert!(!state.scrollback_focused());
}

#[test]
fn prompt_pointer_places_the_edit_cursor() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Ready);
    state.set_composer("alpha beta");
    let area = Rect::new(0, 0, 80, 24);
    let mut buffer = Buffer::empty(area);
    render_surface(&mut state, &session, area, &mut buffer);
    let (column, row) = find_text(&buffer, "alpha beta").expect("prompt should be visible");

    assert_eq!(
        handle_input_mouse(
            &mut state,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                column.saturating_add(6),
                row,
            ),
        ),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE),
        ),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "alpha Xbeta");
}

#[test]
fn plan_command_menu_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Ready);
    state.set_composer("/pl");

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn prompt_history_browse_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);

    assert_eq!(
        handle_key(&mut state, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "inspect this repo");
    assert!(state.history().open);

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn skill_and_plugin_mention_menu_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Ready);
    state.set_mention_catalog(MentionCatalog {
        candidates: vec![
            MentionCandidate {
                kind: MentionKind::Plugin,
                display: "Browser Use".to_string(),
                description: "Control the active browser".to_string(),
                insert_text: "@Browser-Use".to_string(),
                search_terms: vec!["browser-use".to_string()],
                target: MentionTarget::Plugin {
                    name: "Browser Use".to_string(),
                    path: "plugin://browser-use@bundled".to_string(),
                },
            },
            MentionCandidate {
                kind: MentionKind::Skill,
                display: "Code Review".to_string(),
                description: "Review a pull request".to_string(),
                insert_text: "$code-review".to_string(),
                search_terms: vec!["code-review".to_string()],
                target: MentionTarget::Skill {
                    name: "code-review".to_string(),
                    path: "/workspace/.codex/skills/code-review/SKILL.md".into(),
                },
            },
        ],
    });
    state.set_composer("$");

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn file_search_menu_and_atomic_reference_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Ready);
    state.set_composer("@src");
    let request = state
        .take_file_search_request()
        .expect("file query should schedule a search");
    assert!(state.apply_file_search_results(
        request.generation,
        &request.query,
        vec![
            FuzzyFileSearchResult {
                root: "/workspace".to_string(),
                path: "src/lib.rs".to_string(),
                match_type: FuzzyFileSearchMatchType::File,
                file_name: "lib.rs".to_string(),
                score: 100,
                indices: Some(vec![0, 1, 2]),
            },
            FuzzyFileSearchResult {
                root: "/workspace".to_string(),
                path: "src/view".to_string(),
                match_type: FuzzyFileSearchMatchType::Directory,
                file_name: "view".to_string(),
                score: 90,
                indices: Some(vec![0, 1, 2]),
            },
        ],
    ));
    let menu = render_at_size(&mut state, &session, 80, 18);

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "@src/lib.rs ");
    assert_eq!(state.composer_elements().len(), 1);
    let chip = render_at_size(&mut state, &session, 80, 18);

    insta::assert_snapshot!(
        "file_search_menu_and_atomic_reference",
        format!("MENU\n{menu}\n\nREFERENCE\n{chip}")
    );
}

#[test]
fn file_search_colon_opens_line_viewer_and_inserts_selected_range_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Ready);
    state.set_composer("@src");
    let search = state
        .take_file_search_request()
        .expect("file query should schedule a search");
    assert!(state.apply_file_search_results(
        search.generation,
        &search.query,
        vec![FuzzyFileSearchResult {
            root: "/workspace".to_string(),
            path: "src/lib.rs".to_string(),
            match_type: FuzzyFileSearchMatchType::File,
            file_name: "lib.rs".to_string(),
            score: 100,
            indices: Some(vec![0, 1, 2]),
        }],
    ));
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
        ),
        InputAction::Redraw
    );
    let request = state
        .take_file_viewer_request()
        .expect("colon should request the selected file");
    assert_eq!(request.path, "src/lib.rs");
    assert!(state.apply_file_viewer_result(
        request.generation,
        Ok(["fn first() {}", "fn second() {}", "fn third() {}"].join("\n")),
    ));
    let _ = render_at_size(&mut state, &session, 80, 20);
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
        ),
        InputAction::Redraw
    );
    for _ in 0..2 {
        assert_eq!(
            handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            InputAction::Redraw
        );
    }
    let viewer = render_at_size(&mut state, &session, 80, 20);
    insta::assert_snapshot!("file_line_viewer_selection", viewer);

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        ),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "@src/lib.rs:1-3 ");
    assert_eq!(state.composer_elements().len(), 1);
}

#[test]
fn disconnected_slash_command_menu_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Disconnected(
        "app-server connection lost".to_string(),
    ));
    state.set_composer("/");

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn model_argument_menu_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Ready);
    set_test_model_catalog(&mut state, &session);
    state.set_composer("/model ");

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        ),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "/model Claude Sonnet 4 ");
    insta::assert_snapshot!(
        "model_effort_argument_menu_snapshot",
        render_at_size(&mut state, &session, 80, 24)
    );

    let InputAction::Slash { invocation, .. } = handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    ) else {
        panic!("terminal effort selection should execute with the same Enter");
    };
    assert_eq!(invocation.command, crate::SlashCommandId::Model);
    assert_eq!(invocation.args, "Claude Sonnet 4 xhigh");
}

#[test]
fn model_picker_snapshot_and_effort_selection() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Ready);
    set_test_model_catalog(&mut state, &session);
    let _ = render_at_size(&mut state, &session, 80, 24);
    assert!(state.focus_scrollback());

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('m'), KeyModifiers::CONTROL),
        ),
        InputAction::Redraw
    );
    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        ),
        InputAction::Redraw
    );
    let InputAction::Slash { invocation, .. } = handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    ) else {
        panic!("effort selection should dispatch /model");
    };
    assert_eq!(invocation.command, crate::SlashCommandId::Model);
    assert_eq!(invocation.args, "Claude Sonnet 4 xhigh");
}

fn set_test_model_catalog(state: &mut SurfaceState, session: &SessionState) {
    state.set_model_catalog(
        vec![
            serde_json::from_value(json!({
                "modelProvider": "anthropic",
                "modelProviderName": "Anthropic",
                "id": "claude-sonnet-4",
                "model": "claude-sonnet-4",
                "upgrade": null,
                "upgradeInfo": null,
                "availabilityNux": null,
                "displayName": "Claude Sonnet 4",
                "description": "Fast coding model",
                "hidden": false,
                "supportedReasoningEfforts": [],
                "defaultReasoningEffort": "none",
                "inputModalities": ["text", "image"],
                "supportsPersonality": true,
                "additionalSpeedTiers": [],
                "serviceTiers": [],
                "defaultServiceTier": null,
                "isDefault": true
            }))
            .expect("valid model"),
        ],
        session.model.clone(),
        session.model_provider.clone(),
        Some(codex_protocol::openai_models::ReasoningEffort::High),
    );
}

#[test]
fn status_modal_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.open_modal(ModalState::info(
        "Session status",
        vec![
            ModalRow::new("Thread", "thread-1"),
            ModalRow::new("Model", "claude-sonnet-4 · anthropic"),
            ModalRow::new("Working directory", "/workspace"),
            ModalRow::new("Context", "9.2K / 500K"),
        ],
    ));

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn ecosystem_modal_scroll_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    let mut modal = ModalState::info(
        "Skills",
        (0..30)
            .map(|index| {
                ModalRow::new(
                    format!("skill-{index:02}"),
                    if index % 3 == 0 {
                        "repo · enabled"
                    } else {
                        "user · enabled"
                    },
                )
            })
            .collect(),
    );
    modal.scroll_by(8);
    state.open_modal(modal);

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn shortcuts_modal_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.open_shortcut_help();

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn shortcuts_sessions_action_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.open_shortcut_help();
    let shortcuts = state
        .shortcut_help_mut()
        .expect("shortcut help should be open");
    shortcuts.begin_search();
    for character in "sessions".chars() {
        shortcuts.insert_query(character);
    }
    shortcuts.select(1);
    assert!(shortcuts.open_selected_detail());

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn shortcuts_modal_scrolled_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.open_shortcut_help();
    let shortcuts = state
        .shortcut_help_mut()
        .expect("shortcut help should be open");
    shortcuts.select(5);
    shortcuts.expand_selected();
    shortcuts.select(8);
    shortcuts.expand_selected();
    shortcuts.select_end();

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn command_palette_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_composer("keep this draft");
    state.open_command_palette();

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn multiline_prompt_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.toggle_multiline_mode();
    state.set_composer("first line\nsecond line");

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn new_session_confirmation_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Ready);
    state.set_composer("keep this draft");
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
        ),
        InputAction::Redraw
    );

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn theme_picker_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.open_theme_picker();

    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn visible_overlay_owns_keyboard_input_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.pending_requests_mut().note(request(json!({
        "method": "item/commandExecution/requestApproval",
        "id": "request-behind-overlay",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-behind-overlay",
            "startedAtMs": 100,
            "reason": "request arrived while the theme picker was open",
            "command": "just test",
            "cwd": "/workspace"
        }
    })));
    state.open_theme_picker();

    assert_eq!(
        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),),
        InputAction::Redraw
    );
    assert_eq!(state.theme_id(), AstralThemeId::Day);
    assert_eq!(state.pending_requests().len(), 1);
    insta::assert_snapshot!(render_at_size(&mut state, &session, 80, 24));
}

#[test]
fn subagent_view_is_read_only_and_owns_input_snapshot() {
    let session = session_state();
    let child: Thread = serde_json::from_value(json!({
        "id": "thread-child",
        "sessionId": "session-1",
        "forkedFromId": null,
        "parentThreadId": "thread-1",
        "preview": "inspect retry behavior",
        "ephemeral": false,
        "modelProvider": "anthropic",
        "createdAt": 2,
        "updatedAt": 3,
        "status": {"type": "idle"},
        "path": null,
        "cwd": "/workspace",
        "cliVersion": "0.0.0",
        "source": "cli",
        "threadSource": "subagent",
        "agentNickname": "Feynman",
        "agentRole": "explorer",
        "gitInfo": null,
        "name": null,
        "turns": [{
            "id": "turn-child",
            "items": [
                {
                    "type": "userMessage",
                    "id": "child-user",
                    "content": [{
                        "type": "text",
                        "text": "Inspect the retry policy.",
                        "text_elements": []
                    }]
                },
                {
                    "type": "reasoning",
                    "id": "child-reasoning",
                    "summary": ["Trace the retry configuration and call sites."],
                    "content": []
                },
                {
                    "type": "commandExecution",
                    "id": "child-command",
                    "command": "rg max_retry",
                    "cwd": "/workspace",
                    "processId": null,
                    "source": "agent",
                    "status": "completed",
                    "commandActions": [{
                        "type": "search",
                        "command": "rg max_retry",
                        "query": "max_retry",
                        "path": null
                    }],
                    "aggregatedOutput": "src/retry.rs:42: max_retry = 3",
                    "exitCode": 0,
                    "durationMs": 18
                },
                {
                    "type": "agentMessage",
                    "id": "child-agent",
                    "text": "The retry policy is configured in `src/retry.rs`.",
                    "phase": null,
                    "memoryCitation": null
                }
            ],
            "itemsView": "full",
            "status": "completed",
            "error": null,
            "startedAt": 2,
            "completedAt": 3,
            "durationMs": 1000
        }]
    }))
    .expect("valid child thread");
    let mut state = SurfaceState::from_session(&session);
    state.open_subagent_view(child, &session);

    assert_eq!(
        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        InputAction::Redraw
    );
    insta::assert_snapshot!(render_at_size(&mut state, &session, 100, 28));

    assert_eq!(
        handle_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        InputAction::Redraw
    );
    assert!(!state.subagent_view_open());
}

#[test]
fn timeline_rail_surface_snapshot() {
    let mut session = session_state();
    let second_started_at =
        local_timestamp_seconds(/*hour*/ 0, /*minute*/ 0, /*second*/ 3);
    session.thread.turns.push(
        serde_json::from_value(json!({
            "id": "turn-2",
            "items": [
                {
                    "type": "userMessage",
                    "id": "user-2",
                    "content": [{
                        "type": "text",
                        "text": "continue with the implementation",
                        "text_elements": []
                    }]
                },
                {
                    "type": "agentMessage",
                    "id": "agent-2",
                    "text": "I’m validating the next layer.",
                    "phase": null,
                    "memoryCitation": null
                }
            ],
            "itemsView": "full",
            "status": "completed",
            "error": null,
            "startedAt": second_started_at,
            "completedAt": second_started_at + 1,
            "durationMs": 1000
        }))
        .expect("valid second turn"),
    );
    let mut state = SurfaceState::from_session(&session);
    state.set_timeline_visible(true);
    let area = Rect::new(0, 0, 80, 24);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let (column, row) = find_symbol(&buffer, "━").expect("active timeline tick");
    assert_eq!(
        handle_input_mouse(&mut state, mouse(MouseEventKind::Moved, column, row),),
        InputAction::Redraw
    );
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    insta::assert_snapshot!(buffer_text(&buffer));

    let (_, prompt_row_before) =
        find_text(&buffer, "continue with the implementation").expect("second prompt");
    let rail_column = area.right().saturating_sub(1);
    let second_tick_row = (area.y..area.bottom())
        .find(|row| buffer[(rail_column, *row)].symbol() == "─")
        .expect("second timeline tick");
    assert_eq!(
        handle_input_mouse(
            &mut state,
            mouse(MouseEventKind::Moved, rail_column, second_tick_row),
        ),
        InputAction::Redraw
    );
    assert_eq!(
        handle_input_mouse(
            &mut state,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                rail_column,
                second_tick_row,
            ),
        ),
        InputAction::Redraw
    );
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let (_, prompt_row_after) =
        find_text(&buffer, "continue with the implementation").expect("jumped prompt");
    assert!(
        prompt_row_after < prompt_row_before,
        "timeline jump should move prompt toward the viewport top: {prompt_row_before} -> \
         {prompt_row_after}\n{}",
        buffer_text(&buffer)
    );
}

#[test]
fn selected_theme_controls_the_surface_background() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_theme(AstralThemeId::Day);
    let area = Rect::new(0, 0, 80, 24);
    let mut buffer = Buffer::empty(area);
    render_surface(&mut state, &session, area, &mut buffer);

    assert_eq!(buffer[(0, 0)].bg, state.theme().bg_base);
}

#[test]
fn command_approval_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    let request: ServerRequest = serde_json::from_value(json!({
        "method": "item/commandExecution/requestApproval",
        "id": 8,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "startedAtMs": 100,
            "reason": "needs network access",
            "command": "cargo test --workspace",
            "cwd": "/workspace"
        }
    }))
    .expect("valid command approval");
    state.pending_requests_mut().note(request);
    assert_eq!(
        state
            .pending_requests()
            .front()
            .map(super::super::request::PendingRequest::request_id),
        Some(&RequestId::Integer(8))
    );
    let area = Rect::new(0, 0, 72, 18);
    let mut buffer = Buffer::empty(area);
    render_surface(&mut state, &session, area, &mut buffer);

    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn typed_approval_surfaces_snapshot() {
    insta::assert_snapshot!(
        "file_change_approval_surface",
        request_surface(
            json!({
                "method": "item/fileChange/requestApproval",
                "id": "edit-1",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "itemId": "edit-1",
                    "startedAtMs": 101,
                    "reason": "update source",
                    "grantRoot": null
                }
            }),
            ""
        )
    );
    insta::assert_snapshot!(
        "permissions_approval_surface",
        request_surface(
            json!({
                "method": "item/permissions/requestApproval",
                "id": "permissions-1",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "itemId": "call-1",
                    "environmentId": null,
                    "startedAtMs": 102,
                    "cwd": "/workspace",
                    "reason": "read generated files",
                    "permissions": {
                        "network": {"enabled": true},
                        "fileSystem": {
                            "read": ["/workspace/generated"],
                            "write": null
                        }
                    }
                }
            }),
            ""
        )
    );
    insta::assert_snapshot!(
        "mcp_form_elicitation_surface",
        request_surface(
            json!({
                "method": "mcpServer/elicitation/request",
                "id": "mcp-form",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "serverName": "astral",
                    "mode": "form",
                    "_meta": null,
                    "message": "Choose settings",
                    "requestedSchema": {
                        "type": "object",
                        "properties": {
                            "confirmed": {
                                "type": "boolean",
                                "title": "Confirm changes",
                                "description": "Allow the MCP server to apply these settings"
                            }
                        },
                        "required": ["confirmed"]
                    }
                }
            }),
            "keep this prompt draft"
        )
    );
    insta::assert_snapshot!(
        "mcp_multi_select_elicitation_surface",
        request_surface(
            json!({
                "method": "mcpServer/elicitation/request",
                "id": "mcp-multi-form",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "serverName": "astral",
                    "mode": "form",
                    "_meta": null,
                    "message": "Choose capabilities",
                    "requestedSchema": {
                        "type": "object",
                        "properties": {
                            "features": {
                                "type": "array",
                                "title": "Capabilities",
                                "description": "Select every capability this server may use",
                                "items": {
                                    "type": "string",
                                    "enum": ["search", "edit", "terminal"]
                                },
                                "default": ["edit"]
                            }
                        },
                        "required": ["features"]
                    }
                }
            }),
            "keep this prompt draft"
        )
    );
    insta::assert_snapshot!(
        "user_question_surface",
        request_surface(
            json!({
                "method": "item/tool/requestUserInput",
                "id": "question-1",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "itemId": "question-1",
                    "questions": [{
                        "id": "scope",
                        "header": "Choose scope",
                        "question": "Where should the refactor apply?",
                        "isOther": false,
                        "isSecret": false,
                        "options": [{
                            "label": "Workspace only",
                            "description": "Keep the change local to this repository"
                        }, {
                            "label": "Shared runtime",
                            "description": "Update the common implementation"
                        }]
                    }]
                }
            }),
            "Workspace only"
        )
    );
    insta::assert_snapshot!(
        "mcp_url_elicitation_surface",
        request_surface(
            json!({
                "method": "mcpServer/elicitation/request",
                "id": "mcp-url",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "serverName": "linear",
                    "mode": "url",
                    "_meta": null,
                    "message": "Connect Linear to continue",
                    "url": "https://linear.app/oauth/authorize",
                    "elicitationId": "auth-1"
                }
            }),
            ""
        )
    );
}

#[test]
fn user_question_confirmation_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.pending_requests_mut().note(request(json!({
        "method": "item/tool/requestUserInput",
        "id": "question-1",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "questions": [
                {"id": "first", "header": "First", "question": "First?", "options": null},
                {"id": "second", "header": "Second", "question": "Second?", "options": null}
            ]
        }
    })));
    handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
    );
    handle_paste(&mut state, "answered");
    handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );

    insta::assert_snapshot!(render_at_size(&mut state, &session, 72, 18));
}

#[test]
fn mcp_url_elicitation_waits_for_browser_completion_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.pending_requests_mut().note(request(json!({
        "method": "mcpServer/elicitation/request",
        "id": "mcp-url",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "serverName": "linear",
            "mode": "url",
            "_meta": null,
            "message": "Connect Linear to continue",
            "url": "https://linear.app/oauth/authorize",
            "elicitationId": "auth-1"
        }
    })));

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ),
        InputAction::OpenLink(crate::LinkTarget::Url(
            "https://linear.app/oauth/authorize".to_string()
        ))
    );

    insta::assert_snapshot!(render_at_size(&mut state, &session, 72, 18));
}

#[test]
fn user_question_long_options_keep_selection_visible_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    let options = (1..=14)
        .map(|index| {
            json!({
                "label": format!("Option {index}"),
                "description": format!("Description {index}")
            })
        })
        .collect::<Vec<_>>();
    state.pending_requests_mut().note(request(json!({
        "method": "item/tool/requestUserInput",
        "id": "question-1",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "questions": [{
                "id": "choice",
                "header": "Choose one",
                "question": "The selected option must stay visible.",
                "options": options
            }]
        }
    })));
    for _ in 0..11 {
        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }

    insta::assert_snapshot!(render_at_size(&mut state, &session, 72, 18));
}

#[test]
fn fullscreen_surface_keeps_committed_history_snapshot() {
    let mut session = session_state();
    session.thread.turns[0].items.insert(
        1,
        ThreadItem::Reasoning {
            id: "opaque-reasoning".to_string(),
            summary: Vec::new(),
            content: Vec::new(),
        },
    );
    session.thread.turns[0].status = codex_app_server_protocol::TurnStatus::Completed;
    session.thread.turns[0].completed_at = Some(local_timestamp_seconds(
        /*hour*/ 0, /*minute*/ 0, /*second*/ 2,
    ));
    session.active_turn_id = None;
    let mut state = SurfaceState::from_session(&session);
    assert_eq!(state.drain_committable().len(), 2);
    let area = Rect::new(0, 0, 72, 12);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );

    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn fullscreen_selection_overlay_snapshot() {
    let (buffer, start, theme) = render_day_selection(ColorLevel::TrueColor);

    assert_eq!(buffer[start].fg, theme.bg_base);
    assert_eq!(buffer[start].bg, theme.text_primary);
    insta::assert_snapshot!(
        "fullscreen_selection_overlay",
        format!(
            "{}\n\nselection mask:\n{}",
            buffer_text(&buffer),
            selection_mask(&buffer, theme.text_primary)
        )
    );
}

#[test]
fn fullscreen_selection_overlay_ansi256_snapshot() {
    let (buffer, start, theme) = render_day_selection(ColorLevel::Ansi256);

    assert_eq!(format!("{:?}", theme.bg_base), "Indexed(255)");
    assert_eq!(format!("{:?}", theme.text_primary), "Indexed(234)");
    assert_eq!(buffer[start].fg, theme.bg_base);
    assert_eq!(buffer[start].bg, theme.text_primary);
    insta::assert_snapshot!(
        "fullscreen_selection_overlay_ansi256",
        format!(
            "selected fg: {:?}\nselected bg: {:?}\n\nselection mask:\n{}",
            buffer[start].fg,
            buffer[start].bg,
            selection_mask(&buffer, theme.text_primary)
        )
    );
}

fn render_day_selection(color_level: ColorLevel) -> (Buffer, (u16, u16), crate::view::AstralTheme) {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_theme(AstralThemeId::Day);
    state.set_color_level(color_level);
    let theme = state.theme();
    let area = Rect::new(0, 0, 72, 16);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let start = find_symbol(&buffer, "I").expect("assistant response is visible");
    let end_column = start.0.saturating_add(9);

    state.handle_scrollback_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        start.0,
        start.1,
    ));
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    state.handle_scrollback_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        end_column,
        start.1,
    ));
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );

    (buffer, start, theme)
}

#[test]
fn copied_selection_clears_on_scroll_and_reflow() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_theme(AstralThemeId::Day);
    let theme = state.theme();
    let area = Rect::new(0, 0, 72, 16);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let start = find_symbol(&buffer, "I").expect("assistant response is visible");
    let end_column = start.0.saturating_add(4);

    state.handle_scrollback_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        start.0,
        start.1,
    ));
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    state.handle_scrollback_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        end_column,
        start.1,
    ));
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    assert!(matches!(
        state.handle_scrollback_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            end_column,
            start.1,
        )),
        crate::view::ScrollbackMouseAction::Copy(_)
    ));
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    assert_eq!(buffer[start].bg, theme.text_primary);

    state.scroll_up(/*lines*/ 0);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    assert_ne!(buffer[start].bg, theme.text_primary);

    state.handle_scrollback_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        start.0,
        start.1,
    ));
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    state.handle_scrollback_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        end_column,
        start.1,
    ));
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    assert!(matches!(
        state.handle_scrollback_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            end_column,
            start.1,
        )),
        crate::view::ScrollbackMouseAction::Copy(_)
    ));

    let narrower = Rect::new(0, 0, 71, 16);
    let mut narrower_buffer = Buffer::empty(narrower);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        narrower,
        &mut narrower_buffer,
    );
    assert_ne!(narrower_buffer[start].bg, theme.text_primary);
}

#[test]
fn fullscreen_scrollback_viewport_snapshot() {
    let mut session = session_state();
    let base_timestamp =
        local_timestamp_seconds(/*hour*/ 22, /*minute*/ 13, /*second*/ 20);
    let template = session.thread.turns[0].clone();
    session.thread.turns = (0..8)
        .map(|index| {
            let mut turn = template.clone();
            turn.id = format!("turn-{index}");
            turn.items = vec![
                ThreadItem::UserMessage {
                    id: format!("user-{index}"),
                    client_id: None,
                    content: vec![UserInput::Text {
                        text: format!("question {index}: inspect the layered transcript"),
                        text_elements: Vec::new(),
                    }],
                },
                ThreadItem::AgentMessage {
                    id: format!("agent-{index}"),
                    text: format!(
                        "Response {index} keeps enough content to make the transcript overflow. \
                         [Astral](https://example.com/docs)"
                    ),
                    phase: None,
                    memory_citation: None,
                },
            ];
            turn.status = TurnStatus::Completed;
            turn.started_at = Some(base_timestamp + index);
            turn.completed_at = Some(base_timestamp + index + 1);
            turn.duration_ms = Some(1_000);
            turn
        })
        .collect();
    session.active_turn_id = None;
    let mut state = SurfaceState::from_session(&session);
    state.scroll_up(/*lines*/ 5);
    let area = Rect::new(0, 0, 80, 24);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    assert!(state.cycle_scrollback_link(true));
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );

    insta::assert_snapshot!(buffer_text(&buffer));

    let (column, row) = find_symbol(&buffer, "▼").expect("follow indicator");
    assert_eq!(
        handle_input_mouse(
            &mut state,
            mouse(MouseEventKind::Down(MouseButton::Left), column, row,),
        ),
        InputAction::Redraw
    );
    assert_eq!(state.scroll_offset(), 0);
}

#[test]
fn transcript_search_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    let area = Rect::new(0, 0, 80, 20);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    assert!(state.focus_scrollback());
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        ),
        InputAction::Redraw
    );
    for character in "relevant".chars() {
        assert_eq!(
            handle_key(
                &mut state,
                KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
            ),
            InputAction::Redraw
        );
    }
    let mut settled = false;
    for _ in 0..1_000 {
        let _ = state.poll_scrollback_search();
        if !state.scrollback_search_pending() {
            settled = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(settled, "transcript search daemon should settle");

    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let (x, y) = find_text(&buffer, "relevant").expect("visible search match");
    assert!(
        buffer[(x, y)].modifier.contains(Modifier::REVERSED),
        "search match should be highlighted"
    );
    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn grok_layered_turn_139x35_snapshot() {
    let mut session = session_state();
    let base_timestamp =
        local_timestamp_seconds(/*hour*/ 22, /*minute*/ 13, /*second*/ 20);
    let base_timestamp_ms = base_timestamp * 1_000;
    let mut turn = session.thread.turns.remove(0);
    session.active_turn_id = None;
    let mut state = SurfaceState::new("thread-1");
    let items = vec![
        ThreadItem::UserMessage {
            id: "user-layered".to_string(),
            client_id: None,
            content: vec![UserInput::Text {
                text: "你是谁".to_string(),
                text_elements: Vec::new(),
            }],
        },
        ThreadItem::Reasoning {
            id: "reasoning-layered".to_string(),
            summary: vec!["Identify the request and answer directly.".to_string()],
            content: Vec::new(),
        },
        ThreadItem::AgentMessage {
            id: "agent-layered".to_string(),
            text: "我是 **Astral**，一个面向软件工程的交互式助手。\n\n- 读写代码\n- 运行命令\n- 调试和重构".to_string(),
            phase: None,
            memory_citation: None,
        },
    ];
    turn.id = "turn-layered".to_string();
    turn.items.clear();
    turn.status = TurnStatus::InProgress;
    turn.started_at = Some(base_timestamp);
    turn.completed_at = None;
    turn.duration_ms = None;
    state
        .conversation_mut()
        .apply(&ServerNotification::TurnStarted(TurnStartedNotification {
            thread_id: "thread-1".to_string(),
            turn: turn.clone(),
        }));
    for (item, started_at_ms, completed_at_ms) in [
        (items[0].clone(), base_timestamp_ms, base_timestamp_ms + 50),
        (
            items[1].clone(),
            base_timestamp_ms + 50,
            base_timestamp_ms + 550,
        ),
        (
            items[2].clone(),
            base_timestamp_ms + 550,
            base_timestamp_ms + 2_400,
        ),
    ] {
        state
            .conversation_mut()
            .apply(&ServerNotification::ItemStarted(ItemStartedNotification {
                item: item.clone(),
                thread_id: "thread-1".to_string(),
                turn_id: turn.id.clone(),
                started_at_ms,
            }));
        state
            .conversation_mut()
            .apply(&ServerNotification::ItemCompleted(
                ItemCompletedNotification {
                    item,
                    thread_id: "thread-1".to_string(),
                    turn_id: turn.id.clone(),
                    completed_at_ms,
                },
            ));
    }
    turn.items = items;
    turn.status = TurnStatus::Completed;
    turn.completed_at = Some(base_timestamp + 2);
    turn.duration_ms = Some(2_400);
    state
        .conversation_mut()
        .apply(&ServerNotification::TurnCompleted(
            TurnCompletedNotification {
                thread_id: "thread-1".to_string(),
                turn,
            },
        ));
    let area = Rect::new(0, 0, 139, 35);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );

    let prompt_row = (0..area.height)
        .find(|y| (0..area.width).any(|x| buffer[(x, *y)].symbol() == "›"))
        .unwrap_or_else(|| panic!("user prompt row missing:\n{}", buffer_text(&buffer)));
    let band = crate::view::AstralTheme::default().panel_selected;
    let content_left = area.x + 2;
    let content_right = area.right() - 3;
    assert!(
        (content_left..content_right).all(|x| buffer[(x, prompt_row - 1)].bg == band)
            && (content_left..content_right)
                .skip(30)
                .all(|x| buffer[(x, prompt_row)].bg == band)
            && (content_left..content_right).all(|x| buffer[(x, prompt_row + 1)].bg == band),
        "the user turn row must keep its full-width background band"
    );
    let theme = crate::view::AstralTheme::default();
    assert!(
        buffer
            .content
            .iter()
            .any(|cell| cell.symbol() == "A" && cell.modifier.contains(Modifier::BOLD)),
        "assistant Markdown strong text must be bold"
    );
    assert!(
        buffer
            .content
            .iter()
            .any(|cell| cell.symbol() == "•" && cell.fg == theme.gray),
        "assistant Markdown list markers must use the Astral theme"
    );
    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn reused_provider_item_ids_preserve_turn_order_snapshot() {
    let mut session = session_state();
    let base_timestamp =
        local_timestamp_seconds(/*hour*/ 22, /*minute*/ 13, /*second*/ 20);
    let template = session.thread.turns.remove(0);
    session.thread.turns = ["first", "second"]
        .into_iter()
        .enumerate()
        .map(|(index, label)| {
            let mut turn = template.clone();
            turn.id = format!("turn-{label}");
            turn.items = vec![
                ThreadItem::UserMessage {
                    id: format!("user-{label}"),
                    client_id: None,
                    content: vec![UserInput::Text {
                        text: format!("{label} question"),
                        text_elements: Vec::new(),
                    }],
                },
                ThreadItem::Reasoning {
                    id: String::new(),
                    summary: vec![format!("{label} thought")],
                    content: Vec::new(),
                },
                ThreadItem::AgentMessage {
                    id: format!("agent-{label}"),
                    text: format!("{label} response"),
                    phase: None,
                    memory_citation: None,
                },
            ];
            turn.status = TurnStatus::Completed;
            turn.started_at = Some(base_timestamp + index as i64 * 10);
            turn.completed_at = Some(base_timestamp + index as i64 * 10 + 2);
            turn.duration_ms = Some(2_000);
            turn
        })
        .collect();
    session.active_turn_id = None;
    let mut state = SurfaceState::from_session(&session);
    let area = Rect::new(0, 0, 139, 35);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );

    let rendered = buffer_text(&buffer);
    let first_question = rendered
        .find("first question")
        .unwrap_or_else(|| panic!("first question missing:\n{rendered}"));
    let first_response = rendered
        .find("first response")
        .unwrap_or_else(|| panic!("first response missing:\n{rendered}"));
    let second_question = rendered
        .find("second question")
        .unwrap_or_else(|| panic!("second question missing:\n{rendered}"));
    let second_response = rendered
        .find("second response")
        .unwrap_or_else(|| panic!("second response missing:\n{rendered}"));
    assert!(
        first_question < first_response
            && first_response < second_question
            && second_question < second_response,
        "turns must render in chronological order:\n{rendered}"
    );
    insta::assert_snapshot!("reused_provider_item_ids_preserve_turn_order", rendered);
}

#[test]
fn interleaved_live_events_render_each_turn_once_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::new("thread-1");
    for (turn_id, item) in [
        (
            "turn-first",
            ThreadItem::UserMessage {
                id: "user-first".to_string(),
                client_id: None,
                content: vec![UserInput::Text {
                    text: "first question".to_string(),
                    text_elements: Vec::new(),
                }],
            },
        ),
        (
            "turn-second",
            ThreadItem::UserMessage {
                id: "user-second".to_string(),
                client_id: None,
                content: vec![UserInput::Text {
                    text: "second question".to_string(),
                    text_elements: Vec::new(),
                }],
            },
        ),
        (
            "turn-first",
            ThreadItem::AgentMessage {
                id: "agent-first".to_string(),
                text: "first response".to_string(),
                phase: None,
                memory_citation: None,
            },
        ),
    ] {
        state
            .conversation_mut()
            .apply(&ServerNotification::ItemCompleted(
                ItemCompletedNotification {
                    thread_id: "thread-1".to_string(),
                    turn_id: turn_id.to_string(),
                    item,
                    completed_at_ms: 20,
                },
            ));
    }
    let area = Rect::new(0, 0, 80, 20);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );

    let rendered = buffer_text(&buffer);
    let first_question = rendered.find("first question").expect("first question");
    let first_response = rendered.find("first response").expect("first response");
    let second_question = rendered.find("second question").expect("second question");
    assert!(first_question < first_response && first_response < second_question);
    insta::assert_snapshot!("interleaved_live_events_render_each_turn_once", rendered);
}

#[test]
fn todo_notification_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state
        .conversation_mut()
        .apply(&ServerNotification::TurnPlanUpdated(
            TurnPlanUpdatedNotification {
                thread_id: "thread-1".to_string(),
                turn_id: "turn-1".to_string(),
                explanation: Some("Keep the provider-neutral projection visible.".to_string()),
                plan: vec![
                    TurnPlanStep {
                        step: "Trace app-server notification".to_string(),
                        status: TurnPlanStepStatus::Completed,
                    },
                    TurnPlanStep {
                        step: "Render Grok-style todo rows".to_string(),
                        status: TurnPlanStepStatus::InProgress,
                    },
                    TurnPlanStep {
                        step: "Verify Claude and Codex surfaces".to_string(),
                        status: TurnPlanStepStatus::Pending,
                    },
                ],
            },
        ));
    let area = Rect::new(0, 0, 100, 28);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );

    let rendered = buffer_text(&buffer);
    assert!(rendered.contains("✓ Trace app-server notification"));
    assert!(rendered.contains("▶ Render Grok-style todo rows"));
    assert!(rendered.contains("□ Verify Claude and Codex surfaces"));
    insta::assert_snapshot!("todo_notification_surface", rendered);
}

#[test]
fn scrollback_focus_folds_tool_entries_snapshot() {
    let mut session = session_state();
    session.thread.turns[0]
        .items
        .push(ThreadItem::CoreToolCall {
            id: "tool-1".to_string(),
            tool: "Bash".to_string(),
            arguments: json!({"command": "cargo test -p astral-tui"}),
            status: CoreToolCallStatus::Completed,
            result: Some(
                [
                    "running unit tests",
                    "running snapshot tests",
                    "running integration tests",
                    "150 passed",
                    "0 failed",
                    "finished",
                ]
                .join("\n"),
            ),
            error: None,
            duration_ms: Some(2_400),
        });
    let mut state = SurfaceState::from_session(&session);
    render_at_size(&mut state, &session, 80, 20);

    assert_eq!(
        handle_key(&mut state, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        crate::InputAction::Redraw
    );
    assert!(state.scrollback_focused());
    let collapsed = render_at_size(&mut state, &session, 80, 20);
    assert!(!collapsed.contains("running unit tests"));

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)
        ),
        crate::InputAction::Redraw
    );
    let preview = render_at_size(&mut state, &session, 80, 20);
    assert!(preview.contains("running unit tests"));
    assert!(!preview.contains("running integration tests"));
    assert!(preview.contains("finished"));

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE)
        ),
        crate::InputAction::Redraw
    );
    let expanded = render_at_size(&mut state, &session, 80, 20);
    assert!(expanded.contains("running integration tests"));

    insta::assert_snapshot!(
        "scrollback_focus_fold_surface",
        format!("COLLAPSED\n{collapsed}\n\nPREVIEW\n{preview}\n\nEXPANDED\n{expanded}")
    );
    let area = Rect::new(0, 0, 80, 20);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let (column, row) =
        find_text(&buffer, "❯").unwrap_or_else(|| panic!("prompt is visible:\n{expanded}"));
    assert_eq!(
        handle_input_mouse(
            &mut state,
            mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        ),
        crate::InputAction::Redraw
    );
    assert!(!state.scrollback_focused());
    let (column, row) =
        find_text(&buffer, "Run").unwrap_or_else(|| panic!("tool entry is visible:\n{expanded}"));
    assert_eq!(
        handle_input_mouse(
            &mut state,
            mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        ),
        crate::InputAction::None
    );
    assert!(state.scrollback_focused());
}

#[test]
fn verb_group_double_click_reveals_members_below_the_header_snapshot() {
    let mut session = session_state();
    session.thread.turns[0].items = vec![
        ThreadItem::UserMessage {
            id: "user-group".to_string(),
            client_id: None,
            content: vec![UserInput::Text {
                text: "inspect these files".to_string(),
                text_elements: Vec::new(),
            }],
        },
        ThreadItem::Reasoning {
            id: "thought-group".to_string(),
            summary: vec!["Locate the relevant files first.".to_string()],
            content: Vec::new(),
        },
        ThreadItem::CoreToolCall {
            id: "read-a".to_string(),
            tool: "Read".to_string(),
            arguments: json!({"path": "src/a.rs"}),
            status: CoreToolCallStatus::Completed,
            result: Some("fn a() {}".to_string()),
            error: None,
            duration_ms: Some(20),
        },
        ThreadItem::CoreToolCall {
            id: "search".to_string(),
            tool: "grep".to_string(),
            arguments: json!({"pattern": "render", "path": "src"}),
            status: CoreToolCallStatus::Completed,
            result: Some("src/a.rs:1:fn render()".to_string()),
            error: None,
            duration_ms: Some(30),
        },
        ThreadItem::CoreToolCall {
            id: "read-b".to_string(),
            tool: "Read".to_string(),
            arguments: json!({"path": "src/b.rs"}),
            status: CoreToolCallStatus::Completed,
            result: Some("fn b() {}".to_string()),
            error: None,
            duration_ms: Some(20),
        },
        ThreadItem::AgentMessage {
            id: "agent-group".to_string(),
            text: "The relevant files are loaded.".to_string(),
            phase: None,
            memory_citation: None,
        },
    ];
    session.thread.turns[0].status = TurnStatus::Completed;
    session.active_turn_id = None;
    let mut state = SurfaceState::from_session(&session);
    let area = Rect::new(0, 0, 88, 28);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let collapsed = buffer_text(&buffer);
    assert!(collapsed.contains("Read 2 files, Searched 1 pattern"));
    assert!(!collapsed.contains("Read a.rs"));
    assert!(state.focus_scrollback());
    state.move_entry_selection(-1);
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ),
        InputAction::Redraw
    );
    assert!(
        state.block_viewer().is_none(),
        "Enter on a Grok-style group header toggles the group"
    );
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let expanded_before_viewer = buffer_text(&buffer);
    assert!(expanded_before_viewer.contains("Read a.rs"));
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ),
        InputAction::Redraw
    );
    assert!(
        state.block_viewer().is_some(),
        "Enter on an expanded verb slot opens its anchor entry"
    );
    assert_eq!(
        handle_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        InputAction::Redraw
    );
    assert!(state.block_viewer().is_none());
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let (column, row) = find_text(&buffer, "Read 2 files")
        .unwrap_or_else(|| panic!("group header is visible:\n{collapsed}"));

    for _ in 0..2 {
        state.handle_scrollback_mouse(mouse(MouseEventKind::Down(MouseButton::Left), column, row));
        state.handle_scrollback_mouse(mouse(MouseEventKind::Up(MouseButton::Left), column, row));
    }

    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let collapsed_again = buffer_text(&buffer);
    assert!(collapsed_again.contains("Read 2 files, Searched 1 pattern"));
    assert!(!collapsed_again.contains("Read a.rs"));
    let (column, row) = find_text(&buffer, "Read 2 files")
        .unwrap_or_else(|| panic!("collapsed group header is visible:\n{collapsed_again}"));
    for _ in 0..2 {
        state.handle_scrollback_mouse(mouse(MouseEventKind::Down(MouseButton::Left), column, row));
        state.handle_scrollback_mouse(mouse(MouseEventKind::Up(MouseButton::Left), column, row));
    }

    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let expanded = buffer_text(&buffer);
    assert!(
        expanded.contains("Read 2 files, Searched 1 pattern"),
        "{expanded}"
    );
    assert!(expanded.contains("Read a.rs"));
    assert!(expanded.contains("Search src"));
    assert!(expanded.contains("Read b.rs"));
    insta::assert_snapshot!(
        "verb_group_mouse_expand_surface",
        format!("COLLAPSED\n{collapsed}\n\nEXPANDED\n{expanded}")
    );
}

#[test]
fn edit_diff_expands_by_default_and_double_click_collapses_snapshot() {
    let mut session = session_state();
    session.thread.turns[0].items.push(ThreadItem::FileChange {
        id: "edit-1".to_string(),
        changes: vec![FileUpdateChange {
            path: "src/lib.rs".to_string(),
            kind: PatchChangeKind::Update { move_path: None },
            diff: "@@ -1 +1 @@\n-old value\n+new value".to_string(),
        }],
        status: PatchApplyStatus::Completed,
    });
    let mut state = SurfaceState::from_session(&session);
    let area = Rect::new(0, 0, 80, 28);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let expanded = buffer_text(&buffer);
    assert!(expanded.contains("old value"));
    assert!(expanded.contains("new value"));
    let (column, row) = find_text(&buffer, "Edit lib.rs")
        .unwrap_or_else(|| panic!("edit header is visible:\n{expanded}"));

    for _ in 0..2 {
        state.handle_scrollback_mouse(mouse(MouseEventKind::Down(MouseButton::Left), column, row));
        state.handle_scrollback_mouse(mouse(MouseEventKind::Up(MouseButton::Left), column, row));
    }

    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let collapsed = buffer_text(&buffer);
    assert!(!collapsed.contains("old value"));
    assert!(!collapsed.contains("new value"));
    assert!(collapsed.contains("Edit lib.rs +1/-1"));
    insta::assert_snapshot!(
        "edit_diff_mouse_fold_surface",
        format!("EXPANDED\n{expanded}\n\nCOLLAPSED\n{collapsed}")
    );
}

#[test]
fn enter_opens_the_selected_edit_in_a_scrollable_viewer_snapshot() {
    let mut session = session_state();
    session.thread.turns[0].items.push(ThreadItem::FileChange {
        id: "edit-viewer".to_string(),
        changes: vec![FileUpdateChange {
            path: "src/lib.rs".to_string(),
            kind: PatchChangeKind::Update { move_path: None },
            diff: [
                "@@ -1,3 +1,6 @@",
                " pub fn render() {",
                "-    draw_old();",
                "+    draw_header();",
                "+    draw_body();",
                " }",
                "+",
                "+pub fn close() {}",
            ]
            .join("\n"),
        }],
        status: PatchApplyStatus::Completed,
    });
    let mut state = SurfaceState::from_session(&session);
    let area = Rect::new(0, 0, 80, 24);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    assert!(state.focus_scrollback());

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ),
        InputAction::Redraw
    );
    assert!(state.block_viewer().is_some());
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let viewer = buffer_text(&buffer);
    assert!(viewer.contains("Edit lib.rs"));
    assert!(viewer.contains("draw_header"));
    assert!(viewer.contains("Esc close"));
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
        ),
        InputAction::Redraw
    );
    for _ in 0..2 {
        assert_eq!(
            handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            InputAction::Redraw
        );
    }
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    let visual_selection_mask = selection_mask(&buffer, state.theme().panel_selected);
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)
        ),
        InputAction::CopyText {
            text: [
                "--- a/src/lib.rs",
                "+++ b/src/lib.rs",
                "@@ -1,1 +1,1 @@",
                " pub fn render() {",
                "",
            ]
            .join("\n"),
            notice: "Copied block content".to_string(),
        }
    );
    assert_eq!(
        handle_input_mouse(
            &mut state,
            mouse(MouseEventKind::Down(MouseButton::Left), 8, 5),
        ),
        InputAction::Redraw
    );
    assert_eq!(
        handle_input_mouse(
            &mut state,
            mouse(MouseEventKind::Drag(MouseButton::Left), 15, 5),
        ),
        InputAction::Redraw
    );
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    insta::assert_snapshot!(
        "edit_block_viewer_surface",
        format!(
            "{}\n\nvisual selection mask:\n{}\n\nmouse drag selection mask:\n{}",
            buffer_text(&buffer),
            visual_selection_mask,
            selection_mask(&buffer, state.theme().text_primary)
        )
    );

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        ),
        InputAction::Redraw
    );
    for character in "draw".chars() {
        assert_eq!(
            handle_key(
                &mut state,
                KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
            ),
            InputAction::Redraw
        );
    }
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    insta::assert_snapshot!("edit_block_viewer_search_surface", buffer_text(&buffer));

    assert_eq!(
        handle_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE),
        ),
        InputAction::Redraw
    );
    for character in "draw".chars() {
        assert_eq!(
            handle_key(
                &mut state,
                KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
            ),
            InputAction::Redraw
        );
    }
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ),
        InputAction::Redraw
    );
    render_surface_with_view(
        &mut state,
        &session,
        TranscriptView::Full,
        area,
        &mut buffer,
    );
    insta::assert_snapshot!("edit_block_viewer_filter_surface", buffer_text(&buffer));

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
        ),
        InputAction::Redraw
    );
    assert!(state.block_viewer().is_none());
}

#[test]
fn user_prompt_does_not_advertise_or_open_a_block_viewer_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    render_at_size(&mut state, &session, 80, 18);
    assert!(state.focus_scrollback());
    state.move_entry_selection(-1);

    let rendered = render_at_size(&mut state, &session, 80, 18);
    assert!(!rendered.contains("Enter:open"));
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ),
        InputAction::None
    );
    assert!(state.block_viewer().is_none());

    let area = Rect::new(0, 0, 80, 18);
    let mut buffer = Buffer::empty(area);
    render_surface(&mut state, &session, area, &mut buffer);
    let (column, row) =
        find_text(&buffer, "inspect this repo").expect("selected user prompt is visible");
    let mut second_release = crate::view::ScrollbackMouseAction::Ignored;
    for _ in 0..2 {
        state.handle_scrollback_mouse(mouse(MouseEventKind::Down(MouseButton::Left), column, row));
        second_release = state.handle_scrollback_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            column,
            row,
        ));
    }
    assert_eq!(second_release, crate::view::ScrollbackMouseAction::Ignored);
    assert!(state.block_viewer().is_none());
    insta::assert_snapshot!("user_prompt_selection_without_viewer_surface", rendered);
}

#[test]
fn plan_review_mouse_selects_and_activates_a_decision_row() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.note_completed_plan("turn-1", "# Plan\n- implement");
    assert!(state.maybe_open_plan_review("turn-1", ModeKind::Plan));

    let area = Rect::new(0, 0, 96, 24);
    let mut buffer = Buffer::empty(area);
    render_surface(&mut state, &session, area, &mut buffer);
    let (column, first_choice_row) =
        find_text(&buffer, "Yes, implement this plan").expect("plan decision row");
    let second_choice_row = first_choice_row.saturating_add(1);
    assert!(state.focus_scrollback());

    assert_eq!(
        handle_input_mouse(
            &mut state,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                column,
                second_choice_row,
            ),
        ),
        InputAction::Redraw
    );
    assert!(!state.scrollback_focused());
    assert_eq!(
        handle_input_mouse(
            &mut state,
            mouse(
                MouseEventKind::Up(MouseButton::Left),
                column,
                second_choice_row,
            ),
        ),
        InputAction::Plan(PlanReviewAction::ImplementFresh {
            plan: "# Plan\n- implement".to_string(),
        })
    );
    assert!(state.plan_review().is_none());
}

#[test]
fn scroll_offset_moves_in_both_directions() {
    let mut state = SurfaceState::new("thread-1");
    state.scroll_up(/*lines*/ 20);
    state.scroll_down(/*lines*/ 7);
    assert_eq!(state.scroll_offset(), 13);
    state.scroll_to_bottom();
    assert_eq!(state.scroll_offset(), 0);
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn find_symbol(buffer: &Buffer, symbol: &str) -> Option<(u16, u16)> {
    let area = buffer.area;
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if buffer[(x, y)].symbol() == symbol {
                return Some((x, y));
            }
        }
    }
    None
}

fn find_text(buffer: &Buffer, text: &str) -> Option<(u16, u16)> {
    let area = buffer.area;
    for y in area.y..area.bottom() {
        let line = (area.x..area.right())
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if let Some(byte_offset) = line.find(text) {
            let column_offset: u16 = line[..byte_offset].chars().count().try_into().ok()?;
            return Some((area.x.saturating_add(column_offset), y));
        }
    }
    None
}

fn selection_mask(buffer: &Buffer, selection_background: ratatui::style::Color) -> String {
    let area = buffer.area;
    (area.y..area.bottom())
        .map(|y| {
            let mask = (area.x..area.right())
                .map(|x| {
                    if buffer[(x, y)].bg == selection_background {
                        '^'
                    } else {
                        ' '
                    }
                })
                .collect::<String>();
            mask.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

fn request(value: serde_json::Value) -> ServerRequest {
    serde_json::from_value(value).expect("valid server request")
}

fn request_surface(value: serde_json::Value, composer: &str) -> String {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.pending_requests_mut().note(request(value));
    state.set_composer(composer);
    let area = Rect::new(0, 0, 72, 18);
    let mut buffer = Buffer::empty(area);
    render_surface(&mut state, &session, area, &mut buffer);
    buffer_text(&buffer)
}

fn render_at_size(
    state: &mut SurfaceState,
    session: &SessionState,
    width: u16,
    height: u16,
) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    render_surface(state, session, area, &mut buffer);
    buffer_text(&buffer)
}

fn named_working_surface() -> (SurfaceState, SessionState) {
    let mut session = session_state();
    session.thread.name = Some("Astral session".to_string());
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Working);
    state.set_composer("trace the projection");
    (state, session)
}

fn buffer_text(buffer: &Buffer) -> String {
    let area = buffer.area;
    (area.y..area.y + area.height)
        .map(|y| {
            let mut line = String::new();
            for x in area.x..area.x + area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}
