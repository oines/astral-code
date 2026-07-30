use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ThreadListResponse;
use codex_protocol::config_types::ModeKind;
use codex_terminal_detection::TerminalName;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use ratatui::layout::Rect;
use serde_json::json;

use super::InputAction;
use super::handle_key;
use super::handle_mouse;
use super::handle_paste;
use crate::RequestResolution;
use crate::SlashCommandId;
use crate::SlashInvocation;
use crate::SurfaceActivity;
use crate::SurfaceState;
use crate::macos_modifiers::ModifierState;
use crate::mention::MentionCandidate;
use crate::mention::MentionCatalog;
use crate::mention::MentionKind;
use crate::mention::MentionTarget;
use crate::modal::ModalRow;
use crate::modal::ModalState;
use crate::plan_review::PlanReviewAction;
use crate::plan_review::PlanReviewChoice;
use crate::plan_review::PlanReviewFocus;
use crate::plan_review::PlanReviewState;
use crate::thread_picker::PickerState;
use crate::thread_picker::ThreadPickerAction;
use crate::view::AstralThemeId;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn request(value: serde_json::Value) -> ServerRequest {
    serde_json::from_value(value).expect("valid server request")
}

#[test]
fn composer_submit_and_interrupt_are_distinct_actions() {
    let mut state = SurfaceState::new("thread-1");
    for character in "hello".chars() {
        assert_eq!(
            handle_key(&mut state, key(KeyCode::Char(character))),
            InputAction::Redraw
        );
    }
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Submit(crate::PromptSubmission {
            text: "hello".to_string(),
            elements: Vec::new(),
        })
    );

    state.set_activity(SurfaceActivity::Working);
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ),
        InputAction::Interrupt
    );
}

#[test]
fn prompt_history_browses_live_and_detaches_for_editing() {
    let mut state = SurfaceState::new("thread-1");
    state.record_submission(&crate::PromptSubmission::text_only("older prompt"));
    state.record_submission(&crate::PromptSubmission::text_only("newest prompt"));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Up)),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "newest prompt");
    assert!(state.history().open);

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Up)),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "older prompt");
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Down)),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "newest prompt");
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Down)),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "");
    assert!(!state.history().open);

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Up)),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('!'))),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "newest prompt!");
    assert!(!state.history().open);
}

#[test]
fn large_paste_chip_expands_on_enter_and_keeps_payload_through_history() {
    let mut state = SurfaceState::new("thread-1");
    let pasted = "one\ntwo\nthree\nfour";

    assert_eq!(handle_paste(&mut state, pasted), InputAction::Redraw);
    assert_eq!(state.composer(), "[Pasted: 4 lines]");
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Left)),
        InputAction::Redraw
    );
    assert_eq!(state.composer_cursor(), 0);
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), pasted);

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        ),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "[Pasted: 4 lines]");
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Right)),
        InputAction::Redraw
    );
    let InputAction::Submit(submission) = handle_key(&mut state, key(KeyCode::Enter)) else {
        panic!("enter after the chip should submit");
    };
    assert_eq!(
        submission.user_input(),
        vec![codex_app_server_protocol::UserInput::Text {
            text: pasted.to_string(),
            text_elements: Vec::new(),
        }]
    );

    state.record_submission(&submission);
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Up)),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "[Pasted: 4 lines]");
}

#[test]
fn dollar_completion_selects_a_skill_and_submits_structured_input() {
    let mut state = SurfaceState::new("thread-1");
    state.set_mention_catalog(MentionCatalog {
        candidates: vec![MentionCandidate {
            kind: MentionKind::Skill,
            display: "Review".to_string(),
            description: "Review changes".to_string(),
            insert_text: "$review".to_string(),
            search_terms: vec!["review".to_string()],
            target: MentionTarget::Skill {
                name: "review".to_string(),
                path: "/skills/review/SKILL.md".into(),
            },
        }],
    });
    for character in "$rev".chars() {
        assert_eq!(
            handle_key(&mut state, key(KeyCode::Char(character))),
            InputAction::Redraw
        );
    }
    assert!(state.mentions().open);

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "$review ");
    let InputAction::Submit(submission) = handle_key(&mut state, key(KeyCode::Enter)) else {
        panic!("selected skill should submit");
    };
    assert_eq!(
        submission.user_input(),
        vec![
            codex_app_server_protocol::UserInput::Text {
                text: "$review ".to_string(),
                text_elements: Vec::new(),
            },
            codex_app_server_protocol::UserInput::Skill {
                name: "review".to_string(),
                path: "/skills/review/SKILL.md".into(),
            },
        ]
    );
}

