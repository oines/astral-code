//! Pointer interaction for `request_user_input`.
//!
//! The selection and double-click behavior follows Grok Build's question view
//! at commit 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0). Astral keeps
//! Codex's single-choice response contract and only adopts the TUI interaction
//! model.

use std::time::Duration;
use std::time::Instant;

use codex_app_server_protocol::ToolRequestUserInputParams;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Rect;

use super::ConfirmationChoice;
use super::Focus;
use super::RequestUserInputEvent;
use super::RequestUserInputHit;
use super::RequestUserInputState;
use super::has_options;
use super::option_count;

const MULTI_CLICK_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct RequestUserInputPointerState {
    hovered: Option<RequestUserInputHit>,
    hit_rows: Vec<(RequestUserInputHit, Rect)>,
    last_click: Option<(Instant, RequestUserInputHit)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerAction {
    Consume,
    Select(RequestUserInputHit),
    Activate(RequestUserInputHit),
    Scroll { next: bool },
}

impl RequestUserInputPointerState {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn clear_click(&mut self) {
        self.last_click = None;
    }

    fn observe_rows(&mut self, hit_rows: Vec<(RequestUserInputHit, Rect)>) {
        self.hit_rows = hit_rows;
        if self
            .hovered
            .is_some_and(|hovered| !self.hit_rows.iter().any(|(hit, _)| *hit == hovered))
        {
            self.hovered = None;
        }
        if self
            .last_click
            .is_some_and(|(_, clicked)| !self.hit_rows.iter().any(|(hit, _)| *hit == clicked))
        {
            self.last_click = None;
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, now: Instant) -> PointerAction {
        let hit = self.hit_test(mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Moved => {
                self.hovered = hit;
                PointerAction::Consume
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(hit) = hit else {
                    self.last_click = None;
                    return PointerAction::Consume;
                };
                self.hovered = Some(hit);
                if hit == RequestUserInputHit::Editor {
                    self.last_click = None;
                    return PointerAction::Activate(hit);
                }
                let double_click = self.last_click.is_some_and(|(last, previous)| {
                    previous == hit && now.duration_since(last) < MULTI_CLICK_TIMEOUT
                });
                if double_click {
                    self.last_click = None;
                    PointerAction::Activate(hit)
                } else {
                    self.last_click = Some((now, hit));
                    PointerAction::Select(hit)
                }
            }
            MouseEventKind::ScrollDown => {
                self.last_click = None;
                PointerAction::Scroll { next: true }
            }
            MouseEventKind::ScrollUp => {
                self.last_click = None;
                PointerAction::Scroll { next: false }
            }
            MouseEventKind::Drag(MouseButton::Left)
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => {
                self.last_click = None;
                PointerAction::Consume
            }
            MouseEventKind::Down(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Up(MouseButton::Left | MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Drag(MouseButton::Right | MouseButton::Middle) => {
                PointerAction::Consume
            }
        }
    }

    fn hit_test(&self, column: u16, row: u16) -> Option<RequestUserInputHit> {
        self.hit_rows
            .iter()
            .find(|(_, area)| area.contains((column, row).into()))
            .map(|(hit, _)| *hit)
    }
}

impl RequestUserInputState {
    pub(crate) fn hovered(&self) -> Option<RequestUserInputHit> {
        self.pointer.hovered
    }

    pub(crate) fn observe_rows(&mut self, hit_rows: Vec<(RequestUserInputHit, Rect)>) {
        self.pointer.observe_rows(hit_rows);
    }

    pub(crate) fn handle_mouse(
        &mut self,
        params: &ToolRequestUserInputParams,
        mouse: MouseEvent,
    ) -> RequestUserInputEvent {
        self.handle_mouse_at(params, mouse, Instant::now())
    }

    pub(super) fn handle_mouse_at(
        &mut self,
        params: &ToolRequestUserInputParams,
        mouse: MouseEvent,
        now: Instant,
    ) -> RequestUserInputEvent {
        self.sync(params);
        let action = self.pointer.handle_mouse(mouse, now);
        match action {
            PointerAction::Consume => RequestUserInputEvent::Redraw,
            PointerAction::Scroll { next } => {
                if let Some(choice) = self.confirmation {
                    self.confirmation = Some(match choice {
                        ConfirmationChoice::GoBack => ConfirmationChoice::Proceed,
                        ConfirmationChoice::Proceed => ConfirmationChoice::GoBack,
                    });
                } else if self.focus == Focus::Options
                    && let Some(question) = params.questions.get(self.current_question)
                {
                    self.move_option(option_count(question), next);
                }
                RequestUserInputEvent::Redraw
            }
            PointerAction::Select(hit) => self.select_pointer_hit(params, hit),
            PointerAction::Activate(hit) => self.activate_pointer_hit(params, hit),
        }
    }

    fn select_pointer_hit(
        &mut self,
        params: &ToolRequestUserInputParams,
        hit: RequestUserInputHit,
    ) -> RequestUserInputEvent {
        match hit {
            RequestUserInputHit::Confirmation(index) if self.confirmation.is_some() => {
                self.confirmation = Some(if index == 0 {
                    ConfirmationChoice::GoBack
                } else {
                    ConfirmationChoice::Proceed
                });
            }
            RequestUserInputHit::Option(index) if self.confirmation.is_none() => {
                let valid = params
                    .questions
                    .get(self.current_question)
                    .is_some_and(|question| index < option_count(question));
                if valid {
                    if let Some(answer) = self.current_answer_mut() {
                        let already_selected =
                            answer.selected_option == Some(index) && answer.committed;
                        answer.selected_option = Some(index);
                        answer.committed = !already_selected;
                    }
                    self.focus = Focus::Options;
                }
            }
            RequestUserInputHit::Editor if self.confirmation.is_none() => {
                let has_options = params
                    .questions
                    .get(self.current_question)
                    .is_some_and(has_options);
                if !has_options || self.selected_option().is_some() {
                    self.focus = Focus::Notes;
                    if let Some(answer) = self.current_answer_mut() {
                        answer.notes_visible = true;
                    }
                }
            }
            RequestUserInputHit::Confirmation(_)
            | RequestUserInputHit::Option(_)
            | RequestUserInputHit::Editor => {}
        }
        RequestUserInputEvent::Redraw
    }

    fn activate_pointer_hit(
        &mut self,
        params: &ToolRequestUserInputParams,
        hit: RequestUserInputHit,
    ) -> RequestUserInputEvent {
        match hit {
            RequestUserInputHit::Confirmation(index) if self.confirmation.is_some() => {
                self.confirmation = Some(if index == 0 {
                    ConfirmationChoice::GoBack
                } else {
                    ConfirmationChoice::Proceed
                });
                if index == 0 {
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
                } else {
                    RequestUserInputEvent::Submit(self.response(params))
                }
            }
            RequestUserInputHit::Option(index) if self.confirmation.is_none() => {
                let valid = params
                    .questions
                    .get(self.current_question)
                    .is_some_and(|question| index < option_count(question));
                if !valid {
                    return RequestUserInputEvent::Redraw;
                }
                if let Some(answer) = self.current_answer_mut() {
                    answer.selected_option = Some(index);
                    answer.committed = true;
                }
                self.focus = Focus::Options;
                self.advance_or_submit(params)
            }
            RequestUserInputHit::Editor if self.confirmation.is_none() => {
                self.select_pointer_hit(params, hit)
            }
            RequestUserInputHit::Confirmation(_)
            | RequestUserInputHit::Option(_)
            | RequestUserInputHit::Editor => RequestUserInputEvent::Redraw,
        }
    }
}
