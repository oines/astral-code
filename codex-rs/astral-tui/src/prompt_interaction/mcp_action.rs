//! Empty-schema MCP approvals whose semantics are carried in elicitation metadata.

use std::time::Instant;

use codex_app_server_protocol::McpServerElicitationAction;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::McpServerElicitationRequestResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_protocol::mcp_approval_meta::APPROVAL_KIND_KEY;
use codex_protocol::mcp_approval_meta::APPROVAL_KIND_MCP_TOOL_CALL;
use codex_protocol::mcp_approval_meta::APPROVAL_KIND_TOOL_SUGGESTION;
use codex_protocol::mcp_approval_meta::PERSIST_ALWAYS;
use codex_protocol::mcp_approval_meta::PERSIST_KEY;
use codex_protocol::mcp_approval_meta::PERSIST_SESSION;
use codex_protocol::mcp_approval_meta::TOOL_NAME_KEY;
use codex_protocol::mcp_approval_meta::TOOL_PARAMS_DISPLAY_KEY;
use codex_protocol::mcp_approval_meta::TOOL_PARAMS_KEY;
use codex_protocol::mcp_approval_meta::TOOL_TITLE_KEY;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;
use serde_json::Value;

use super::choice_list::ChoiceList;
use super::choice_list::ChoiceListOutcome;
use crate::ModalOutcome;
use crate::ModalWindow;
use crate::prompt_interaction::PromptInteractionOutcome;
use crate::prompt_interaction::PromptInteractionSubmission;

mod render;

const DISPLAY_PARAM_LIMIT: usize = 3;

#[derive(Clone)]
struct Response {
    action: McpServerElicitationAction,
    meta: Option<Value>,
}

struct ActionOption {
    label: String,
    response: Response,
}

pub(super) struct McpActionPrompt {
    request_id: RequestId,
    server_name: String,
    title: String,
    body: Vec<String>,
    options: Vec<ActionOption>,
    escape: Response,
    choices: ChoiceList,
    window: ModalWindow,
}

impl McpActionPrompt {
    pub(super) fn from_request(request: &ServerRequest) -> Option<Self> {
        let ServerRequest::McpServerElicitationRequest { request_id, params } = request else {
            return None;
        };
        let McpServerElicitationRequest::Form {
            meta,
            message,
            requested_schema,
        } = &params.request
        else {
            return None;
        };
        if !requested_schema.properties.is_empty() {
            return None;
        }
        let meta = meta.as_ref().and_then(Value::as_object);
        if meta
            .and_then(|meta| meta.get(APPROVAL_KIND_KEY))
            .and_then(Value::as_str)
            == Some(APPROVAL_KIND_TOOL_SUGGESTION)
        {
            return None;
        }
        Some(Self::approval(
            request_id.clone(),
            &params.server_name,
            message,
            meta,
        ))
    }

