use codex_app_server_protocol::DynamicToolCallOutputContentItem;
use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::DynamicToolCallResponse;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::ClientToolRegistry;

fn params(namespace: Option<&str>, tool: &str) -> DynamicToolCallParams {
    DynamicToolCallParams {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        call_id: "call-1".to_string(),
        namespace: namespace.map(str::to_string),
        tool: tool.to_string(),
        arguments: json!({"path": "report.html"}),
    }
}

#[tokio::test]
async fn dispatches_by_exact_namespace_and_tool() {
    let mut registry = ClientToolRegistry::default();
    registry.register(
        Some("astral".to_string()),
        "open_artifact",
        |params| async move {
            Ok(DynamicToolCallResponse {
                content_items: vec![DynamicToolCallOutputContentItem::InputText {
                    text: params.arguments["path"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                }],
                success: true,
            })
        },
    );

    assert!(registry.contains(Some("astral"), "open_artifact"));
    assert_eq!(
        registry
            .call(params(Some("astral"), "open_artifact"))
            .await
            .expect("registered handler"),
        DynamicToolCallResponse {
            content_items: vec![DynamicToolCallOutputContentItem::InputText {
                text: "report.html".to_string(),
            }],
            success: true,
        }
    );
}

#[tokio::test]
async fn missing_handler_names_the_unhandled_tool() {
    let error = ClientToolRegistry::default()
        .call(params(Some("astral"), "open_artifact"))
        .await
        .expect_err("missing handler");

    assert_eq!(
        error.message,
        "no Astral client handler registered for astral/open_artifact"
    );
}