#[test]
fn transcript_shortcuts_are_distinct_from_composer_input() {
    let mut state = SurfaceState::new("thread-1");

    assert_eq!(
        handle_key(&mut state, key(KeyCode::PageUp)),
        InputAction::ScrollUp
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::PageDown)),
        InputAction::ScrollDown
    );
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
        ),
        InputAction::CopyLastResponse
    );
    assert!(state.composer().is_empty());
}

#[test]
fn shift_tab_cycles_mode_without_consuming_the_draft() {
    for key in [
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT),
    ] {
        let mut state = SurfaceState::new("thread-1");
        state.set_composer("keep this draft");
        assert_eq!(handle_key(&mut state, key), InputAction::CycleMode);
        assert_eq!(state.composer(), "keep this draft");
    }
}

#[test]
fn slash_completion_and_dispatch_stay_local_to_the_tui() {
    let mut state = SurfaceState::new("thread-1");
    for character in "/co".chars() {
        assert_eq!(
            handle_key(&mut state, key(KeyCode::Char(character))),
            InputAction::Redraw
        );
    }
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "/compact");
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Slash {
            invocation: SlashInvocation {
                command: SlashCommandId::Compact,
                name: "compact",
                args: String::new(),
            },
            submission: crate::PromptSubmission {
                text: "/compact".to_string(),
                elements: Vec::new(),
            },
        }
    );
    assert!(state.composer().is_empty());
}

#[test]
fn plan_command_keeps_the_inline_prompt_for_typed_dispatch() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("/plan inspect the renderer");

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Slash {
            invocation: SlashInvocation {
                command: SlashCommandId::Plan,
                name: "plan",
                args: "inspect the renderer".to_string(),
            },
            submission: crate::PromptSubmission {
                text: "/plan inspect the renderer".to_string(),
                elements: Vec::new(),
            },
        }
    );
    assert!(state.composer().is_empty());
}

#[test]
fn completed_plan_opens_review_and_enter_implements_without_losing_the_draft() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("keep this draft");
    state.note_completed_plan("turn-1", "# Plan\n- implement");

    assert!(state.maybe_open_plan_review("turn-1", ModeKind::Plan));
    assert!(state.composer().is_empty());
    assert_eq!(state.plan_review_focus(), Some(PlanReviewFocus::Decision));
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Plan(PlanReviewAction::Implement)
    );
    assert!(state.plan_review().is_none());
    assert_eq!(state.composer(), "keep this draft");
}

#[test]
fn plan_review_navigation_selects_keep_planning_without_submitting() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("keep this draft");
    state.note_completed_plan("turn-1", "# Plan\n- implement");
    assert!(state.maybe_open_plan_review("turn-1", ModeKind::Plan));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Down)),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('j'))),
        InputAction::Redraw
    );
    assert_eq!(
        state.plan_review().map(PlanReviewState::selection),
        Some(PlanReviewChoice::KeepPlanning)
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Redraw
    );
    assert!(state.plan_review().is_none());
    assert_eq!(state.composer(), "keep this draft");
}

#[test]
fn plan_review_fresh_context_carries_only_the_approved_plan() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("old thread draft");
    state.note_completed_plan("turn-1", "# Plan\n- implement");
    assert!(state.maybe_open_plan_review("turn-1", ModeKind::Plan));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('c'))),
        InputAction::Plan(PlanReviewAction::ImplementFresh {
            plan: "# Plan\n- implement".to_string(),
        })
    );
    assert!(state.plan_review().is_none());
    assert!(state.composer().is_empty());
}

#[test]
fn plan_review_revision_submits_feedback_and_stays_separate_from_the_draft() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("keep this draft");
    state.note_completed_plan("turn-1", "# Plan");
    assert!(state.maybe_open_plan_review("turn-1", ModeKind::Plan));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('s'))),
        InputAction::Redraw
    );
    assert_eq!(state.plan_review_focus(), Some(PlanReviewFocus::Revision));
    for character in "add tests".chars() {
        assert_eq!(
            handle_key(&mut state, key(KeyCode::Char(character))),
            InputAction::Redraw
        );
    }
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Plan(PlanReviewAction::Revise {
            feedback: crate::PromptSubmission::text_only("add tests"),
        })
    );
    assert!(state.plan_review().is_none());
    assert_eq!(state.composer(), "keep this draft");
}

