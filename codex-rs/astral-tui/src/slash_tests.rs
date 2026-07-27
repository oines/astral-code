use pretty_assertions::assert_eq;

use super::SlashCommandId;
use super::SlashController;
use super::SlashError;

#[test]
fn fuzzy_query_ranks_compact_and_exposes_ghost_text() {
    let mut controller = SlashController::default();
    controller.refresh("/cmp", false);
    assert_eq!(
        controller.snapshot().selection().map(|row| row.command),
        Some(SlashCommandId::Compact)
    );

    controller.refresh("/mo", false);
    assert_eq!(controller.snapshot().ghost.as_deref(), Some("del"));
}

#[test]
fn navigation_wraps_and_accepts_the_selected_command() {
    let mut controller = SlashController::default();
    controller.refresh("/", false);
    controller.move_selection(-1);
    assert_eq!(
        controller.snapshot().selection().map(|row| row.command),
        Some(SlashCommandId::Quit)
    );

    let composer = controller
        .accept_selection(false)
        .expect("selected command should complete");
    assert_eq!(composer, "/quit");
}

#[test]
fn invocation_keeps_typed_arguments_and_validation() {
    let mut controller = SlashController::default();
    controller.refresh("/rename release prep", false);
    assert_eq!(
        controller
            .invocation("/rename release prep", false)
            .expect("known command")
            .map(|invocation| (invocation.command, invocation.args)),
        Some((SlashCommandId::Rename, "release prep".to_string()))
    );
    assert_eq!(
        controller.invocation("/rename", false),
        Err(SlashError::MissingArgument {
            command: "rename".to_string(),
            placeholder: "name",
        })
    );
    assert_eq!(
        controller.invocation("/model", true),
        Err(SlashError::Unavailable("model".to_string()))
    );
}

#[test]
fn recently_used_commands_rank_first_for_an_empty_query() {
    let mut controller = SlashController::default();
    controller.record(SlashCommandId::Compact);
    controller.refresh("/", false);
    assert_eq!(
        controller.snapshot().selection().map(|row| row.command),
        Some(SlashCommandId::Compact)
    );
}
