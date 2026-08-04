use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ToolRequestUserInputAnswer;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
use codex_app_server_protocol::ToolRequestUserInputResponse;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Rect;

use crate::ModalOutcome;
use crate::ModalWindow;
use crate::prompt_interaction::PromptInteractionOutcome;
use crate::prompt_interaction::PromptInteractionSubmission;

mod render;

const OTHER_LABEL: &str = "None of the above";
const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Options,
    Notes,
}

#[derive(Clone, Debug, Default)]
struct AnswerState {
    cursor: usize,
    selected: Option<String>,
    note: String,
    scroll: usize,
}

pub(super) struct AskUserPrompt {
    request_id: RequestId,
    questions: Vec<ToolRequestUserInputQuestion>,
    answers: Vec<AnswerState>,
    active: usize,
    focus: Focus,
    hovered: Option<usize>,
    item_hits: Vec<(Rect, usize)>,
    editor_area: Option<Rect>,
    last_click: Option<(usize, Instant)>,
    window: ModalWindow,
}

impl AskUserPrompt {
    pub(super) fn from_request(request: &ServerRequest) -> Option<Self> {
        let ServerRequest::ToolRequestUserInput { request_id, params } = request else {
            return None;
        };
        Some(Self::new(request_id.clone(), params))
    }

    fn new(request_id: RequestId, params: &ToolRequestUserInputParams) -> Self {
        let mut prompt = Self {
            request_id,
            questions: params.questions.clone(),
            answers: vec![AnswerState::default(); params.questions.len()],
            active: 0,
            focus: Focus::Options,
            hovered: None,
            item_hits: Vec::new(),
            editor_area: None,
            last_click: None,
            window: ModalWindow::default(),
        };
        prompt.ensure_focus();
        prompt
    }

    pub(super) fn desired_height(&self, width: u16, available: u16) -> u16 {
        let question_rows = self.current_question().map_or(1, |question| {
            textwrap::wrap(
                &question.question,
                usize::from(width.saturating_sub(2).max(1)),
            )
            .len()
        });
        let tabs = usize::from(self.questions.len() > 1) * 2;
        let editor = usize::from(self.focus == Focus::Notes) * 3;
        (question_rows + self.total_items().min(7) + tabs + editor + 3)
            .clamp(8.min(usize::from(available)), usize::from(available)) as u16
    }

    pub(super) fn handle_key_event(&mut self, key: KeyEvent) -> PromptInteractionOutcome {
        if key.kind == KeyEventKind::Release {
            return PromptInteractionOutcome::Unchanged;
        }
        if self.questions.is_empty() && key.code == KeyCode::Enter {
            return self.submit();
        } else if self.questions.is_empty() {
            return PromptInteractionOutcome::Unchanged;
        }
        match self.focus {
            Focus::Options => self.handle_options_key(key),
            Focus::Notes => self.handle_notes_key(key),
        }
    }

    pub(super) fn handle_mouse_event_at(
        &mut self,
        mouse: MouseEvent,
        now: Instant,
    ) -> PromptInteractionOutcome {
        match self.window.handle_mouse_event(mouse) {
            ModalOutcome::TabChanged(tab) => {
                self.switch_question(tab);
                return PromptInteractionOutcome::Changed;
            }
            ModalOutcome::Handled | ModalOutcome::ShortcutActivated(_) => {
                return PromptInteractionOutcome::Changed;
            }
            ModalOutcome::CloseRequested | ModalOutcome::Unhandled => {}
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll(-1),
            MouseEventKind::ScrollDown => self.scroll(1),
            MouseEventKind::Moved => {
                let hovered = self.item_at(mouse);
                if self.hovered == hovered {
                    PromptInteractionOutcome::Unchanged
                } else {
                    self.hovered = hovered;
                    PromptInteractionOutcome::Changed
                }
            }
            MouseEventKind::Down(MouseButton::Left) => self.click(mouse, now),
            _ => PromptInteractionOutcome::Unchanged,
        }
    }

