use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::UserInput;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::turn_start_request;

#[test]
fn default_turn_request_inherits_thread_settings() {
    let request = turn_start_request(
        RequestId::Integer(9),
        "thread-1".to_string(),
        vec![UserInput::Text {
            text: "hello".to_string(),
            text_elements: Vec::new(),
        }],
    );

    let ClientRequest::TurnStart { request_id, params } = request else {
        panic!("expected turn/start");
    };
    assert_eq!(request_id, RequestId::Integer(9));
    assert_eq!(
        serde_json::to_value(params).expect("serialize params"),
        json!({
            "threadId": "thread-1",
            "clientUserMessageId": null,
            "input": [{
                "type": "text",
                "text": "hello",
                "text_elements": []
            }],
            "modelClientMetadata": null,
            "additionalContext": null,
            "environments": null,
            "cwd": null,
            "runtimeWorkspaceRoots": null,
            "approvalPolicy": null,
            "approvalsReviewer": null,
            "sandboxPolicy": null,
            "permissions": null,
            "model": null,
            "modelProvider": null,
            "effort": null,
            "summary": null,
            "personality": null,
            "outputSchema": null,
            "collaborationMode": null
        })
    );
}
