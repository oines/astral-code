use std::sync::Arc;
use std::sync::Mutex;

use codex_config::config_toml::SecretString;
use codex_config::config_toml::WebSearchProvider;
use codex_config::config_toml::WebSearchRuntimeConfig;
use codex_protocol::items::WebSearchItem;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::protocol::TruncationPolicy;
use codex_tools::ConversationHistory;
use codex_tools::ExtensionTurnItem;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ToolCall;
use codex_tools::ToolExecutor;
use codex_tools::ToolName;
use codex_tools::ToolOutput;
use codex_tools::ToolPayload;
use codex_tools::ToolSpec;
use codex_tools::TurnItemEmissionFuture;
use codex_tools::TurnItemEmitter;
use pretty_assertions::assert_eq;

use super::FETCH_TOOL_NAME;
use super::SEARCH_TOOL_NAME;
use super::WEB_FETCH_DESCRIPTION;
use super::WEB_NAMESPACE;
use super::WEB_SEARCH_DESCRIPTION;
use super::WebFetchInput;
use super::WebFetchTool;
use super::WebSearchInput;
use super::WebSearchTool;
use super::format_search_results;
use super::web_tool_spec;
use crate::provider::WebSearchResult;

#[derive(Debug, Clone, PartialEq)]
enum RecordedTurnItem {
    Started(WebSearchItem),
    Completed(WebSearchItem),
}

#[derive(Clone, Default)]
struct RecordingTurnItemEmitter {
    items: Arc<Mutex<Vec<RecordedTurnItem>>>,
}

impl RecordingTurnItemEmitter {
    fn items(&self) -> Vec<RecordedTurnItem> {
        self.items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl TurnItemEmitter for RecordingTurnItemEmitter {
    fn emit_started<'a>(&'a self, item: ExtensionTurnItem) -> TurnItemEmissionFuture<'a> {
        let ExtensionTurnItem::WebSearch(item) = item else {
            return Box::pin(std::future::ready(()));
        };
        self.items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(RecordedTurnItem::Started(item));
        Box::pin(std::future::ready(()))
    }

    fn emit_completed<'a>(&'a self, item: ExtensionTurnItem) -> TurnItemEmissionFuture<'a> {
        let ExtensionTurnItem::WebSearch(item) = item else {
            return Box::pin(std::future::ready(()));
        };
        self.items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(RecordedTurnItem::Completed(item));
        Box::pin(std::future::ready(()))
    }
}

#[test]
fn web_search_schema_exposes_simple_query_and_limit_shape() {
    let ToolSpec::Namespace(namespace) =
        web_tool_spec::<WebSearchInput>(SEARCH_TOOL_NAME, WEB_SEARCH_DESCRIPTION)
    else {
        panic!("expected namespace spec");
    };
    let [ResponsesApiNamespaceTool::Function(tool)] = namespace.tools.as_slice() else {
        panic!("expected a single function");
    };

    assert_eq!(namespace.name, WEB_NAMESPACE);
    assert_eq!(tool.name, SEARCH_TOOL_NAME);
    let properties = tool
        .parameters
        .properties
        .as_ref()
        .expect("properties should exist");
    assert!(properties.contains_key("query"));
    assert!(properties.contains_key("limit"));
    assert_eq!(properties.len(), 2);
}

#[test]
fn web_fetch_schema_exposes_simple_url_and_format_shape() {
    let ToolSpec::Namespace(namespace) =
        web_tool_spec::<WebFetchInput>(FETCH_TOOL_NAME, WEB_FETCH_DESCRIPTION)
    else {
        panic!("expected namespace spec");
    };
    let [ResponsesApiNamespaceTool::Function(tool)] = namespace.tools.as_slice() else {
        panic!("expected a single function");
    };

    assert_eq!(namespace.name, WEB_NAMESPACE);
    assert_eq!(tool.name, FETCH_TOOL_NAME);
    let properties = tool
        .parameters
        .properties
        .as_ref()
        .expect("properties should exist");
    assert!(properties.contains_key("url"));
    assert!(properties.contains_key("format"));
    assert_eq!(properties.len(), 2);
}

