//! Empty-schema MCP actions whose semantics are carried in elicitation metadata.

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
use super::mcp_url::validate_external_url;
use crate::ModalOutcome;
use crate::ModalWindow;
use crate::prompt_interaction::PromptInteractionOutcome;
use crate::prompt_interaction::PromptInteractionSubmission;

mod render;

const TOOL_TYPE_KEY: &str = "tool_type";
const TOOL_ID_KEY: &str = "tool_id";
const SUGGEST_TYPE_KEY: &str = "suggest_type";
const SUGGEST_REASON_KEY: &str = "suggest_reason";
const INSTALL_URL_KEY: &str = "install_url";
const DISPLAY_PARAM_LIMIT: usize = 3;

#[derive(Clone)]
struct Response {
    action: McpServerElicitationAction,
    meta: Option<Value>,
}

#[derive(Clone)]
enum ChoiceAction {
    Respond(Response),
    OpenExternalUrl(String),
    Back,
}

struct ActionOption {
    label: String,
    action: ChoiceAction,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Ready,
    WaitingForBrowser,
}

pub(super) struct McpActionPrompt {
    request_id: RequestId,
    server_name: String,
    title: String,
    body: Vec<String>,
    displayed_url: Option<String>,
    safe_url: Option<String>,
    ready_options: Vec<ActionOption>,
    waiting_options: Vec<ActionOption>,
    escape: Response,
    stage: Stage,
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
            && let Some(meta) = meta
            && let Some(prompt) =
                Self::tool_suggestion(request_id.clone(), &params.server_name, message, meta)
        {
            return Some(prompt);
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
            action: ChoiceAction::Respond(Response {
                action: McpServerElicitationAction::Accept,
                meta: persist.map(|persist| serde_json::json!({PERSIST_KEY: persist})),
            }),
        };
        let mut ready_options = vec![accept(
            if tool_approval { "Run tool" } else { "Allow" },
            None,
        )];
        if supports_persist(meta, PERSIST_SESSION) {
            ready_options.push(accept("Allow for this session", Some(PERSIST_SESSION)));
        }
        if supports_persist(meta, PERSIST_ALWAYS) {
            ready_options.push(accept("Always allow", Some(PERSIST_ALWAYS)));
        }
        if !tool_approval {
            ready_options.push(ActionOption {
                label: "Deny".to_string(),
                action: ChoiceAction::Respond(Response {
                    action: McpServerElicitationAction::Decline,
                    meta: None,
                }),
            });
        }
        let cancel = Response {
            action: McpServerElicitationAction::Cancel,
            meta: None,
        };
        ready_options.push(ActionOption {
            label: "Cancel".to_string(),
            action: ChoiceAction::Respond(cancel.clone()),
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
            displayed_url: None,
            safe_url: None,
            ready_options,
            waiting_options: Vec::new(),
            escape: cancel,
            stage: Stage::Ready,
            choices: ChoiceList::default(),
            window: ModalWindow::default(),
        }
    }