    pub(super) fn handle_paste(&mut self, text: &str) -> PromptInteractionOutcome {
        if text.is_empty() || self.questions.is_empty() {
            return PromptInteractionOutcome::Unchanged;
        }
        self.enter_notes();
        self.answer_mut().note.push_str(text);
        PromptInteractionOutcome::Changed
    }

    fn handle_options_key(&mut self, key: KeyEvent) -> PromptInteractionOutcome {
        match (key.code, key.modifiers) {
            (KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab, _) => self.move_cursor(-1),
            (KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab, KeyModifiers::NONE) => {
                self.move_cursor(1)
            }
            (KeyCode::Left | KeyCode::Char('h'), KeyModifiers::NONE) => self.switch_relative(-1),
            (KeyCode::Right | KeyCode::Char('l'), KeyModifiers::NONE) => self.switch_relative(1),
            (KeyCode::Char('z'), KeyModifiers::NONE) => self.enter_notes(),
            (KeyCode::Char(' '), KeyModifiers::NONE) => self.toggle_current(),
            (KeyCode::Enter, KeyModifiers::NONE) => return self.choose_current(),
            (KeyCode::Esc, KeyModifiers::NONE) => self.clear_current(),
            (KeyCode::Char(ch @ '1'..='9'), KeyModifiers::NONE) => {
                let index = usize::from(ch as u8 - b'1');
                if index < self.option_count() {
                    self.select(index);
                    return self.advance_or_submit();
                }
            }
            (KeyCode::Char(ch), modifiers)
                if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT =>
            {
                self.enter_notes();
                self.answer_mut().note.push(ch);
            }
            _ => return PromptInteractionOutcome::Unchanged,
        }
        PromptInteractionOutcome::Changed
    }

