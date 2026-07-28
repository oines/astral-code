use codex_app_server_protocol::ToolRequestUserInputQuestion;

use super::AnswerState;
use super::Focus;
use super::OTHER_OPTION_LABEL;
use super::RequestUserInputState;

impl RequestUserInputState {
    pub(super) fn clear_notes(&mut self) {
        self.editor.clear();
        if let Some(answer) = self.current_answer_mut() {
            answer.draft.clear();
            answer.committed = false;
            answer.notes_visible = false;
        }
        self.focus = Focus::Options;
    }

    pub(super) fn question_count(&self) -> usize {
        self.answers.len()
    }

    pub(super) fn current_answer(&self) -> Option<&AnswerState> {
        self.answers.get(self.current_question)
    }

    pub(super) fn current_answer_mut(&mut self) -> Option<&mut AnswerState> {
        self.answers.get_mut(self.current_question)
    }
}

pub(crate) fn has_options(question: &ToolRequestUserInputQuestion) -> bool {
    question
        .options
        .as_ref()
        .is_some_and(|options| !options.is_empty())
}

pub(crate) fn option_count(question: &ToolRequestUserInputQuestion) -> usize {
    question.options.as_ref().map(Vec::len).unwrap_or_default()
        + usize::from(question.is_other && has_options(question))
}

pub(crate) fn option_label(
    question: &ToolRequestUserInputQuestion,
    index: usize,
) -> Option<String> {
    let options = question.options.as_ref()?;
    if let Some(option) = options.get(index) {
        return Some(option.label.clone());
    }
    (question.is_other && index == options.len()).then(|| OTHER_OPTION_LABEL.to_string())
}
