use std::time::Instant;

use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::CommandExecutionRequestApprovalResponse;
use codex_app_server_protocol::FileChangeApprovalDecision;
use codex_app_server_protocol::FileChangeRequestApprovalParams;
use codex_app_server_protocol::FileChangeRequestApprovalResponse;
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
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;

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

mod mcp;

#[derive(Clone)]
enum ApprovalChoice {
    Command(CommandExecutionApprovalDecision),
    FileChange(FileChangeApprovalDecision),
    Mcp(mcp::Response),
}

struct ApprovalOption {
    label: String,
    choice: ApprovalChoice,
}

#[derive(Clone, Copy)]
enum ApprovalBodyTone {
    Dim,
    Plain,
}

pub(super) struct ApprovalPrompt {
    request_id: RequestId,
    title: String,
    body: Vec<String>,
    body_tone: ApprovalBodyTone,
    options: Vec<ApprovalOption>,
    cancel: ApprovalChoice,
    choices: ChoiceList,
    window: ModalWindow,
}

impl ApprovalPrompt {
    pub(super) fn from_request(request: &ServerRequest) -> Option<Self> {
        match request {
            ServerRequest::CommandExecutionRequestApproval { request_id, params } => {
                Some(Self::command(request_id.clone(), params))
            }
            ServerRequest::FileChangeRequestApproval { request_id, params } => {
                Some(Self::file_change(request_id.clone(), params))
            }
            ServerRequest::McpServerElicitationRequest { request_id, params } => {
                mcp::from_request(request_id.clone(), params)
            }
            _ => None,
        }
    }

    fn command(request_id: RequestId, params: &CommandExecutionRequestApprovalParams) -> Self {
        let mut body = Vec::new();
        if let Some(reason) = params.reason.as_deref() {
            body.push(format!("Reason: {reason}"));
        }
        if let Some(cwd) = params.cwd.as_ref() {
            body.push(format!("Working directory: {}", cwd.display()));
        }
        body.push(format!(
            "$ {}",
            params
                .command
                .as_deref()
                .unwrap_or("Command details unavailable")
        ));
        let decisions = effective_command_decisions(params);
        let options = decisions
            .into_iter()
            .map(|decision| ApprovalOption {
                label: command_decision_label(&decision, params),
                choice: ApprovalChoice::Command(decision),
            })
            .collect();
        Self {
            request_id,
            title: "Allow command?".to_string(),
            body,
            body_tone: ApprovalBodyTone::Dim,
            options,
            cancel: ApprovalChoice::Command(CommandExecutionApprovalDecision::Cancel),
            choices: ChoiceList::default(),
            window: ModalWindow::default(),
        }
    }

    fn file_change(request_id: RequestId, params: &FileChangeRequestApprovalParams) -> Self {
        let mut body = Vec::new();
        if let Some(reason) = params.reason.as_deref() {
            body.push(format!("Reason: {reason}"));
        }
        if let Some(root) = params.grant_root.as_ref() {
            body.push(format!("Requested write root: {}", root.display()));
        }
        let options = [
            ("Yes, proceed", FileChangeApprovalDecision::Accept),
            (
                "Yes, and don't ask again for these files",
                FileChangeApprovalDecision::AcceptForSession,
            ),
            (
                "No, and tell Astral Code what to do differently",
                FileChangeApprovalDecision::Cancel,
            ),
        ]
        .into_iter()
        .map(|(label, decision)| ApprovalOption {
            label: label.to_string(),
            choice: ApprovalChoice::FileChange(decision),
        })
        .collect();
        Self {
            request_id,
            title: "Allow file changes?".to_string(),
            body,
            body_tone: ApprovalBodyTone::Dim,
            options,
            cancel: ApprovalChoice::FileChange(FileChangeApprovalDecision::Cancel),
            choices: ChoiceList::default(),
            window: ModalWindow::default(),
        }
    }

    pub(super) fn selected_index(&self) -> usize {
        self.choices.selected()
    }

