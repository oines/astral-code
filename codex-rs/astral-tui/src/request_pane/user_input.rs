use codex_app_server_protocol::ToolRequestUserInputParams;

use crate::request_user_input::OTHER_OPTION_LABEL;
use crate::request_user_input::RequestUserInputState;
use crate::request_user_input::has_options;
use crate::request_user_input::option_count;

use super::PaneRow;
use super::input_cursor_width;
use super::push_visible_options;

const OPTION_HINTS: &[(&str, &str)] = &[
    ("↑/↓", "navigate"),
    ("Enter", "select"),
    ("Tab", "notes"),
    ("Esc", "cancel"),
];
const TEXT_HINTS: &[(&str, &str)] = &[
    ("Enter", "next"),
    ("Ctrl+P/N", "question"),
    ("Esc", "cancel"),
];
const CONFIRM_HINTS: &[(&str, &str)] =
    &[("↑/↓", "navigate"), ("Enter", "confirm"), ("Esc", "back")];

pub(super) fn shortcuts(
    params: &ToolRequestUserInputParams,
    state: &RequestUserInputState,
) -> &'static [(&'static str, &'static str)] {
    if state.confirmation_choice().is_some() {
        CONFIRM_HINTS
    } else if params
        .questions
        .get(state.current_question())
        .is_some_and(has_options)
        && !state.notes_visible()
    {
        OPTION_HINTS
    } else {
        TEXT_HINTS
    }
}

pub(super) fn push_content(
    rows: &mut Vec<PaneRow>,
    params: &ToolRequestUserInputParams,
    state: &RequestUserInputState,
    max_rows: u16,
) -> bool {
    if let Some(choice) = state.confirmation_choice() {
        let unanswered = state.unanswered_count(params);
        rows.push(PaneRow::Title(
            "Submit with unanswered questions?".to_string(),
        ));
        rows.push(PaneRow::Body(format!(
            "{unanswered} question{} unanswered",
            if unanswered == 1 { "" } else { "s" }
        )));
        rows.push(PaneRow::Blank);
        rows.extend([
            PaneRow::Option {
                label: "Go back".to_string(),
                detail: Some("Return to the first unanswered question".to_string()),
                selected: choice == 0,
                committed: false,
            },
            PaneRow::Option {
                label: "Proceed".to_string(),
                detail: Some("Submit empty answers where needed".to_string()),
                selected: choice == 1,
                committed: false,
            },
        ]);
        return false;
    }

    let Some(question) = params.questions.get(state.current_question()) else {
        return false;
    };
    let current = state.current_question();
    let counter = if params.questions.len() > 1 {
        format!(" · {}/{}", current + 1, params.questions.len())
    } else {
        String::new()
    };
    rows.push(PaneRow::Title(format!("{}{counter}", question.header)));
    rows.push(PaneRow::Body(question.question.clone()));
    if let Some(options) = &question.options {
        let selected = state.selected_option();
        let committed = state.option_committed();
        let mut option_rows = options
            .iter()
            .enumerate()
            .map(|(index, option)| PaneRow::Option {
                label: format!("{}. {}", index + 1, option.label),
                detail: (!option.description.is_empty()).then(|| option.description.clone()),
                selected: selected == Some(index),
                committed: committed && selected == Some(index),
            })
            .collect::<Vec<_>>();
        if option_count(question) > options.len() {
            let index = options.len();
            option_rows.push(PaneRow::Option {
                label: format!("{}. {OTHER_OPTION_LABEL}", index + 1),
                detail: Some("Add details in notes if needed".to_string()),
                selected: selected == Some(index),
                committed: committed && selected == Some(index),
            });
        }
        let reserve_after_options = if state.notes_visible() { 2 } else { 0 };
        let capacity = usize::from(max_rows)
            .saturating_sub(rows.len() + reserve_after_options)
            .max(1);
        push_visible_options(rows, option_rows, selected.unwrap_or_default(), capacity);
    }

    if has_options(question) && !state.notes_visible() {
        return false;
    }
    rows.push(PaneRow::Blank);
    let secret = question.is_secret;
    let editor = state.editor();
    rows.push(PaneRow::Input {
        text: if secret {
            "•".repeat(editor.chars().count())
        } else {
            editor.to_string()
        },
        cursor_column: input_cursor_width(editor, state.editor_cursor(), secret),
    });
    true
}
