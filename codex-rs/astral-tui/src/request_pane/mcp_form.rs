use crate::mcp_form::McpFormControl;
use crate::mcp_form::McpFormHit;
use crate::mcp_form::McpFormState;

use super::PaneHit;
use super::PaneRow;
use super::input_cursor_width;
use super::option_row::OptionMarker;
use super::push_visible_options;

const TEXT_HINTS: &[(&str, &str)] = &[
    ("Enter", "next"),
    ("Ctrl+P/N", "field"),
    ("Ctrl+D", "decline"),
    ("Esc", "cancel"),
];
const SINGLE_SELECT_HINTS: &[(&str, &str)] = &[
    ("Space", "choose"),
    ("Enter", "next"),
    ("Ctrl+D", "decline"),
    ("Esc", "cancel"),
];
const MULTI_SELECT_HINTS: &[(&str, &str)] = &[
    ("Space", "toggle"),
    ("Enter", "next"),
    ("Ctrl+D", "decline"),
    ("Esc", "cancel"),
];
const EMPTY_HINTS: &[(&str, &str)] = &[
    ("Enter", "accept"),
    ("Ctrl+D", "decline"),
    ("Esc", "cancel"),
];

pub(super) fn shortcuts(state: &McpFormState) -> &'static [(&'static str, &'static str)] {
    match state.current_field().map(|field| &field.control) {
        Some(McpFormControl::Text { .. }) => TEXT_HINTS,
        Some(McpFormControl::Select { multiple: true, .. }) => MULTI_SELECT_HINTS,
        Some(McpFormControl::Select {
            multiple: false, ..
        }) => SINGLE_SELECT_HINTS,
        None => EMPTY_HINTS,
    }
}

pub(super) fn push_content(
    rows: &mut Vec<PaneRow>,
    server_name: &str,
    message: &str,
    state: &McpFormState,
    max_rows: u16,
) -> bool {
    rows.push(PaneRow::Title(format!("{server_name} needs input")));
    rows.push(PaneRow::Body(message.to_string()));
    let Some(field) = state.current_field() else {
        rows.push(PaneRow::Blank);
        rows.push(PaneRow::Body(
            "No fields requested · press Enter to continue".to_string(),
        ));
        return false;
    };

    rows.push(PaneRow::Blank);
    let required = if field.schema.required {
        " · required"
    } else {
        ""
    };
    rows.push(PaneRow::Title(format!(
        "{} · {}{required} · {}/{}",
        field.schema.title,
        field.schema.kind.label(),
        state.current_index() + 1,
        state.field_count()
    )));
    if let Some(description) = &field.schema.description {
        rows.push(PaneRow::Body(description.clone()));
    }

    let mut input = false;
    match &field.control {
        McpFormControl::Text { .. } => {
            rows.push(PaneRow::Input {
                hit: Some(PaneHit::McpForm(McpFormHit::Editor)),
                text: state.editor().to_string(),
                cursor_column: input_cursor_width(
                    state.editor(),
                    state.editor_cursor(),
                    /*secret*/ false,
                ),
            });
            input = true;
        }
        McpFormControl::Select {
            choices,
            cursor,
            selected,
            multiple,
        } => {
            let options = choices
                .iter()
                .enumerate()
                .map(|(index, choice)| PaneRow::Option {
                    hit: Some(PaneHit::McpForm(McpFormHit::Choice(index))),
                    marker: if *multiple {
                        OptionMarker::Checkbox
                    } else {
                        OptionMarker::Radio
                    },
                    label: choice.label.clone(),
                    detail: None,
                    selected: *cursor == index,
                    committed: selected.contains(&index),
                })
                .collect();
            push_options(rows, options, *cursor, max_rows, state.error().is_some());
        }
    }
    if let Some(error) = state.error() {
        rows.push(PaneRow::Error(error.to_string()));
    }
    input
}

fn push_options(
    rows: &mut Vec<PaneRow>,
    options: Vec<PaneRow>,
    cursor: usize,
    max_rows: u16,
    has_error: bool,
) {
    let capacity = usize::from(max_rows)
        .saturating_sub(rows.len() + usize::from(has_error))
        .max(1);
    push_visible_options(rows, options, cursor, capacity);
}