    pub(super) fn set_selected_index(&mut self, selected: usize) {
        self.choices.set_selected(selected, self.options.len());
    }

    pub(super) fn desired_height(&self, width: u16, available: u16) -> u16 {
        let content_width = usize::from(width.saturating_sub(2).max(1));
        let body_rows = self
            .body
            .iter()
            .map(|text| textwrap::wrap(text, content_width).len())
            .sum::<usize>();
        (body_rows + self.options.len() + 2)
            .min(usize::from(available))
            .max(6.min(usize::from(available))) as u16
    }

    pub(super) fn render(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        queue_len: usize,
        responding: bool,
    ) {
        self.choices.begin_frame();
        let title = if queue_len > 1 {
            format!("{} · {queue_len} requests waiting", self.title)
        } else {
            self.title.to_string()
        };
        let shortcuts = [
            ModalShortcut::hint("↑/↓ navigate"),
            ModalShortcut::hint("Enter confirm"),
            ModalShortcut::hint("Esc cancel"),
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
        let option_rows = (self.options.len() as u16).min(layout.content.height);
        let options_y = layout.content.bottom().saturating_sub(option_rows);
        render_body(
            buffer,
            Rect::new(
                layout.content.x,
                layout.content.y,
                layout.content.width,
                options_y.saturating_sub(layout.content.y),
            ),
            &self.body,
            self.body_tone,
        );
        for (index, option) in self
            .options
            .iter()
            .take(usize::from(option_rows))
            .enumerate()
        {
            let row = Rect::new(
                layout.content.x,
                options_y.saturating_add(index as u16),
                layout.content.width,
                1,
            );
            let prefix = self.choices.prefix(index);
            let style = self.choices.style(index);
            buffer.set_line(
                row.x,
                row.y,
                &Line::from(format!("{prefix}{}. {}", index + 1, option.label)).style(style),
                row.width,
            );
            self.choices.record_hit(row);
        }
        if responding && !layout.footer.is_empty() {
            buffer.set_line(
                layout.footer.x,
                layout.footer.y,
                &Line::from("Sending decision…").dim(),
                layout.footer.width,
            );
        }
    }

    pub(super) fn handle_key_event(&mut self, key: KeyEvent) -> PromptInteractionOutcome {
        if key.kind == KeyEventKind::Release {
            return PromptInteractionOutcome::Unchanged;
        }
        if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE {
            return self.submit(self.cancel.clone());
        }
        let outcome = self.choices.handle_key(key, self.options.len());
        self.handle_choice_outcome(outcome)
    }

    pub(super) fn handle_mouse_event_at(
        &mut self,
        mouse: MouseEvent,
        now: Instant,
    ) -> PromptInteractionOutcome {
        match self.window.handle_mouse_event(mouse) {
            ModalOutcome::CloseRequested => return self.submit(self.cancel.clone()),
            ModalOutcome::Handled | ModalOutcome::ShortcutActivated(_) => {
                return PromptInteractionOutcome::Changed;
            }
            ModalOutcome::TabChanged(_) | ModalOutcome::Unhandled => {}
        }
        let outcome = self.choices.handle_mouse(mouse, now, self.options.len());
        self.handle_choice_outcome(outcome)
    }

    fn handle_choice_outcome(&self, outcome: ChoiceListOutcome) -> PromptInteractionOutcome {
        match outcome {
            ChoiceListOutcome::Unchanged => PromptInteractionOutcome::Unchanged,
            ChoiceListOutcome::Changed => PromptInteractionOutcome::Changed,
            ChoiceListOutcome::Activate(index) => self
                .options
                .get(index)
                .map_or(PromptInteractionOutcome::Unchanged, |option| {
                    self.submit(option.choice.clone())
                }),
        }
    }

    fn submit(&self, choice: ApprovalChoice) -> PromptInteractionOutcome {
        let result = match choice {
            ApprovalChoice::Command(decision) => {
                serde_json::to_value(CommandExecutionRequestApprovalResponse { decision })
            }
            ApprovalChoice::FileChange(decision) => {
                serde_json::to_value(FileChangeRequestApprovalResponse { decision })
            }
            ApprovalChoice::Mcp(response) => {
                serde_json::to_value(McpServerElicitationRequestResponse {
                    action: response.action,
                    content: None,
                    meta: response.meta,
                })
            }
        };
        match result {
            Ok(result) => PromptInteractionOutcome::Submit(PromptInteractionSubmission {
                request_id: self.request_id.clone(),
                result,
            }),
            Err(error) => PromptInteractionOutcome::Failed(format!(
                "failed to serialize app-server interaction response: {error}"
            )),
        }
    }
}

fn effective_command_decisions(
    params: &CommandExecutionRequestApprovalParams,
) -> Vec<CommandExecutionApprovalDecision> {
    if let Some(decisions) = params.available_decisions.as_ref() {
        return decisions.clone();
    }
    if params.network_approval_context.is_some() {
        let mut decisions = vec![
            CommandExecutionApprovalDecision::Accept,
            CommandExecutionApprovalDecision::AcceptForSession,
        ];
        if let Some(amendment) = params
            .proposed_network_policy_amendments
            .as_deref()
            .and_then(|amendments| {
                amendments.iter().find(|amendment| {
                    amendment.action == codex_app_server_protocol::NetworkPolicyRuleAction::Allow
                })
            })
        {
            decisions.push(
                CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment {
                    network_policy_amendment: amendment.clone(),
                },
            );
        }
        decisions.push(CommandExecutionApprovalDecision::Cancel);
        return decisions;
    }
    if params.additional_permissions.is_some() {
        return vec![
            CommandExecutionApprovalDecision::Accept,
            CommandExecutionApprovalDecision::Cancel,
        ];
    }
    let mut decisions = vec![CommandExecutionApprovalDecision::Accept];
    if let Some(amendment) = params.proposed_execpolicy_amendment.as_ref() {
        decisions.push(
            CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment {
                execpolicy_amendment: amendment.clone(),
            },
        );
    }
    decisions.push(CommandExecutionApprovalDecision::Cancel);
    decisions
}

fn command_decision_label(
    decision: &CommandExecutionApprovalDecision,
    params: &CommandExecutionRequestApprovalParams,
) -> String {
    match decision {
        CommandExecutionApprovalDecision::Accept => "Yes, proceed".to_string(),
        CommandExecutionApprovalDecision::AcceptForSession => {
            if params.network_approval_context.is_some() {
                "Yes, and allow this host for this conversation".to_string()
            } else {
                "Yes, and don't ask again in this session".to_string()
            }
        }
        CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment { .. } => {
            "Yes, and allow similar commands in the future".to_string()
        }
        CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment {
            network_policy_amendment,
        } => format!(
            "Apply the proposed network rule for {}",
            network_policy_amendment.host
        ),
        CommandExecutionApprovalDecision::Decline => "No, continue without running it".to_string(),
        CommandExecutionApprovalDecision::Cancel => {
            "No, and tell Astral Code what to do differently".to_string()
        }
    }
}

fn render_body(buffer: &mut Buffer, area: Rect, body: &[String], body_tone: ApprovalBodyTone) {
    if area.is_empty() {
        return;
    }
    let mut row = area.y;
    for text in body {
        for wrapped in textwrap::wrap(text, usize::from(area.width.max(1))) {
            if row >= area.bottom() {
                return;
            }
            let style = match (text.starts_with("$ "), body_tone) {
                (true, _) => Style::default().cyan(),
                (false, ApprovalBodyTone::Dim) => Style::default().dim(),
                (false, ApprovalBodyTone::Plain) => Style::default(),
            };
            buffer.set_line(
                area.x,
                row,
                &Line::from(wrapped.into_owned()).style(style),
                area.width,
            );
            row = row.saturating_add(1);
        }
    }
}
