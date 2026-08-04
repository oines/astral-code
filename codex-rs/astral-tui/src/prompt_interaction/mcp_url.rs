use std::time::Instant;

use codex_app_server_protocol::McpServerElicitationAction;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::McpServerElicitationRequestResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use url::Url;

use crate::ModalOutcome;
use crate::ModalPresentation;
use crate::ModalShortcut;
use crate::ModalSizing;
use crate::ModalWindow;
use crate::ModalWindowConfig;
use crate::prompt_interaction::PromptInteractionOutcome;
use crate::prompt_interaction::PromptInteractionSubmission;
use crate::prompt_interaction::choice_list::ChoiceList;
use crate::prompt_interaction::choice_list::ChoiceListOutcome;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Ready,
    WaitingForBrowser,
}

pub(super) struct McpUrlPrompt {
    request_id: RequestId,
    server_name: String,
    message: String,
    displayed_url: String,
    safe_url: Option<String>,
    stage: Stage,
    choices: ChoiceList,
    window: ModalWindow,
}

impl McpUrlPrompt {
    pub(super) fn from_request(request: &ServerRequest) -> Option<Self> {
        let ServerRequest::McpServerElicitationRequest { request_id, params } = request else {
            return None;
        };
        let McpServerElicitationRequest::Url { message, url, .. } = &params.request else {
            return None;
        };
        let safe_url = validate_external_url(url);
        let displayed_url = safe_url.clone().unwrap_or_else(|| url.clone());
        Some(Self {
            request_id: request_id.clone(),
            server_name: params.server_name.clone(),
            message: message.clone(),
            displayed_url,
            safe_url,
            stage: Stage::Ready,
            choices: ChoiceList::default(),
            window: ModalWindow::default(),
        })
    }

    pub(super) fn selected_index(&self) -> usize {
        self.choices.selected()
    }

    pub(super) fn set_selected_index(&mut self, selected: usize) {
        let choice_count = self.choice_count();
        self.choices.set_selected(selected, choice_count);
    }

    pub(super) fn desired_height(&self, width: u16, available: u16) -> u16 {
        let content_width = usize::from(width.saturating_sub(2).max(1));
        let body_rows = self.body_lines(content_width).len();
        (body_rows + self.choice_count() + 2)
            .min(usize::from(available))
            .max(8.min(usize::from(available))) as u16
    }

    pub(super) fn render(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        queue_len: usize,
        responding: bool,
    ) {
        self.choices.begin_frame();
        let title = self.title(queue_len);
        let shortcuts = [
            ModalShortcut::hint("↑/↓ navigate"),
            ModalShortcut::hint("Enter confirm"),
            ModalShortcut::hint("Esc decline"),
        ];
        let mut sizing = ModalSizing::medium().compact();
        sizing.footer_rows = 1;
        let config = ModalWindowConfig::new(&title)
            .with_shortcuts(&shortcuts)
            .with_sizing(sizing)
            .with_presentation(ModalPresentation::Embedded);
        let Some(layout) = self.window.render(buffer, area, &config) else {
            return;
        };
        let choice_rows = (self.choice_count() as u16).min(layout.content.height);
        let choices_y = layout.content.bottom().saturating_sub(choice_rows);
        let body_area = Rect::new(
            layout.content.x,
            layout.content.y,
            layout.content.width,
            choices_y.saturating_sub(layout.content.y),
        );
        render_lines(
            buffer,
            body_area,
            self.body_lines(usize::from(body_area.width.max(1))),
        );
        for (index, label) in self
            .labels()
            .iter()
            .take(usize::from(choice_rows))
            .enumerate()
        {
            let row = Rect::new(
                layout.content.x,
                choices_y + index as u16,
                layout.content.width,
                1,
            );
            let line = Line::from(format!(
                "{}{}. {label}",
                self.choices.prefix(index),
                index + 1
            ))
            .style(self.choices.style(index));
            buffer.set_line(row.x, row.y, &line, row.width);
            self.choices.record_hit(row);
        }
        if responding && !layout.footer.is_empty() {
            let status = Line::from("Sending response…").dim();
            buffer.set_line(
                layout.footer.x,
                layout.footer.y,
                &status,
                layout.footer.width,
            );
        }
    }

    pub(super) fn handle_key_event(&mut self, key: KeyEvent) -> PromptInteractionOutcome {
        if key.kind == KeyEventKind::Release {
            return PromptInteractionOutcome::Unchanged;
        }
        if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE {
            return self.submit(McpServerElicitationAction::Decline);
        }
        let choice_count = self.choice_count();
        let outcome = self.choices.handle_key(key, choice_count);
        self.handle_choice_outcome(outcome)
    }

    pub(super) fn handle_mouse_event_at(
        &mut self,
        mouse: MouseEvent,
        now: Instant,
    ) -> PromptInteractionOutcome {
        match self.window.handle_mouse_event(mouse) {
            ModalOutcome::CloseRequested => {
                return self.submit(McpServerElicitationAction::Decline);
            }
            ModalOutcome::Handled | ModalOutcome::ShortcutActivated(_) => {
                return PromptInteractionOutcome::Changed;
            }
            ModalOutcome::TabChanged(_) | ModalOutcome::Unhandled => {}
        }
        let choice_count = self.choice_count();
        let outcome = self.choices.handle_mouse(mouse, now, choice_count);
        self.handle_choice_outcome(outcome)
    }