#[test]
fn plan_review_only_opens_for_a_real_plan_in_plan_mode() {
    let mut state = SurfaceState::new("thread-1");
    assert!(!state.maybe_open_plan_review("turn-1", ModeKind::Plan));

    state.note_completed_plan("turn-1", "# Plan");
    assert!(!state.maybe_open_plan_review("turn-1", ModeKind::Default));
    assert!(state.plan_review().is_none());
}

#[test]
fn slash_errors_stay_local_to_the_tui() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("/does-not-exist");
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Notice("Unknown command: /does-not-exist".to_string())
    );
    assert_eq!(state.composer(), "/does-not-exist");

    state.set_composer("/model");
    state.set_activity(SurfaceActivity::Working);
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Notice("/model is unavailable while Astral is working".to_string())
    );

    state.set_composer("/compact");
    state.set_activity(SurfaceActivity::Disconnected("connection lost".to_string()));
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Notice("/compact requires an app-server connection".to_string())
    );
}

#[test]
fn modal_focus_blocks_composer_input_until_escape() {
    let mut state = SurfaceState::new("thread-1");
    state.open_modal(ModalState::info(
        "Session status",
        vec![ModalRow::new("Model", "gpt-5")],
    ));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('x'))),
        InputAction::None
    );
    assert!(state.composer().is_empty());
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Esc)),
        InputAction::Redraw
    );
    assert!(state.modal().is_none());
}

#[test]
fn modal_inventory_scrolls_without_touching_the_composer() {
    let mut state = SurfaceState::new("thread-1");
    state.open_modal(ModalState::info(
        "Skills",
        (0..20)
            .map(|index| ModalRow::new(format!("Skill {index}"), "enabled"))
            .collect(),
    ));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::PageDown)),
        InputAction::Redraw
    );
    assert_eq!(state.modal().map(|modal| modal.scroll_offset), Some(10));
    assert_eq!(
        handle_key(&mut state, key(KeyCode::End)),
        InputAction::Redraw
    );
    assert_eq!(state.modal().map(|modal| modal.scroll_offset), Some(19));
    assert!(state.composer().is_empty());
}

#[test]
fn shortcuts_toggle_is_global() {
    let mut state = SurfaceState::new("thread-1");
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('.'), KeyModifiers::CONTROL)
        ),
        InputAction::OpenShortcuts
    );
}

#[test]
fn sessions_shortcut_preserves_the_draft_and_uses_resume() {
    let mut state = SurfaceState::new("thread-1");
    state.set_activity(SurfaceActivity::Ready);
    state.set_composer("keep this draft");
    let shortcut = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);

    assert_eq!(
        handle_key(&mut state, shortcut),
        InputAction::Slash {
            invocation: SlashInvocation {
                command: SlashCommandId::Resume,
                name: "resume",
                args: String::new(),
            },
            submission: crate::PromptSubmission::text_only(String::new()),
        }
    );
    assert_eq!(state.composer(), "keep this draft");

    state.set_activity(SurfaceActivity::Working);
    assert_eq!(
        handle_key(&mut state, shortcut),
        InputAction::Notice("Session selection is unavailable while Astral is working".to_string())
    );
}

#[test]
fn new_session_shortcut_requires_a_second_press() {
    let mut state = SurfaceState::new("thread-1");
    state.set_activity(SurfaceActivity::Ready);
    state.set_composer("keep this draft");
    let shortcut = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);

    assert_eq!(handle_key(&mut state, shortcut), InputAction::Redraw);
    assert_eq!(
        state.pending_action(),
        Some(crate::actions::ActionId::NewSession)
    );
    assert_eq!(state.composer(), "keep this draft");

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('x'))),
        InputAction::Redraw
    );
    assert_eq!(state.pending_action(), None);
    assert_eq!(handle_key(&mut state, shortcut), InputAction::Redraw);
    assert_eq!(
        handle_key(&mut state, shortcut),
        InputAction::Slash {
            invocation: SlashInvocation {
                command: SlashCommandId::New,
                name: "new",
                args: String::new(),
            },
            submission: crate::PromptSubmission::text_only(String::new()),
        }
    );

    state.set_activity(SurfaceActivity::Working);
    assert_eq!(
        handle_key(&mut state, shortcut),
        InputAction::Notice(
            "Starting a new session is unavailable while Astral is working".to_string()
        )
    );
}