#[test]
fn formats_search_results_as_plain_text() {
    let output = format_search_results(
        "rust news",
        &[WebSearchResult {
            title: "Rust Result".to_string(),
            url: "https://example.com/rust".to_string(),
            snippet: Some("A useful snippet.".to_string()),
            published_at: Some("2026-01-01".to_string()),
            score: Some(0.9),
        }],
    );

    assert_eq!(
        output,
        "Search query: \"rust news\"\nResults returned: 1\n\n1. Title: Rust Result\n   URL: https://example.com/rust\n   Published: 2026-01-01\n   Snippet: A useful snippet.\n"
    );
}

#[tokio::test]
async fn web_search_failure_returns_model_output_and_completes_visible_item() {
    let emitter = RecordingTurnItemEmitter::default();
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all("http://127.0.0.1:9").expect("proxy URL should parse"))
        .timeout(std::time::Duration::from_millis(100))
        .build()
        .expect("client should build");
    let tool = WebSearchTool {
        client,
        config: WebSearchRuntimeConfig {
            provider: WebSearchProvider::Tavily,
            api_key: SecretString::new("secret".to_string()).expect("secret should be valid"),
            default_limit: 5,
            max_limit: 20,
        },
    };
    let call = ToolCall {
        turn_id: "turn-1".to_string(),
        call_id: "call-web-search".to_string(),
        tool_name: ToolName::namespaced(WEB_NAMESPACE, SEARCH_TOOL_NAME),
        model: "test-model".to_string(),
        truncation_policy: TruncationPolicy::Bytes(1024),
        conversation_history: ConversationHistory::default(),
        turn_item_emitter: Arc::new(emitter.clone()),
        payload: ToolPayload::Function {
            arguments: serde_json::json!({
                "query": "rust news",
                "limit": 5,
            })
            .to_string(),
        },
    };

    let output = tool.handle(call).await.expect("tool should not fatal");

    let response = output.to_response_item(
        "call-web-search",
        &ToolPayload::Function {
            arguments: "{}".to_string(),
        },
    );
    let ResponseInputItem::FunctionCallOutput { output, .. } = response else {
        panic!("expected function call output");
    };
    assert_eq!(output.success, Some(false));
    let body = output
        .body
        .to_text()
        .expect("output should be readable text");
    assert!(body.contains("Web search failed for query \"rust news\""));
    assert_eq!(
        emitter
            .items()
            .into_iter()
            .map(|item| match item {
                RecordedTurnItem::Started(item) => ("started", item.id),
                RecordedTurnItem::Completed(item) => ("completed", item.id),
            })
            .collect::<Vec<_>>(),
        vec![
            ("started", "call-web-search".to_string()),
            ("completed", "call-web-search".to_string()),
        ]
    );
}

#[tokio::test]
async fn web_fetch_failure_returns_model_output() {
    let emitter = RecordingTurnItemEmitter::default();
    let tool = WebFetchTool {
        client: reqwest::Client::new(),
    };
    let call = ToolCall {
        turn_id: "turn-1".to_string(),
        call_id: "call-web-fetch".to_string(),
        tool_name: ToolName::namespaced(WEB_NAMESPACE, FETCH_TOOL_NAME),
        model: "test-model".to_string(),
        truncation_policy: TruncationPolicy::Bytes(1024),
        conversation_history: ConversationHistory::default(),
        turn_item_emitter: Arc::new(emitter.clone()),
        payload: ToolPayload::Function {
            arguments: serde_json::json!({
                "url": "http://127.0.0.1:8080",
                "format": "markdown",
            })
            .to_string(),
        },
    };

    let output = tool.handle(call).await.expect("tool should not fatal");
    let response = output.to_response_item(
        "call-web-fetch",
        &ToolPayload::Function {
            arguments: "{}".to_string(),
        },
    );
    let ResponseInputItem::FunctionCallOutput { output, .. } = response else {
        panic!("expected function call output");
    };
    assert_eq!(output.success, Some(false));
    let body = output
        .body
        .to_text()
        .expect("output should be readable text");
    assert!(body.contains("Web fetch failed: refusing to fetch private address"));
    assert_eq!(
        emitter
            .items()
            .into_iter()
            .map(|item| match item {
                RecordedTurnItem::Started(item) => ("started", item.id, item.query),
                RecordedTurnItem::Completed(item) => ("completed", item.id, item.query),
            })
            .collect::<Vec<_>>(),
        vec![(
            "completed",
            "call-web-fetch".to_string(),
            "http://127.0.0.1:8080".to_string(),
        )]
    );
}