    fn approval(
        request_id: RequestId,
        server_name: &str,
        message: &str,
        meta: Option<&serde_json::Map<String, Value>>,
    ) -> Self {
        let tool_approval = meta
            .and_then(|meta| meta.get(APPROVAL_KIND_KEY))
            .and_then(Value::as_str)
            == Some(APPROVAL_KIND_MCP_TOOL_CALL);
        let mut body = vec![message.trim().to_string()];
        if tool_approval {
            body.extend(display_params(meta));
        }
        let accept = |label: &str, persist: Option<&str>| ActionOption {
            label: label.to_string(),
            response: Response {
                action: McpServerElicitationAction::Accept,
                meta: persist.map(|persist| serde_json::json!({PERSIST_KEY: persist})),
            },
        };
        let mut options = vec![accept(
            if tool_approval { "Run tool" } else { "Allow" },
            None,
        )];
        if supports_persist(meta, PERSIST_SESSION) {
            options.push(accept("Allow for this session", Some(PERSIST_SESSION)));
        }
        if supports_persist(meta, PERSIST_ALWAYS) {
            options.push(accept("Always allow", Some(PERSIST_ALWAYS)));
        }
        if !tool_approval {
            options.push(ActionOption {
                label: "Deny".to_string(),
                response: Response {
                    action: McpServerElicitationAction::Decline,
                    meta: None,
                },
            });
        }
        let cancel = Response {
            action: McpServerElicitationAction::Cancel,
            meta: None,
        };
        options.push(ActionOption {
            label: "Cancel".to_string(),
            response: cancel.clone(),
        });
        Self {
            request_id,
            server_name: server_name.to_string(),
            title: if tool_approval {
                meta.and_then(|meta| meta.get(TOOL_TITLE_KEY).or_else(|| meta.get(TOOL_NAME_KEY)))
                    .and_then(Value::as_str)
                    .map_or_else(
                        || "Allow MCP tool?".to_string(),
                        |tool| format!("Run {tool}?"),
                    )
            } else {
                "Allow MCP request?".to_string()
            },
            body,
            options,
            escape: cancel,
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

    pub(super) fn handle_key_event(&mut self, key: KeyEvent) -> PromptInteractionOutcome {
        if key.kind == KeyEventKind::Release {
            return PromptInteractionOutcome::Unchanged;
        }
        if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE {
            return self.submit(self.escape.clone());
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
            ModalOutcome::CloseRequested => return self.submit(self.escape.clone()),
            ModalOutcome::Handled | ModalOutcome::ShortcutActivated(_) => {
                return PromptInteractionOutcome::Changed;
            }
            ModalOutcome::TabChanged(_) | ModalOutcome::Unhandled => {}
        }
        let outcome = self.choices.handle_mouse(mouse, now, self.options.len());
        self.handle_choice_outcome(outcome)
    }

    fn handle_choice_outcome(&mut self, outcome: ChoiceListOutcome) -> PromptInteractionOutcome {
        match outcome {
            ChoiceListOutcome::Unchanged => PromptInteractionOutcome::Unchanged,
            ChoiceListOutcome::Changed => PromptInteractionOutcome::Changed,
            ChoiceListOutcome::Activate(index) => {
                let Some(response) = self
                    .options
                    .get(index)
                    .map(|option| option.response.clone())
                else {
                    return PromptInteractionOutcome::Unchanged;
                };
                self.submit(response)
            }
        }
    }

    fn submit(&self, response: Response) -> PromptInteractionOutcome {
        match serde_json::to_value(McpServerElicitationRequestResponse {
            action: response.action,
            content: None,
            meta: response.meta,
        }) {
            Ok(result) => PromptInteractionOutcome::Submit(PromptInteractionSubmission {
                request_id: self.request_id.clone(),
                result,
            }),
            Err(error) => PromptInteractionOutcome::Failed(format!(
                "failed to serialize MCP action elicitation response: {error}"
            )),
        }
    }
}

fn supports_persist(meta: Option<&serde_json::Map<String, Value>>, expected: &str) -> bool {
    match meta.and_then(|meta| meta.get(PERSIST_KEY)) {
        Some(Value::String(value)) => value == expected,
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .any(|value| value == expected),
        _ => false,
    }
}

fn display_params(meta: Option<&serde_json::Map<String, Value>>) -> Vec<String> {
    let Some(meta) = meta else {
        return Vec::new();
    };
    let mut params = meta
        .get(TOOL_PARAMS_DISPLAY_KEY)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| {
                    let value = value.as_object()?;
                    let name = value
                        .get("display_name")
                        .or_else(|| value.get("name"))?
                        .as_str()?
                        .trim();
                    let param_value = value.get("value")?.clone();
                    (!name.is_empty()).then(|| (name.to_string(), param_value))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if params.is_empty() {
        params = meta
            .get(TOOL_PARAMS_KEY)
            .and_then(Value::as_object)
            .map(|params| {
                let mut params = params
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect::<Vec<_>>();
                params.sort_by(|left, right| left.0.cmp(&right.0));
                params
            })
            .unwrap_or_default();
    }
    let omitted = params.len().saturating_sub(DISPLAY_PARAM_LIMIT);
    let mut lines = params
        .into_iter()
        .take(DISPLAY_PARAM_LIMIT)
        .map(|(name, value)| {
            let value = match value {
                Value::String(value) => value.split_whitespace().collect::<Vec<_>>().join(" "),
                value => value.to_string(),
            };
            let mut chars = value.chars();
            let prefix = chars.by_ref().take(80).collect::<String>();
            let value = if chars.next().is_some() {
                format!("{prefix}…")
            } else {
                prefix
            };
            format!("{name}: {value}")
        })
        .collect::<Vec<_>>();
    if omitted > 0 {
        lines.push(format!("… {omitted} more parameters"));
    }
    lines
}