#[test]
fn command_palette_preserves_the_draft_while_collecting_required_arguments() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("keep this draft");

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
        ),
        InputAction::Redraw
    );
    assert_eq!(handle_paste(&mut state, "rename"), InputAction::Redraw);
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "/rename ");

    assert_eq!(handle_paste(&mut state, "new name"), InputAction::Redraw);
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Slash {
            invocation: SlashInvocation {
                command: SlashCommandId::Rename,
                name: "rename",
                args: "new name".to_string(),
            },
            submission: crate::PromptSubmission::text_only("/rename new name"),
        }
    );
    assert_eq!(state.composer(), "keep this draft");
}

#[test]
fn multiline_mode_swaps_enter_and_modified_enter() {
    let mut state = SurfaceState::new("thread-1");
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('m'), KeyModifiers::CONTROL),
        ),
        InputAction::ToggleMultiline
    );

    state.toggle_multiline_mode();
    state.set_composer("first line");
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "first line\n");

    state.set_composer("first line\nsecond line");
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
        ),
        InputAction::Submit(crate::PromptSubmission::text_only(
            "first line\nsecond line"
        ))
    );
}

#[test]
fn apple_terminal_recovers_enter_modifiers_dropped_by_the_pty() {
    let key = key(KeyCode::Enter);
    let held = ModifierState {
        shift: true,
        ..ModifierState::default()
    };

    assert!(super::terminal_support::is_modified_enter_for(
        &key,
        TerminalName::AppleTerminal,
        held,
    ));
    assert!(!super::terminal_support::is_modified_enter_for(
        &key,
        TerminalName::Ghostty,
        held,
    ));
}

#[test]
fn trailing_backslash_continues_the_prompt_instead_of_submitting() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("first line\\");

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "first line\n");

    state.composer_state_mut().insert_text("second line");
    let InputAction::Submit(submission) = handle_key(&mut state, key(KeyCode::Enter)) else {
        panic!("the continued prompt should submit after more text");
    };
    assert_eq!(submission.text(), "first line\nsecond line");
}

#[test]
fn shell_mode_runs_directly_and_restores_from_history() {
    let mut state = SurfaceState::new("thread-1");
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('!'), KeyModifiers::SHIFT),
        ),
        InputAction::Redraw
    );
    assert!(state.shell_input_mode());
    assert_eq!(state.composer(), "");
    assert_eq!(handle_key(&mut state, key(KeyCode::Up)), InputAction::None);

    state.toggle_multiline_mode();
    state.set_composer("printf shell-ok");
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::RunShellCommand {
            command: "printf shell-ok".to_string(),
        }
    );
    assert!(!state.shell_input_mode());

    state.record_submission(&crate::PromptSubmission::text_only("! printf shell-ok"));
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Up)),
        InputAction::Redraw
    );
    assert!(state.shell_input_mode());
    assert_eq!(state.composer(), "printf shell-ok");

    state.composer_state_mut().clear();
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Esc)),
        InputAction::Redraw
    );
    assert!(!state.shell_input_mode());
}

#[test]
fn theme_cancel_restores_the_original_preview() {
    let mut state = SurfaceState::new("thread-1");
    state.open_theme_picker();
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Down)),
        InputAction::Redraw
    );
    assert_eq!(state.theme_id(), AstralThemeId::Day);
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Esc)),
        InputAction::Redraw
    );
    assert_eq!(state.theme_id(), AstralThemeId::Night);
    assert!(state.theme_picker().is_none());
}

#[test]
fn theme_selection_reports_the_persisted_name() {
    let mut state = SurfaceState::new("thread-1");
    state.open_theme_picker();
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Down)),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::SelectTheme("astral-day".to_string())
    );
    assert_eq!(state.theme_id(), AstralThemeId::Day);
    assert!(state.theme_picker().is_none());
}

