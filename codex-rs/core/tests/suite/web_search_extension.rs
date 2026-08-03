use std::sync::Arc;

use codex_config::config_toml::SecretString;
use codex_config::config_toml::WebSearchProvider;
use codex_config::config_toml::WebSearchRuntimeConfig;
use codex_core::config::Config;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_login::CodexAuth;
use codex_protocol::config_types::WebSearchMode;
use codex_web_search_extension::install as install_web_search_extension;
use core_test_support::responses;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn optional_web_search_arguments_round_trip_through_the_agent() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call_with_namespace(
                    "search-1",
                    "web",
                    "search",
                    &json!({"query": "rust", "domains": ["rust-lang.org"], "limit": 3}).to_string(),
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;
    let auth = CodexAuth::from_api_key("dummy");
    let auth_manager = codex_core::test_support::auth_manager_from_auth(auth.clone());
    let mut extensions = ExtensionRegistryBuilder::<Config>::new();
    install_web_search_extension(&mut extensions, auth_manager);
    let test = test_codex()
        .with_auth(auth)
        .with_extensions(Arc::new(extensions.build()))
        .with_config(|config| {
            config
                .web_search_mode
                .set(WebSearchMode::Live)
                .expect("web search mode should be accepted");
            config.web_search_runtime_config = Some(WebSearchRuntimeConfig {
                provider: WebSearchProvider::SerpApi,
                api_key: SecretString::new("test-key".to_string()).expect("secret should be valid"),
            });
        })
        .build(&server)
        .await?;
    test.submit_turn("search the web").await?;

    let requests = mock.requests();
    let (content, _) = requests[1]
        .function_call_output_content_and_success("search-1")
        .expect("web search output should be returned to the model");
    assert_eq!(
        content.as_deref(),
        Some(
            "Web search failed for query \"rust\": SerpAPI Google search does not support limit; omit limit or configure another web search provider"
        )
    );

    Ok(())
}
