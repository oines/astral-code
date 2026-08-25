use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

use codex_api::SearchCommands;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthDotJson;
use codex_login::AuthManager;
use codex_login::TokenData;
use codex_login::save_codex_oauth_auth;
use codex_login::token_data::parse_chatgpt_jwt_claims;
use codex_model_provider::create_model_provider;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::items::WebSearchItem;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::TranscriptInputItem;
use codex_protocol::models::TranscriptItem;
use codex_protocol::models::WebSearchAction;
use codex_protocol::protocol::TruncationPolicy;
use codex_tools::ConversationHistory;
use codex_tools::ExtensionTurnItem;
use codex_tools::ToolCall;
use codex_tools::ToolExecutor;
use codex_tools::ToolName;
use codex_tools::ToolOutput;
use codex_tools::ToolPayload;
use codex_tools::TurnItemEmissionFuture;
use codex_tools::TurnItemEmitter;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header_regex;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::CodexWebSearchTool;
use super::RUN_TOOL_NAME;
use super::WEB_NAMESPACE;
use super::command_action;

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

#[tokio::test]
async fn web_run_uses_codex_oauth_and_emits_history_items() -> Result<(), Box<dyn Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/alpha/search"))
        .and(header_regex("Authorization", "Bearer codex-access"))
        .and(header_regex("ChatGPT-Account-ID", "workspace-123"))
        .and(header_regex("originator", "codex_cli_rs"))
        .and(header_regex("x-astral-turn-metadata", "turn-metadata"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "output": "search result",
        })))
        .expect(1)
        .mount(&server)
        .await;
    let auth_home = tempfile::tempdir()?;
    let auth_manager = codex_oauth_auth_manager(auth_home.path()).await?;
    let mut provider = ModelProviderInfo::create_codex_provider();
    provider.base_url = Some(server.uri());
    let emitter = RecordingTurnItemEmitter::default();
    let tool = CodexWebSearchTool {
        session_id: "search-session".to_string(),
        provider: create_model_provider(provider, Some(auth_manager)),
        settings: Default::default(),
    };
    let payload = ToolPayload::Function {
        arguments: json!({"search_query": [{"q": "OpenAI news"}]}).to_string(),
    };
    let output = tool
        .handle(ToolCall {
            turn_id: "turn-search".to_string(),
            call_id: "call-search".to_string(),
            tool_name: ToolName::namespaced(WEB_NAMESPACE, RUN_TOOL_NAME),
            model: "gpt-test".to_string(),
            codex_turn_metadata: Some("turn-metadata".to_string()),
            truncation_policy: TruncationPolicy::Tokens(2_500),
            conversation_history: ConversationHistory::new(vec![TranscriptItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "find the news".to_string(),
                }],
                phase: None,
            }]),
            turn_item_emitter: Arc::new(emitter.clone()),
            payload: payload.clone(),
        })
        .await?;

    let TranscriptInputItem::FunctionCallOutput { output, .. } =
        output.to_response_item("call-search", &payload)
    else {
        return Err("web.run did not return function call output".into());
    };
    let FunctionCallOutputBody::ContentItems(items) = output.body else {
        return Err("web.run output was not content items".into());
    };
    assert_eq!(
        items,
        vec![FunctionCallOutputContentItem::InputText {
            text: "search result".to_string(),
        }]
    );
    assert_eq!(
        emitter.items(),
        vec![
            RecordedTurnItem::Started(WebSearchItem {
                id: "call-search".to_string(),
                query: String::new(),
                action: WebSearchAction::Other,
            }),
            RecordedTurnItem::Completed(WebSearchItem {
                id: "call-search".to_string(),
                query: "OpenAI news".to_string(),
                action: WebSearchAction::Search {
                    query: Some("OpenAI news".to_string()),
                    queries: None,
                },
            }),
        ]
    );
    let requests = server
        .received_requests()
        .await
        .ok_or("mock server did not record web request")?;
    let body: Value = serde_json::from_slice(&requests[0].body)?;
    assert_eq!(body["id"], "search-session");
    assert_eq!(body["model"], "gpt-test");
    assert_eq!(
        body["commands"],
        json!({"search_query": [{"q": "OpenAI news"}]})
    );
    assert_eq!(body["input"][0]["role"], "user");
    assert_eq!(body["max_output_tokens"], 2_500);
    Ok(())
}

#[test]
fn command_action_reports_queries_and_navigation_detail() {
    let cases = [
        (
            r#"{"image_query":[{"q":"waterfalls"},{"q":"mountains"}]}"#,
            WebSearchAction::Search {
                query: None,
                queries: Some(vec!["waterfalls".to_string(), "mountains".to_string()]),
            },
        ),
        (
            r#"{"open":[{"ref_id":"https://example.com/docs"}]}"#,
            WebSearchAction::OpenPage {
                url: Some("https://example.com/docs".to_string()),
            },
        ),
        (
            r#"{"find":[{"ref_id":"turn0search0","pattern":"install"}]}"#,
            WebSearchAction::FindInPage {
                url: None,
                pattern: Some("install".to_string()),
            },
        ),
    ];

    for (arguments, expected) in cases {
        let commands: SearchCommands =
            serde_json::from_str(arguments).expect("valid search command arguments");
        assert_eq!(command_action(&commands), expected);
    }
}

async fn codex_oauth_auth_manager(home: &Path) -> Result<Arc<AuthManager>, Box<dyn Error>> {
    let id_token = concat!(
        "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.",
        "eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20iLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgi",
        "OnsiY2hhdGdwdF9hY2NvdW50X2lkIjoid29ya3NwYWNlLTEyMyIsImNoYXRncHRfcGxhbl90eXBl",
        "IjoicGx1cyJ9fQ.sig"
    );
    save_codex_oauth_auth(
        home,
        &AuthDotJson {
            auth_mode: Some("chatgpt".to_string()),
            api_key: None,
            tokens: Some(TokenData {
                id_token: parse_chatgpt_jwt_claims(id_token)?,
                access_token: "codex-access".to_string(),
                refresh_token: "codex-refresh".to_string(),
                account_id: Some("workspace-123".to_string()),
            }),
            last_refresh: None,
        },
        AuthCredentialsStoreMode::File,
    )?;
    Ok(Arc::new(
        AuthManager::new(
            home.to_path_buf(),
            /*enable_astral_api_key_env*/ false,
            AuthCredentialsStoreMode::File,
        )
        .await,
    ))
}
