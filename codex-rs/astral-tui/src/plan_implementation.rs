//! Local interaction shown after a completed proposed-plan turn.
//!
//! Proposed plans are app-server transcript items, but the decision to keep
//! planning or start implementation is TUI-local. This module keeps that
//! presentation separate from blocking JSON-RPC server requests.

use std::time::Instant;

use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnStatus;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::ModalOutcome;
use crate::ModalPresentation;
use crate::ModalShortcut;
use crate::ModalSizing;
use crate::ModalWindow;
use crate::ModalWindowConfig;
use crate::prompt_interaction::choice_list::ChoiceList;
use crate::prompt_interaction::choice_list::ChoiceListOutcome;

const TITLE: &str = "Implement this plan?";
const IMPLEMENT_MESSAGE: &str = "Implement the plan.";
const FRESH_CONTEXT_PREFIX: &str = concat!(
    "A previous agent produced the plan below to accomplish the user's task. ",
    "Implement the plan in a fresh context. Treat the plan as the source of ",
    "user intent, re-read files as needed, and carry the work through ",
    "implementation and verification."
);

const OPTIONS: [(&str, &str); 3] = [
    (
        "Yes, implement this plan",
        "Switch to Default and start coding.",
    ),
    (
        "Yes, clear context and implement",
        "Fresh thread with this plan.",
    ),
    ("No, stay in Plan mode", "Continue planning with the model."),
];

/// One completed plan awaiting a local implementation decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanImplementationRequest {
    turn_id: String,
    item_id: String,
    plan_markdown: String,
}

impl PlanImplementationRequest {
    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    pub fn plan_markdown(&self) -> &str {
        &self.plan_markdown
    }
}

/// Semantic action selected from the post-plan prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanImplementationSelection {
    ImplementCurrentThread { input: String },
    ImplementFreshThread { input: String },
    StayInPlanMode,
}

/// Result of routing input through the post-plan prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlanImplementationOutcome {
    Unchanged,
    Changed,
    Selected(PlanImplementationSelection),
}

/// Retained Grok-style presenter for a local plan implementation decision.
pub struct PlanImplementationHost {
    request: Option<PlanImplementationRequest>,
    choices: ChoiceList,
    window: ModalWindow,
}

impl Default for PlanImplementationHost {
    fn default() -> Self {
        Self::new()
    }
}

impl PlanImplementationHost {
    pub fn new() -> Self {
        Self {
            request: None,
            choices: ChoiceList::default(),
            window: ModalWindow::default(),
        }
    }

    pub fn is_presentable(&self) -> bool {
        self.request.is_some()
    }

    pub fn sync(&mut self, request: Option<&PlanImplementationRequest>) -> bool {
        if self.request.as_ref() == request {
            return false;
        }
        let same_item = self
            .request
            .as_ref()
            .zip(request)
            .is_some_and(|(previous, current)| {
                previous.turn_id == current.turn_id && previous.item_id == current.item_id
            });
        self.request = request.cloned();
        if !same_item {
            self.choices = ChoiceList::default();
        }
        true
    }

    pub fn desired_height(&self, width: u16, available: u16) -> u16 {
        if self.request.is_none() {
            return 0;
        }
        let rows = self
            .option_lines(width.saturating_sub(4).max(1))
            .iter()
            .map(Vec::len)
            .sum::<usize>()
            + 2;
        rows.min(usize::from(available))
            .max(6.min(usize::from(available))) as u16
    }

    pub fn render(&mut self, buffer: &mut Buffer, area: Rect) {
        if self.request.is_none() {
            return;
        }
        self.choices.begin_frame();
        let shortcuts = [
            ModalShortcut::hint("↑/↓ navigate"),
            ModalShortcut::hint("Enter confirm"),
            ModalShortcut::hint("Esc stay in Plan"),
        ];
        let mut sizing = ModalSizing::medium().compact();
        sizing.footer_rows = 1;
        let config = ModalWindowConfig::new(TITLE)
            .with_shortcuts(&shortcuts)
            .with_sizing(sizing)
            .with_presentation(ModalPresentation::Embedded);
        let Some(layout) = self.window.render(buffer, area, &config) else {
            return;
        };

        let option_lines = self.option_lines(layout.content.width);
        let mut y = layout.content.y;
        for lines in option_lines {
            let start_y = y;
            for line in lines {
                if y >= layout.content.bottom() {
                    break;
                }
                buffer.set_line(layout.content.x, y, &line, layout.content.width);
                y = y.saturating_add(1);
            }
            self.choices.record_hit(Rect::new(
                layout.content.x,
                start_y,
                layout.content.width,
                y.saturating_sub(start_y),
            ));
        }
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> PlanImplementationOutcome {
        if self.request.is_none() || key.kind == KeyEventKind::Release {
            return PlanImplementationOutcome::Unchanged;
        }
        if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE {
            return stay_in_plan();
        }
        let outcome = self.choices.handle_key(key, OPTIONS.len());
        self.handle_choice_outcome(outcome)
    }

    pub fn handle_mouse_event_at(
        &mut self,
        mouse: MouseEvent,
        now: Instant,
    ) -> PlanImplementationOutcome {
        if self.request.is_none() {
            return PlanImplementationOutcome::Unchanged;
        }
        match self.window.handle_mouse_event(mouse) {
            ModalOutcome::CloseRequested => return stay_in_plan(),
            ModalOutcome::Handled | ModalOutcome::ShortcutActivated(_) => {
                return PlanImplementationOutcome::Changed;
            }
            ModalOutcome::TabChanged(_) | ModalOutcome::Unhandled => {}
        }
        let outcome = self.choices.handle_mouse(mouse, now, OPTIONS.len());
        self.handle_choice_outcome(outcome)
    }

    fn option_lines(&self, width: u16) -> Vec<Vec<Line<'static>>> {
        let content_width = usize::from(width.max(1));
        OPTIONS
            .iter()
            .enumerate()
            .map(|(index, (label, description))| {
                let prefix = format!("{}{}. ", self.choices.prefix(index), index + 1);
                let continuation = " ".repeat(prefix.chars().count());
                let text_width = content_width.saturating_sub(prefix.chars().count()).max(1);
                let style = self.choices.style(index);
                let mut lines = textwrap::wrap(label, text_width)
                    .into_iter()
                    .enumerate()
                    .map(|(line_index, text)| {
                        let prefix = if line_index == 0 {
                            prefix.as_str()
                        } else {
                            continuation.as_str()
                        };
                        Line::from(format!("{prefix}{text}")).style(style)
                    })
                    .collect::<Vec<_>>();
                let description_prefix = " ".repeat(prefix.chars().count());
                lines.extend(
                    textwrap::wrap(description, text_width)
                        .into_iter()
                        .map(|text| {
                            Line::from(format!("{description_prefix}{text}")).style(style.dim())
                        }),
                );
                lines
            })
            .collect()
    }