    fn tool_suggestion(
        request_id: RequestId,
        server_name: &str,
        message: &str,
        meta: &serde_json::Map<String, Value>,
    ) -> Option<Self> {
        let tool_name = meta.get(TOOL_NAME_KEY)?.as_str()?.trim();
        let tool_id = meta.get(TOOL_ID_KEY)?.as_str()?.trim();
        let tool_type = meta.get(TOOL_TYPE_KEY)?.as_str()?.trim();
        let suggest_type = meta.get(SUGGEST_TYPE_KEY)?.as_str()?.trim();
        let reason = meta.get(SUGGEST_REASON_KEY)?.as_str()?.trim();
        if tool_name.is_empty()
            || tool_id.is_empty()
            || !matches!(tool_type, "connector" | "plugin")
            || !matches!(suggest_type, "install" | "enable")
        {
            return None;
        }
        let raw_url = meta.get(INSTALL_URL_KEY).and_then(Value::as_str);
        let safe_url = raw_url.and_then(validate_external_url);
        let displayed_url = raw_url.map(ToString::to_string);
        let verb = if suggest_type == "enable" {
            "Enable"
        } else {
            "Install"
        };
        let mut body = vec![message.trim().to_string()];
        if reason != message.trim() {
            body.push(reason.to_string());
        }
        body.push(format!("{verb} {tool_type}: {tool_name}"));
        let accept = Response {
            action: McpServerElicitationAction::Accept,
            meta: None,
        };
        let decline = Response {
            action: McpServerElicitationAction::Decline,
            meta: None,
        };
        let mut ready_options = Vec::new();
        match (raw_url, safe_url.as_ref()) {
            (Some(_), Some(url)) => ready_options.push(ActionOption {
                label: format!("Open {verb} page"),
                action: ChoiceAction::OpenExternalUrl(url.clone()),
            }),
            (None, _) => ready_options.push(ActionOption {
                label: verb.to_string(),
                action: ChoiceAction::Respond(accept.clone()),
            }),
            (Some(_), None) => {}
        }
        ready_options.push(ActionOption {
            label: "Not now".to_string(),
            action: ChoiceAction::Respond(decline.clone()),
        });
        if supports_persist(Some(meta), PERSIST_ALWAYS) {
            ready_options.push(ActionOption {
                label: "Don't suggest this again".to_string(),
                action: ChoiceAction::Respond(Response {
                    action: McpServerElicitationAction::Decline,
                    meta: Some(serde_json::json!({PERSIST_KEY: PERSIST_ALWAYS})),
                }),
            });
        }
        Some(Self {
            request_id,
            server_name: server_name.to_string(),
            title: format!("{verb} {tool_name}?"),
            body,
            displayed_url,
            safe_url,
            ready_options,
            waiting_options: vec![
                ActionOption {
                    label: if suggest_type == "enable" {
                        "I enabled it".to_string()
                    } else {
                        "I installed it".to_string()
                    },
                    action: ChoiceAction::Respond(accept),
                },
                ActionOption {
                    label: "Back".to_string(),
                    action: ChoiceAction::Back,
                },
            ],
            escape: decline,
            stage: Stage::Ready,
            choices: ChoiceList::default(),
            window: ModalWindow::default(),
        })
    }

    pub(super) fn selected_index(&self) -> usize {
        self.choices.selected()
    }

    pub(super) fn set_selected_index(&mut self, selected: usize) {
        self.choices.set_selected(selected, self.options().len());
    }

    pub(super) fn handle_key_event(&mut self, key: KeyEvent) -> PromptInteractionOutcome {
        if key.kind == KeyEventKind::Release {
            return PromptInteractionOutcome::Unchanged;
        }
        if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE {
            return self.submit(self.escape.clone());
        }
        let outcome = self.choices.handle_key(key, self.options().len());
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
        let outcome = self.choices.handle_mouse(mouse, now, self.options().len());
        self.handle_choice_outcome(outcome)
    }

    fn handle_choice_outcome(&mut self, outcome: ChoiceListOutcome) -> PromptInteractionOutcome {
        match outcome {
            ChoiceListOutcome::Unchanged => PromptInteractionOutcome::Unchanged,
            ChoiceListOutcome::Changed => PromptInteractionOutcome::Changed,
            ChoiceListOutcome::Activate(index) => {
                let Some(action) = self
                    .options()
                    .get(index)
                    .map(|option| option.action.clone())
                else {
                    return PromptInteractionOutcome::Unchanged;
                };
                match action {
                    ChoiceAction::Respond(response) => self.submit(response),
                    ChoiceAction::OpenExternalUrl(url) => {
                        self.stage = Stage::WaitingForBrowser;
                        self.choices.set_selected(0, self.waiting_options.len());
                        PromptInteractionOutcome::OpenExternalUrl { url }
                    }
                    ChoiceAction::Back => {
                        self.stage = Stage::Ready;
                        self.choices.set_selected(0, self.ready_options.len());
                        PromptInteractionOutcome::Changed
                    }
                }
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

    fn options(&self) -> &[ActionOption] {
        match self.stage {
            Stage::Ready => &self.ready_options,
            Stage::WaitingForBrowser => &self.waiting_options,
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
