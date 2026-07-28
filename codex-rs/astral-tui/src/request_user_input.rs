//! Stateful interaction for app-server `request_user_input` requests.
//!
//! The request editor is intentionally separate from the primary prompt
//! composer. Approvals and questions can therefore arrive while the user has a
//! draft without overwriting that draft.

mod pointer;
mod state;

use std::collections::HashMap;

use codex_app_server_protocol::ToolRequestUserInputAnswer;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
use codex_app_server_protocol::ToolRequestUserInputResponse;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::composer::ComposerState;

use self::pointer::RequestUserInputPointerState;
pub(crate) use self::state::has_options;
pub(crate) use self::state::option_count;
pub(crate) use self::state::option_label;

#[cfg(test)]
#[path = "request_user_input_tests.rs"]
mod tests;

pub(crate) const OTHER_OPTION_LABEL: &str = "None of the above";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Options,
    Notes,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AnswerState {
    selected_option: Option<usize>,
    draft: String,
    committed: bool,
    notes_visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmationChoice {
    GoBack,
    Proceed,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RequestUserInputEvent {
    None,
    Redraw,
    Submit(ToolRequestUserInputResponse),
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestUserInputHit {
    Option(usize),
    Editor,
    Confirmation(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RequestUserInputState {
    request: Option<ToolRequestUserInputParams>,
    answers: Vec<AnswerState>,
    current_question: usize,
    focus: Focus,
    editor: ComposerState,
    confirmation: Option<ConfirmationChoice>,
    pointer: RequestUserInputPointerState,
}

impl Default for RequestUserInputState {
    fn default() -> Self {
        Self {
            request: None,
            answers: Vec::new(),
            current_question: 0,
            focus: Focus::Options,
            editor: ComposerState::default(),
            confirmation: None,
            pointer: RequestUserInputPointerState::default(),
        }
    }
}

impl RequestUserInputState {
    pub(crate) fn sync(&mut self, params: &ToolRequestUserInputParams) {
        if self.request.as_ref() == Some(params) {
            return;
        }

        self.request = Some(params.clone());
        self.answers = params
            .questions
            .iter()
            .map(|question| {
                let has_options = has_options(question);
                AnswerState {
                    selected_option: has_options.then_some(0),
                    notes_visible: !has_options,
                    ..AnswerState::default()
                }
            })
            .collect();
        self.current_question = 0;
        self.focus = if params.questions.first().is_some_and(has_options) {
            Focus::Options
        } else {
            Focus::Notes
        };
        self.editor.clear();
        self.confirmation = None;
        self.pointer.reset();
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn current_question(&self) -> usize {
        self.current_question
    }

    pub(crate) fn selected_option(&self) -> Option<usize> {
        self.current_answer()
            .and_then(|answer| answer.selected_option)
    }

    pub(crate) fn option_committed(&self) -> bool {
        self.current_answer().is_some_and(|answer| answer.committed)
    }

    pub(crate) fn notes_visible(&self) -> bool {
        self.current_answer()
            .is_some_and(|answer| answer.notes_visible)
    }

    pub(crate) fn editor(&self) -> &str {
        self.editor.text()
    }

    pub(crate) fn editor_cursor(&self) -> usize {
        self.editor.cursor()
    }

    pub(crate) fn confirmation_choice(&self) -> Option<usize> {
        self.confirmation.map(|choice| match choice {
            ConfirmationChoice::GoBack => 0,
            ConfirmationChoice::Proceed => 1,
        })
    }

    pub(crate) fn unanswered_count(&self, params: &ToolRequestUserInputParams) -> usize {
        params
            .questions
            .iter()
            .enumerate()
            .filter(|(index, question)| !self.is_answered(*index, question))
            .count()
    }

    pub(crate) fn handle_key(
        &mut self,
        params: &ToolRequestUserInputParams,
        key: KeyEvent,
    ) -> RequestUserInputEvent {
        self.sync(params);
        if self.confirmation.is_some() {
            return self.handle_confirmation(params, key);
        }
        if key.code == KeyCode::Char('X') && key.modifiers == KeyModifiers::SHIFT {
            return RequestUserInputEvent::Cancel;
        }
        if key.code == KeyCode::Esc {
            if self.focus == Focus::Notes
                && params
                    .questions
                    .get(self.current_question)
                    .is_some_and(has_options)
            {
                self.clear_notes();
                return RequestUserInputEvent::Redraw;
            }
            return RequestUserInputEvent::Cancel;
        }
        if self.question_count() == 0 {
            return RequestUserInputEvent::Submit(ToolRequestUserInputResponse {
                answers: HashMap::new(),
            });
        }

        if self.handle_question_navigation(params, key) {
            return RequestUserInputEvent::Redraw;
        }

        let Some(question) = params.questions.get(self.current_question) else {
            return RequestUserInputEvent::None;
        };
        match self.focus {
            Focus::Options => self.handle_option_key(params, question, key),
            Focus::Notes => self.handle_notes_key(params, question, key),
        }
    }

    pub(crate) fn handle_paste(&mut self, params: &ToolRequestUserInputParams, text: &str) -> bool {
        self.sync(params);
        if text.is_empty() || self.confirmation.is_some() {
            return false;
        }
        if params
            .questions
            .get(self.current_question)
            .is_some_and(has_options)
        {
            self.focus = Focus::Notes;
            if let Some(answer) = self.current_answer_mut() {
                answer.notes_visible = true;
                answer.committed = false;
            }
        }
        self.editor.insert_text(text);
        true
    }

    fn handle_confirmation(
        &mut self,
        params: &ToolRequestUserInputParams,
        key: KeyEvent,
    ) -> RequestUserInputEvent {
        match key.code {
            KeyCode::Esc => {
                self.confirmation = None;
                RequestUserInputEvent::Redraw
            }
            KeyCode::Up | KeyCode::Down | KeyCode::Char('j' | 'k') => {
                self.confirmation = self.confirmation.map(|choice| match choice {
                    ConfirmationChoice::GoBack => ConfirmationChoice::Proceed,
                    ConfirmationChoice::Proceed => ConfirmationChoice::GoBack,
                });
                RequestUserInputEvent::Redraw
            }
            KeyCode::Enter if self.confirmation == Some(ConfirmationChoice::Proceed) => {
                RequestUserInputEvent::Submit(self.response(params))
            }
            KeyCode::Enter => {
                self.confirmation = None;
                if let Some(index) =
                    params
                        .questions
                        .iter()
                        .enumerate()
                        .find_map(|(index, question)| {
                            (!self.is_answered(index, question)).then_some(index)
                        })
                {
                    self.move_to(params, index);
                }
                RequestUserInputEvent::Redraw
            }
            _ => RequestUserInputEvent::None,
        }
    }

    fn handle_question_navigation(
        &mut self,
        params: &ToolRequestUserInputParams,
        key: KeyEvent,
    ) -> bool {
        let previous = key.code == KeyCode::PageUp
            || (key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL))
            || (self.focus == Focus::Options && key.code == KeyCode::Left);
        let next = key.code == KeyCode::PageDown
            || (key.code == KeyCode::Char('n') && key.modifiers.contains(KeyModifiers::CONTROL))
            || (self.focus == Focus::Options && key.code == KeyCode::Right);
        if !previous && !next {
            return false;
        }
        let count = self.question_count();
        if count < 2 {
            return false;
        }
        let target = if previous {
            (self.current_question + count - 1) % count
        } else {
            (self.current_question + 1) % count
        };
        self.move_to(params, target);
        true
    }

    fn handle_option_key(
        &mut self,
        params: &ToolRequestUserInputParams,
        question: &ToolRequestUserInputQuestion,
        key: KeyEvent,
    ) -> RequestUserInputEvent {
        let option_count = option_count(question);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_option(option_count, /*next*/ false);
                RequestUserInputEvent::Redraw
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_option(option_count, /*next*/ true);
                RequestUserInputEvent::Redraw
            }
            KeyCode::Backspace | KeyCode::Delete => {
                if let Some(answer) = self.current_answer_mut() {
                    answer.selected_option = None;
                    answer.committed = false;
                    answer.draft.clear();
                    answer.notes_visible = false;
                }
                self.editor.clear();
                RequestUserInputEvent::Redraw
            }
            KeyCode::Tab if self.selected_option().is_some() => {
                self.focus = Focus::Notes;
                if let Some(answer) = self.current_answer_mut() {
                    answer.notes_visible = true;
                }
                RequestUserInputEvent::Redraw
            }
            KeyCode::Char(' ') => {
                if let Some(answer) = self.current_answer_mut() {
                    answer.committed = answer.selected_option.is_some();
                }
                RequestUserInputEvent::Redraw
            }
            KeyCode::Enter => {
                if let Some(answer) = self.current_answer_mut() {
                    answer.committed = answer.selected_option.is_some();
                }
                self.advance_or_submit(params)
            }
            KeyCode::Char(character) => {
                let Some(index) = character
                    .to_digit(10)
                    .and_then(|digit| digit.checked_sub(1))
                    .map(|index| index as usize)
                    .filter(|index| *index < option_count)
                else {
                    return RequestUserInputEvent::None;
                };
                if let Some(answer) = self.current_answer_mut() {
                    answer.selected_option = Some(index);
                    answer.committed = true;
                }
                self.advance_or_submit(params)
            }
            _ => RequestUserInputEvent::None,
        }
    }

    fn handle_notes_key(
        &mut self,
        params: &ToolRequestUserInputParams,
        question: &ToolRequestUserInputQuestion,
        key: KeyEvent,
    ) -> RequestUserInputEvent {
        if has_options(question) && key.code == KeyCode::Tab {
            self.save_editor();
            self.focus = Focus::Options;
            return RequestUserInputEvent::Redraw;
        }
        if key.code == KeyCode::Enter
            && !key
                .modifiers
                .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
        {
            self.save_editor();
            if let Some(answer) = self.current_answer_mut() {
                answer.committed = !has_options(question) || answer.selected_option.is_some();
            }
            return self.advance_or_submit(params);
        }
        if self.editor.edit_key(key) {
            if let Some(answer) = self.current_answer_mut() {
                answer.committed = false;
            }
            RequestUserInputEvent::Redraw
        } else {
            RequestUserInputEvent::None
        }
    }

    fn advance_or_submit(&mut self, params: &ToolRequestUserInputParams) -> RequestUserInputEvent {
        if self.current_question + 1 < self.question_count() {
            self.move_to(params, self.current_question + 1);
            return RequestUserInputEvent::Redraw;
        }
        self.save_editor();
        if self.unanswered_count(params) == 0 {
            RequestUserInputEvent::Submit(self.response(params))
        } else {
            self.confirmation = Some(ConfirmationChoice::GoBack);
            RequestUserInputEvent::Redraw
        }
    }

    fn move_option(&mut self, option_count: usize, next: bool) {
        if option_count == 0 {
            return;
        }
        if let Some(answer) = self.current_answer_mut() {
            let current = answer.selected_option.unwrap_or(0).min(option_count - 1);
            answer.selected_option = Some(if next {
                (current + 1) % option_count
            } else {
                (current + option_count - 1) % option_count
            });
            answer.committed = false;
        }
    }

    fn move_to(&mut self, params: &ToolRequestUserInputParams, index: usize) {
        if index >= self.question_count() {
            return;
        }
        self.save_editor();
        self.current_question = index;
        self.editor.replace(
            self.current_answer()
                .map(|answer| answer.draft.clone())
                .unwrap_or_default(),
        );
        self.focus = if params.questions.get(index).is_some_and(has_options) {
            Focus::Options
        } else {
            Focus::Notes
        };
        self.pointer.clear_click();
    }

    fn save_editor(&mut self) {
        let draft = self.editor.text().to_string();
        if let Some(answer) = self.current_answer_mut() {
            answer.draft = draft;
        }
    }

    fn response(&self, params: &ToolRequestUserInputParams) -> ToolRequestUserInputResponse {
        let answers = params
            .questions
            .iter()
            .enumerate()
            .map(|(index, question)| {
                let state = self.answers.get(index);
                let mut values = state
                    .filter(|state| state.committed)
                    .and_then(|state| state.selected_option)
                    .and_then(|selected| option_label(question, selected))
                    .into_iter()
                    .collect::<Vec<_>>();
                if let Some(note) = state
                    .filter(|state| state.committed)
                    .map(|state| state.draft.trim())
                    .filter(|note| !note.is_empty())
                {
                    if has_options(question) {
                        values.push(format!("user_note: {note}"));
                    } else {
                        values.push(note.to_string());
                    }
                }
                (
                    question.id.clone(),
                    ToolRequestUserInputAnswer { answers: values },
                )
            })
            .collect();
        ToolRequestUserInputResponse { answers }
    }

    fn is_answered(&self, index: usize, question: &ToolRequestUserInputQuestion) -> bool {
        let Some(answer) = self.answers.get(index) else {
            return false;
        };
        if has_options(question) {
            answer.committed && answer.selected_option.is_some()
        } else {
            answer.committed && !answer.draft.trim().is_empty()
        }
    }
}