    fn handle_choice_outcome(&self, outcome: ChoiceListOutcome) -> PlanImplementationOutcome {
        match outcome {
            ChoiceListOutcome::Unchanged => PlanImplementationOutcome::Unchanged,
            ChoiceListOutcome::Changed => PlanImplementationOutcome::Changed,
            ChoiceListOutcome::Activate(0) => PlanImplementationOutcome::Selected(
                PlanImplementationSelection::ImplementCurrentThread {
                    input: IMPLEMENT_MESSAGE.to_string(),
                },
            ),
            ChoiceListOutcome::Activate(1) => {
                let Some(request) = self.request.as_ref() else {
                    return PlanImplementationOutcome::Unchanged;
                };
                PlanImplementationOutcome::Selected(
                    PlanImplementationSelection::ImplementFreshThread {
                        input: format!("{FRESH_CONTEXT_PREFIX}\n\n{}", request.plan_markdown),
                    },
                )
            }
            ChoiceListOutcome::Activate(2) => stay_in_plan(),
            ChoiceListOutcome::Activate(_) => PlanImplementationOutcome::Unchanged,
        }
    }
}

fn stay_in_plan() -> PlanImplementationOutcome {
    PlanImplementationOutcome::Selected(PlanImplementationSelection::StayInPlanMode)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TrackerState {
    AwaitingTurnCompletion(PlanImplementationRequest),
    Ready(PlanImplementationRequest),
}

#[derive(Debug, Default)]
pub(crate) struct PlanImplementationTracker {
    state: Option<TrackerState>,
}

impl PlanImplementationTracker {
    pub(crate) fn request(&self) -> Option<&PlanImplementationRequest> {
        match self.state.as_ref() {
            Some(TrackerState::Ready(request)) => Some(request),
            Some(TrackerState::AwaitingTurnCompletion(_)) | None => None,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.state = None;
    }

    pub(crate) fn observe_event(&mut self, thread_id: &str, event: &AppServerEvent) {
        match event {
            AppServerEvent::ServerNotification(notification) => {
                self.observe_notification(thread_id, notification);
            }
            AppServerEvent::Disconnected { .. } => self.clear(),
            AppServerEvent::Lagged { .. } | AppServerEvent::ServerRequest(_) => {}
        }
    }

    fn observe_notification(&mut self, thread_id: &str, notification: &ServerNotification) {
        match notification {
            ServerNotification::TurnStarted(notification)
                if notification.thread_id == thread_id =>
            {
                self.clear();
            }
            ServerNotification::ItemCompleted(notification)
                if notification.thread_id == thread_id =>
            {
                if let ThreadItem::Plan { id, text } = &notification.item
                    && !text.trim().is_empty()
                {
                    self.state = Some(TrackerState::AwaitingTurnCompletion(
                        PlanImplementationRequest {
                            turn_id: notification.turn_id.clone(),
                            item_id: id.clone(),
                            plan_markdown: text.clone(),
                        },
                    ));
                }
            }
            ServerNotification::TurnCompleted(notification)
                if notification.thread_id == thread_id =>
            {
                let matching_request = match self.state.take() {
                    Some(TrackerState::AwaitingTurnCompletion(request))
                        if request.turn_id == notification.turn.id =>
                    {
                        Some(request)
                    }
                    state => {
                        self.state = state;
                        None
                    }
                };
                if notification.turn.status == TurnStatus::Completed
                    && let Some(request) = matching_request
                {
                    self.state = Some(TrackerState::Ready(request));
                }
            }
            ServerNotification::ThreadClosed(notification)
                if notification.thread_id == thread_id =>
            {
                self.clear();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "plan_implementation_tests.rs"]
mod tests;
