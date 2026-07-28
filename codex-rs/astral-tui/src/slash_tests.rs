use pretty_assertions::assert_eq;

use super::SlashCommandId;
use super::SlashCommandState;
use super::SlashController;
use super::SlashError;

#[test]
fn fuzzy_query_ranks_compact_and_exposes_ghost_text() {
    let mut controller = SlashController::default();
    controller.refresh("/cmp", SlashCommandState::Idle);
    assert_eq!(
        controller.snapshot().selection().map(|row| row.command),
        Some(SlashCommandId::Compact)
    );

    controller.refresh("/mo", SlashCommandState::Idle);
    assert_eq!(controller.snapshot().ghost.as_deref(), Some("del"));
}

#[test]
fn navigation_wraps_and_accepts_the_selected_command() {
    let mut controller = SlashController::default();
    controller.refresh("/", SlashCommandState::Idle);
    controller.move_selection(-1);
    assert_eq!(
        controller.snapshot().selection().map(|row| row.command),
        Some(SlashCommandId::Quit)
    );

    let composer = controller
        .accept_selection(SlashCommandState::Idle)
        .expect("selected command should complete");
    assert_eq!(composer, "/quit");
}

#[test]
fn invocation_keeps_typed_arguments_and_validation() {
    let mut controller = SlashController::default();
    controller.refresh("/rename release prep", SlashCommandState::Idle);
    assert_eq!(
        controller
            .invocation("/rename release prep", SlashCommandState::Idle)
            .expect("known command")
            .map(|invocation| (invocation.command, invocation.args)),
        Some((SlashCommandId::Rename, "release prep".to_string()))
    );
    assert_eq!(
        controller.invocation("/rename", SlashCommandState::Idle),
        Err(SlashError::MissingArgument {
            command: "rename".to_string(),
            placeholder: "name",
        })
    );
    assert_eq!(
        controller.invocation("/model", SlashCommandState::Working),
        Err(SlashError::UnavailableWhileWorking("model".to_string()))
    );
    assert_eq!(
        controller
            .invocation("/plan inspect ordering", SlashCommandState::Idle)
            .expect("plan command")
            .map(|invocation| (invocation.command, invocation.args)),
        Some((SlashCommandId::Plan, "inspect ordering".to_string()))
    );
}

#[test]
fn command_availability_tracks_working_and_disconnected_states() {
    let mut controller = SlashController::default();
    controller.refresh("/model ", SlashCommandState::Working);
    assert_eq!(
        controller.snapshot(),
        &super::SlashSnapshot {
            active: true,
            title: "commands",
            query: "model".to_string(),
            ..super::SlashSnapshot::default()
        }
    );
    assert_eq!(
        controller.invocation("/compact", SlashCommandState::Disconnected),
        Err(SlashError::RequiresConnection("compact".to_string()))
    );

    controller.refresh("/", SlashCommandState::Disconnected);
    assert_eq!(
        controller
            .snapshot()
            .matches
            .iter()
            .map(|row| row.command)
            .collect::<Vec<_>>(),
        vec![
            SlashCommandId::Status,
            SlashCommandId::Copy,
            SlashCommandId::Theme,
            SlashCommandId::Timeline,
            SlashCommandId::Exit,
            SlashCommandId::Quit,
        ]
    );
}

#[test]
fn recently_used_commands_rank_first_for_an_empty_query() {
    let mut controller = SlashController::default();
    controller.record(SlashCommandId::Compact);
    controller.refresh("/", SlashCommandState::Idle);
    assert_eq!(
        controller.snapshot().selection().map(|row| row.command),
        Some(SlashCommandId::Compact)
    );
}
