use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::DynamicToolCallOutputContentItem;
use codex_app_server_protocol::DynamicToolCallResponse;
use codex_app_server_protocol::FileChangeApprovalDecision;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::PendingRequest;
use super::PendingRequestError;
use super::PendingRequestResponse;
use super::PendingRequests;
use super::RequestResolution;

fn server_request(value: serde_json::Value) -> ServerRequest {
    serde_json::from_value(value).expect("valid server request")
}

#[test]
fn queues_distinct_requests_and_replaces_replayed_payloads() {
    let mut requests = PendingRequests::default();
    requests.note(server_request(json!({
        "method": "item/commandExecution/requestApproval",
        "id": 7,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "startedAtMs": 100,
            "command": "cargo check"
        }
    })));
    requests.note(server_request(json!({
        "method": "item/commandExecution/requestApproval",
        "id": 7,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "startedAtMs": 100,
            "command": "just test"
        }
    })));
    requests.note(server_request(json!({
        "method": "item/fileChange/requestApproval",
        "id": "patch-1",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "edit-1",
            "startedAtMs": 101,
            "reason": "write source",
            "grantRoot": null
        }
    })));

    assert_eq!(requests.len(), 2);
    let Some(PendingRequest::CommandExecution { params, .. }) = requests.front() else {
        panic!("expected command approval first");
    };
    assert_eq!(params.command.as_deref(), Some("just test"));
    assert_eq!(
        requests.front().and_then(PendingRequest::thread_id),
        Some("thread-1")
    );
}

#[test]
fn preserves_command_and_file_decision_semantics() {
    let mut requests = PendingRequests::default();
    requests.note(server_request(json!({
        "method": "item/commandExecution/requestApproval",
        "id": 1,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "startedAtMs": 100
        }
    })));
    requests.note(server_request(json!({
        "method": "item/fileChange/requestApproval",
        "id": 2,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "edit-1",
            "startedAtMs": 101,
            "reason": null,
            "grantRoot": null
        }
    })));

    assert_eq!(
        requests.resolve(
            &RequestId::Integer(1),
            PendingRequestResponse::CommandExecution(
                CommandExecutionApprovalDecision::AcceptForSession,
            ),
        ),
        Ok(RequestResolution::Success {
            request_id: RequestId::Integer(1),
            result: json!({"decision": "acceptForSession"}),
        })
    );
    assert_eq!(
        requests.resolve(
            &RequestId::Integer(2),
            PendingRequestResponse::FileChange(FileChangeApprovalDecision::Cancel),
        ),
        Ok(RequestResolution::Success {
            request_id: RequestId::Integer(2),
            result: json!({"decision": "cancel"}),
        })
    );
    assert!(requests.is_empty());
}

#[test]
fn dynamic_tool_results_are_first_class_and_not_approvals() {
    let mut requests = PendingRequests::default();
    requests.note(server_request(json!({
        "method": "item/tool/call",
        "id": "dynamic-1",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "callId": "call-1",
            "namespace": "astral",
            "tool": "open_artifact",
            "arguments": {"path": "report.html"}
        }
    })));

    assert_eq!(
        requests.resolve(
            &RequestId::String("dynamic-1".to_string()),
            PendingRequestResponse::DynamicTool(DynamicToolCallResponse {
                content_items: vec![DynamicToolCallOutputContentItem::InputText {
                    text: "opened".to_string(),
                }],
                success: true,
            }),
        ),
        Ok(RequestResolution::Success {
            request_id: RequestId::String("dynamic-1".to_string()),
            result: json!({
                "contentItems": [{"type": "inputText", "text": "opened"}],
                "success": true
            }),
        })
    );
}

#[test]
fn mismatched_response_keeps_request_pending_and_any_request_can_be_rejected() {
    let mut requests = PendingRequests::default();
    let request_id = RequestId::String("edit-1".to_string());
    requests.note(server_request(json!({
        "method": "item/fileChange/requestApproval",
        "id": "edit-1",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "edit-1",
            "startedAtMs": 101,
            "reason": null,
            "grantRoot": null
        }
    })));

    assert_eq!(
        requests.resolve(
            &request_id,
            PendingRequestResponse::CommandExecution(CommandExecutionApprovalDecision::Accept),
        ),
        Err(PendingRequestError::WrongResponse {
            expected: "file change approval",
            received: "command execution approval",
        })
    );
    assert_eq!(requests.len(), 1);

    assert_eq!(
        requests.resolve(
            &request_id,
            PendingRequestResponse::Reject {
                code: -32000,
                message: "surface closed".to_string(),
            },
        ),
        Ok(RequestResolution::Reject {
            request_id,
            error: codex_app_server_protocol::JSONRPCErrorError {
                code: -32000,
                message: "surface closed".to_string(),
                data: None,
            },
        })
    );
    assert!(requests.is_empty());
}
