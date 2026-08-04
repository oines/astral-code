//! Projection of empty-schema MCP approvals into the shared approval prompt.

use codex_app_server_protocol::McpServerElicitationAction;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::McpServerElicitationRequestParams;
use codex_app_server_protocol::RequestId;
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
use serde_json::Value;

use super::ApprovalBodyTone;
use super::ApprovalChoice;
use super::ApprovalOption;
use super::ApprovalPrompt;
use super::ChoiceList;
use super::ModalWindow;

const DISPLAY_PARAM_LIMIT: usize = 3;

#[derive(Clone)]
pub(super) struct Response {
    pub(super) action: McpServerElicitationAction,
    pub(super) meta: Option<Value>,
}

pub(super) fn from_request(
    request_id: RequestId,
    params: &McpServerElicitationRequestParams,
) -> Option<ApprovalPrompt> {
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
    let approval_kind = meta
        .and_then(|meta| meta.get(APPROVAL_KIND_KEY))
        .and_then(Value::as_str);
    if approval_kind == Some(APPROVAL_KIND_TOOL_SUGGESTION) {
        return None;
    }

    let tool_approval = approval_kind == Some(APPROVAL_KIND_MCP_TOOL_CALL);
    let mut body = vec![message.trim().to_string()];
    if tool_approval {
        body.extend(display_params(meta));
    }
    let accept = |label: &str, persist: Option<&str>| ApprovalOption {
        label: label.to_string(),
        choice: ApprovalChoice::Mcp(Response {
            action: McpServerElicitationAction::Accept,
            meta: persist.map(|persist| serde_json::json!({PERSIST_KEY: persist})),
        }),
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
        options.push(ApprovalOption {
            label: "Deny".to_string(),
            choice: ApprovalChoice::Mcp(Response {
                action: McpServerElicitationAction::Decline,
                meta: None,
            }),
        });
    }
    let cancel = ApprovalChoice::Mcp(Response {
        action: McpServerElicitationAction::Cancel,
        meta: None,
    });
    options.push(ApprovalOption {
        label: "Cancel".to_string(),
        choice: cancel.clone(),
    });

    let title = if tool_approval {
        meta.and_then(|meta| meta.get(TOOL_TITLE_KEY).or_else(|| meta.get(TOOL_NAME_KEY)))
            .and_then(Value::as_str)
            .map_or_else(
                || "Allow MCP tool?".to_string(),
                |tool| format!("Run {tool}?"),
            )
    } else {
        "Allow MCP request?".to_string()
    };
    Some(ApprovalPrompt {
        request_id,
        title: format!("{title} · {}", params.server_name),
        body,
        body_tone: ApprovalBodyTone::Plain,
        options,
        cancel,
        choices: ChoiceList::default(),
        window: ModalWindow::default(),
    })
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
        .map(|(name, value)| format!("{name}: {}", bounded_value(value)))
        .collect::<Vec<_>>();
    if omitted > 0 {
        lines.push(format!("… {omitted} more parameters"));
    }
    lines
}

fn bounded_value(value: Value) -> String {
    let value = match value {
        Value::String(value) => value.split_whitespace().collect::<Vec<_>>().join(" "),
        value => value.to_string(),
    };
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(80).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}