#[test]
fn thread_picker_owns_text_input_until_escape() {
    let mut state = SurfaceState::new("thread-1");
    state.open_thread_picker(PickerState::new(
        ThreadPickerAction::Resume,
        ThreadListResponse {
            data: Vec::new(),
            next_cursor: None,
            backwards_cursor: None,
        },
    ));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('x'))),
        InputAction::Redraw
    );
    assert_eq!(handle_paste(&mut state, "yz"), InputAction::Redraw);
    assert!(state.thread_picker().is_some());
    assert!(state.composer().is_empty());
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Esc)),
        InputAction::Redraw
    );
    assert!(state.thread_picker().is_none());
}

#[test]
fn command_session_approval_preserves_typed_decision() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("draft survives approval");
    state.pending_requests_mut().note(request(json!({
        "method": "item/commandExecution/requestApproval",
        "id": 7,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "startedAtMs": 100,
            "availableDecisions": ["accept", "acceptForSession", "decline", "cancel"]
        }
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('a'))),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::Integer(7),
            result: json!({"decision": "acceptForSession"}),
        })
    );
    assert_eq!(state.composer(), "draft survives approval");
    assert_eq!(state.pending_requests().len(), 1);
}

#[test]
fn approval_picker_navigation_resolves_the_selected_decision() {
    let mut state = SurfaceState::new("thread-1");
    state.pending_requests_mut().note(request(json!({
        "method": "item/commandExecution/requestApproval",
        "id": 71,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "startedAtMs": 100,
            "availableDecisions": ["accept", "acceptForSession", "decline", "cancel"]
        }
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Down)),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::Integer(71),
            result: json!({"decision": "acceptForSession"}),
        })
    );
}

#[test]
fn approval_picker_second_click_resolves_the_pointed_decision() {
    let mut state = SurfaceState::new("thread-1");
    state.pending_requests_mut().note(request(json!({
        "method": "item/commandExecution/requestApproval",
        "id": 72,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "startedAtMs": 100,
            "availableDecisions": ["accept", "decline", "cancel"]
        }
    })));
    state.sync_request_states();
    state
        .request_choice_mut()
        .observe_rows(vec![(1, Rect::new(2, 5, 20, 1))]);
    let click = mouse(MouseEventKind::Down(MouseButton::Left), 4, 5);

    assert_eq!(handle_mouse(&mut state, click), InputAction::Redraw);
    assert_eq!(
        handle_mouse(&mut state, click),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::Integer(72),
            result: json!({"decision": "decline"}),
        })
    );
}

#[test]
fn command_approval_respects_available_decisions() {
    let mut state = SurfaceState::new("thread-1");
    state.pending_requests_mut().note(request(json!({
        "method": "item/commandExecution/requestApproval",
        "id": 8,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "startedAtMs": 100,
            "availableDecisions": ["accept", "decline"]
        }
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('a'))),
        InputAction::None
    );
    assert_eq!(state.pending_requests().len(), 1);
}

#[test]
fn file_change_and_permission_approvals_preserve_scope() {
    let mut state = SurfaceState::new("thread-1");
    state.pending_requests_mut().note(request(json!({
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
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('a'))),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::String("edit-1".to_string()),
            result: json!({"decision": "acceptForSession"}),
        })
    );
    state.remove_pending_request(&RequestId::String("edit-1".to_string()));

    state.pending_requests_mut().note(request(json!({
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
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('a'))),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::String("permissions-1".to_string()),
            result: json!({
                "permissions": {
                    "network": {"enabled": true},
                    "fileSystem": {
                        "read": ["/workspace/generated"],
                        "write": null
                    }
                },
                "scope": "session"
            }),
        })
    );
}

#[test]
fn declining_permissions_rejects_the_request() {
    let mut state = SurfaceState::new("thread-1");
    state.pending_requests_mut().note(request(json!({
        "method": "item/permissions/requestApproval",
        "id": 9,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "environmentId": null,
            "startedAtMs": 102,
            "cwd": "/workspace",
            "reason": null,
            "permissions": {
                "network": null,
                "fileSystem": null
            }
        }
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('n'))),
        InputAction::Resolve(RequestResolution::Reject {
            request_id: RequestId::Integer(9),
            error: codex_app_server_protocol::JSONRPCErrorError {
                code: -32000,
                message: "permission request declined".to_string(),
                data: None,
            },
        })
    );
}