    fn handle_notes_key(&mut self, key: KeyEvent) -> PromptInteractionOutcome {
        match (key.code, key.modifiers) {
            (KeyCode::Esc | KeyCode::Tab, _) => self.focus = Focus::Options,
            (KeyCode::Enter, KeyModifiers::NONE) => return self.advance_or_submit(),
            (KeyCode::Enter, KeyModifiers::SHIFT) => self.answer_mut().note.push('\n'),
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.answer_mut().note.pop();
            }
            (KeyCode::Char(ch), modifiers)
                if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT =>
            {
                self.answer_mut().note.push(ch);
            }
            _ => return PromptInteractionOutcome::Unchanged,
        }
        PromptInteractionOutcome::Changed
    }

    fn choose_current(&mut self) -> PromptInteractionOutcome {
        let cursor = self.answer().cursor;
        if cursor >= self.option_count() {
            self.enter_notes();
            return PromptInteractionOutcome::Changed;
        }
        self.select(cursor);
        self.advance_or_submit()
    }

    fn advance_or_submit(&mut self) -> PromptInteractionOutcome {
        if self.active + 1 < self.questions.len() {
            self.switch_question(self.active + 1);
            PromptInteractionOutcome::Changed
        } else {
            self.submit()
        }
    }

    fn submit(&self) -> PromptInteractionOutcome {
        let answers = self
            .questions
            .iter()
            .zip(&self.answers)
            .map(|(question, answer)| {
                let mut values = answer.selected.clone().into_iter().collect::<Vec<_>>();
                if !answer.note.trim().is_empty() {
                    values.push(format!("user_note: {}", answer.note.trim()));
                }
                (
                    question.id.clone(),
                    ToolRequestUserInputAnswer { answers: values },
                )
            })
            .collect::<HashMap<_, _>>();
        match serde_json::to_value(ToolRequestUserInputResponse { answers }) {
            Ok(result) => PromptInteractionOutcome::Submit(PromptInteractionSubmission {
                request_id: self.request_id.clone(),
                result,
            }),
            Err(error) => PromptInteractionOutcome::Failed(format!(
                "failed to serialize request_user_input response: {error}"
            )),
        }
    }

    fn current_question(&self) -> Option<&ToolRequestUserInputQuestion> {
        self.questions.get(self.active)
    }

    fn answer(&self) -> &AnswerState {
        &self.answers[self.active]
    }

    fn answer_mut(&mut self) -> &mut AnswerState {
        &mut self.answers[self.active]
    }

    fn option_count(&self) -> usize {
        self.current_question().map_or(0, |question| {
            let option_count = question.options.as_ref().map_or(0, Vec::len);
            option_count + usize::from(question.is_other && option_count > 0)
        })
    }

    fn total_items(&self) -> usize {
        self.option_count() + 1
    }

    fn option_label(&self, index: usize) -> Option<&str> {
        let question = self.current_question()?;
        question
            .options
            .as_ref()
            .and_then(|options| options.get(index))
            .map(|option| option.label.as_str())
            .or_else(|| {
                (question.is_other && index + 1 == self.option_count()).then_some(OTHER_LABEL)
            })
    }

    fn select(&mut self, index: usize) {
        if let Some(label) = self.option_label(index).map(str::to_string) {
            self.answer_mut().selected = Some(label);
        }
    }

    fn toggle_current(&mut self) {
        let cursor = self.answer().cursor;
        if cursor >= self.option_count() {
            self.enter_notes();
            return;
        }
        let label = self.option_label(cursor).map(str::to_string);
        let answer = self.answer_mut();
        answer.selected = (answer.selected != label).then_some(label).flatten();
    }

    fn move_cursor(&mut self, delta: i32) {
        let last = self.total_items().saturating_sub(1) as i32;
        let cursor = self.answer().cursor;
        self.answer_mut().cursor = (cursor as i32 + delta).clamp(0, last) as usize;
        self.last_click = None;
    }

    fn scroll(&mut self, delta: i32) -> PromptInteractionOutcome {
        let scroll = self.answer().scroll;
        self.answer_mut().scroll = (scroll as i32 + delta).max(0) as usize;
        PromptInteractionOutcome::Changed
    }

    fn switch_relative(&mut self, delta: i32) {
        if self.questions.len() > 1 {
            let len = self.questions.len() as i32;
            self.switch_question((self.active as i32 + delta).rem_euclid(len) as usize);
        }
    }

    fn switch_question(&mut self, index: usize) {
        self.active = index.min(self.questions.len().saturating_sub(1));
        self.hovered = None;
        self.last_click = None;
        self.ensure_focus();
        self.window.set_active_tab(self.active);
    }

    fn enter_notes(&mut self) {
        if self.questions.is_empty() {
            return;
        }
        if self.option_count() > 0 && self.answer().selected.is_none() {
            let cursor = self.answer().cursor;
            self.select(cursor.min(self.option_count().saturating_sub(1)));
        }
        self.focus = Focus::Notes;
    }

    fn clear_current(&mut self) {
        let answer = self.answer_mut();
        answer.selected = None;
        answer.note.clear();
    }

    fn ensure_focus(&mut self) {
        if !self.questions.is_empty() && self.option_count() == 0 {
            self.focus = Focus::Notes;
        } else {
            self.focus = Focus::Options;
        }
    }

    fn item_at(&self, mouse: MouseEvent) -> Option<usize> {
        self.item_hits.iter().find_map(|(area, index)| {
            area.contains((mouse.column, mouse.row).into())
                .then_some(*index)
        })
    }

    fn click(&mut self, mouse: MouseEvent, now: Instant) -> PromptInteractionOutcome {
        let point = (mouse.column, mouse.row).into();
        if self.editor_area.is_some_and(|area| area.contains(point)) {
            self.enter_notes();
            return PromptInteractionOutcome::Changed;
        }
        let Some(index) = self.item_at(mouse) else {
            self.last_click = None;
            return PromptInteractionOutcome::Unchanged;
        };
        self.answer_mut().cursor = index;
        let double_click = self.last_click.is_some_and(|(previous, at)| {
            previous == index && now.saturating_duration_since(at) < DOUBLE_CLICK_WINDOW
        });
        self.last_click = (!double_click).then_some((index, now));
        if index >= self.option_count() {
            self.enter_notes();
            PromptInteractionOutcome::Changed
        } else if double_click {
            self.select(index);
            self.advance_or_submit()
        } else {
            self.toggle_current();
            PromptInteractionOutcome::Changed
        }
    }
}