    fn handle_choice_outcome(&mut self, outcome: ChoiceListOutcome) -> PromptInteractionOutcome {
        match outcome {
            ChoiceListOutcome::Unchanged => PromptInteractionOutcome::Unchanged,
            ChoiceListOutcome::Changed => PromptInteractionOutcome::Changed,
            ChoiceListOutcome::Activate(index) => self.activate(index),
        }
    }

    fn activate(&mut self, index: usize) -> PromptInteractionOutcome {
        match (self.stage, self.safe_url.as_ref(), index) {
            (Stage::Ready, Some(url), 0) => {
                let url = url.clone();
                self.stage = Stage::WaitingForBrowser;
                let choice_count = self.choice_count();
                self.choices.set_selected(0, choice_count);
                PromptInteractionOutcome::OpenExternalUrl { url }
            }
            (Stage::WaitingForBrowser, _, 0) => self.submit(McpServerElicitationAction::Accept),
            (Stage::WaitingForBrowser, _, 1) => {
                self.stage = Stage::Ready;
                let choice_count = self.choice_count();
                self.choices.set_selected(0, choice_count);
                PromptInteractionOutcome::Changed
            }
            _ => self.submit(McpServerElicitationAction::Decline),
        }
    }

    fn submit(&self, action: McpServerElicitationAction) -> PromptInteractionOutcome {
        match serde_json::to_value(McpServerElicitationRequestResponse {
            action,
            content: None,
            meta: None,
        }) {
            Ok(result) => PromptInteractionOutcome::Submit(PromptInteractionSubmission {
                request_id: self.request_id.clone(),
                result,
            }),
            Err(error) => PromptInteractionOutcome::Failed(format!(
                "failed to serialize MCP URL elicitation response: {error}"
            )),
        }
    }

    fn choice_count(&self) -> usize {
        usize::from(self.safe_url.is_some() || self.stage == Stage::WaitingForBrowser) + 1
    }

    fn labels(&self) -> &'static [&'static str] {
        match (self.stage, self.safe_url.is_some()) {
            (Stage::Ready, true) => &["Open link", "Back"],
            (Stage::Ready, false) => &["Decline"],
            (Stage::WaitingForBrowser, _) => &["I finished", "Back"],
        }
    }

    fn title(&self, queue_len: usize) -> String {
        let mut title = match self.stage {
            Stage::Ready => format!("Action required · {}", self.server_name),
            Stage::WaitingForBrowser => "Finish in browser".to_string(),
        };
        if queue_len > 1 {
            title.push_str(&format!(" · {queue_len} requests waiting"));
        }
        title
    }

    fn body_lines(&self, width: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        match self.stage {
            Stage::Ready => {
                lines.extend(wrapped(&self.message, width, true));
                lines.push(Line::default());
                lines.push("URL".dim().into());
                let safe = self.safe_url.is_some();
                for line in textwrap::wrap(&self.displayed_url, width.max(1)) {
                    let line = Line::from(line.into_owned());
                    lines.push(if safe {
                        line.cyan().underlined()
                    } else {
                        line.red()
                    });
                }
                lines.push(Line::default());
                let instruction = if safe {
                    "Complete the requested action in your browser, then return here."
                } else {
                    "Blocked: only credential-free HTTPS URLs can be opened."
                };
                lines.extend(wrapped(instruction, width, false));
            }
            Stage::WaitingForBrowser => {
                lines.extend(wrapped(
                    "Complete the requested action in the browser window that just opened. Then return here and select “I finished”.",
                    width,
                    false,
                ));
                lines.push(Line::default());
                lines.push("Link".dim().into());
                for line in textwrap::wrap(&self.displayed_url, width.max(1)) {
                    lines.push(Line::from(line.into_owned()).cyan().underlined());
                }
            }
        }
        lines
    }
}

pub(super) fn validate_external_url(raw: &str) -> Option<String> {
    let parsed = Url::parse(raw).ok()?;
    (parsed.scheme() == "https"
        && parsed.host_str().is_some()
        && parsed.username().is_empty()
        && parsed.password().is_none())
    .then(|| parsed.to_string())
}

fn wrapped(text: &str, width: usize, italic: bool) -> Vec<Line<'static>> {
    textwrap::wrap(text, width.max(1))
        .into_iter()
        .map(|line| {
            let line = Line::from(line.into_owned());
            if italic { line.italic() } else { line }
        })
        .collect()
}

fn render_lines(buffer: &mut Buffer, area: Rect, lines: Vec<Line<'static>>) {
    for (offset, line) in lines.into_iter().take(usize::from(area.height)).enumerate() {
        buffer.set_line(area.x, area.y + offset as u16, &line, area.width);
    }
}