#[test]
fn mcp_form_and_url_elicitations_keep_typed_actions() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("keep this prompt draft");
    state.pending_requests_mut().note(request(json!({
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
                        "title": "Confirm changes"
                    },
                    "count": {
                        "type": "integer",
                        "title": "Retry count",
                        "minimum": 1,
                        "maximum": 5
                    },
                    "features": {
                        "type": "array",
                        "title": "Features",
                        "minItems": 1,
                        "items": {
                            "type": "string",
                            "enum": ["search", "edit"]
                        }
                    }
                },
                "required": ["confirmed", "count", "features"]
            }
        }
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char(' '))),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Redraw
    );
    assert_eq!(handle_paste(&mut state, "2.5"), InputAction::Redraw);
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Redraw
    );
    assert_eq!(state.mcp_form().error(), Some("Enter a whole number"));
    for _ in 0..3 {
        assert_eq!(
            handle_key(&mut state, key(KeyCode::Backspace)),
            InputAction::Redraw
        );
    }
    assert_eq!(handle_paste(&mut state, "3"), InputAction::Redraw);
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char(' '))),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Down)),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char(' '))),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::String("mcp-form".to_string()),
            result: json!({
                "action": "accept",
                "content": {
                    "confirmed": true,
                    "count": 3,
                    "features": ["search", "edit"]
                },
                "_meta": null
            }),
        })
    );
    assert_eq!(state.composer(), "keep this prompt draft");
    state.remove_pending_request(&RequestId::String("mcp-form".to_string()));

    state.pending_requests_mut().note(request(json!({
        "method": "mcpServer/elicitation/request",
        "id": "mcp-url",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "serverName": "astral",
            "mode": "url",
            "_meta": null,
            "message": "Open authorization page",
            "url": "https://example.com/auth",
            "elicitationId": "elicit-1"
        }
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('y'))),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::String("mcp-url".to_string()),
            result: json!({
                "action": "accept",
                "content": null,
                "_meta": null
            }),
        })
    );
    assert_eq!(state.composer(), "keep this prompt draft");
}

#[test]
fn user_input_supports_multiple_question_answers() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("keep this prompt draft");
    state.pending_requests_mut().note(request(json!({
        "method": "item/tool/requestUserInput",
        "id": "question-1",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "questions": [
                {
                    "id": "language",
                    "header": "Language",
                    "question": "Which language?",
                    "options": null
                },
                {
                    "id": "style",
                    "header": "Style",
                    "question": "Which style?",
                    "options": null
                }
            ]
        }
    })));
    for character in "Rust".chars() {
        assert_eq!(
            handle_key(&mut state, key(KeyCode::Char(character))),
            InputAction::Redraw
        );
    }
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Redraw
    );
    for character in "concise".chars() {
        assert_eq!(
            handle_key(&mut state, key(KeyCode::Char(character))),
            InputAction::Redraw
        );
    }

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::String("question-1".to_string()),
            result: json!({
                "answers": {
                    "language": {"answers": ["Rust"]},
                    "style": {"answers": ["concise"]}
                }
            }),
        })
    );
    assert_eq!(state.composer(), "keep this prompt draft");
}

#[test]
fn user_input_option_navigation_submits_the_selected_label() {
    let mut state = SurfaceState::new("thread-1");
    state.pending_requests_mut().note(request(json!({
        "method": "item/tool/requestUserInput",
        "id": "question-1",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "questions": [{
                "id": "scope",
                "header": "Scope",
                "question": "Which scope?",
                "isOther": false,
                "isSecret": false,
                "options": [
                    {"label": "Workspace", "description": "Only this repo"},
                    {"label": "Shared", "description": "Common runtime"}
                ]
            }]
        }
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Down)),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::String("question-1".to_string()),
            result: json!({"answers": {"scope": {"answers": ["Shared"]}}}),
        })
    );
}

#[test]
fn user_input_confirms_before_submitting_unanswered_questions() {
    let mut state = SurfaceState::new("thread-1");
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
    let next_question = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
    assert_eq!(handle_key(&mut state, next_question), InputAction::Redraw);
    assert_eq!(handle_paste(&mut state, "answered"), InputAction::Redraw);
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Redraw
    );
    assert_eq!(state.request_user_input().confirmation_choice(), Some(0));
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Down)),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::String("question-1".to_string()),
            result: json!({
                "answers": {
                    "first": {"answers": []},
                    "second": {"answers": ["answered"]}
                }
            }),
        })
    );
}
