use std::collections::BTreeMap;

use anyhow::Result;
use codex_login::CodexAuth;
use codex_models_manager::capabilities::ModelCapabilitiesCache;
use codex_models_manager::capabilities::ModelCapability;
use codex_models_manager::manager::RefreshStrategy;
use codex_models_manager::manager::SharedModelsManager;
use codex_models_manager::model_info::model_info_from_slug;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ToolMode;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_models_once;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::request_tool_names;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::submit_thread_settings;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use tokio::time::Duration;
use tokio::time::Instant;
use tokio::time::sleep;

const CODE_MODE_MODEL: &str = "test-config-code-mode";
const DIRECT_MODEL: &str = "test-config-direct";

fn remote_model(slug: &str) -> ModelInfo {
    ModelInfo {
        visibility: ModelVisibility::List,
        used_fallback_model_metadata: false,
        ..model_info_from_slug(slug)
    }
}

async fn wait_for_models(manager: &SharedModelsManager) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let models = manager.list_models(RefreshStrategy::Online).await;
        if [DIRECT_MODEL, CODE_MODE_MODEL].iter().all(|slug| {
            models
                .iter()
                .any(|available_model| available_model.model == *slug)
        }) {
            return;
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for configured tool-mode models");
        }
        sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn configured_tool_mode_follows_the_model_selected_for_each_turn() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = wiremock::MockServer::start().await;
    let models_mock = mount_models_once(
        &server,
        ModelsResponse {
            models: vec![remote_model(DIRECT_MODEL), remote_model(CODE_MODE_MODEL)],
        },
    )
    .await;
    let response_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-direct"),
                ev_assistant_message("msg-direct", "done"),
                ev_completed("resp-direct"),
            ]),
            sse(vec![
                ev_response_created("resp-code-mode"),
                ev_assistant_message("msg-code-mode", "done"),
                ev_completed("resp-code-mode"),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_auth(CodexAuth::create_dummy_api_key_auth_for_testing())
        .with_config(|config| {
            config.model = Some(DIRECT_MODEL.to_string());
            let model_key = format!("{}/{CODE_MODE_MODEL}", config.model_provider_id);
            config.model_capability_overrides = Some(ModelCapabilitiesCache {
                version: 1,
                source: "config.toml".to_string(),
                generated_at_unix_seconds: 0,
                models: BTreeMap::from([(
                    model_key,
                    ModelCapability {
                        tool_mode: Some(ToolMode::CodeMode),
                        ..Default::default()
                    },
                )]),
            });
        })
        .build(&server)
        .await?;
    wait_for_models(&test.thread_manager.get_models_manager()).await;
    assert_eq!(models_mock.requests().len(), 1);

    test.submit_turn("use the direct model").await?;
    submit_thread_settings(
        &test.codex,
        ThreadSettingsOverrides {
            model: Some(CODE_MODE_MODEL.to_string()),
            ..Default::default()
        },
    )
    .await?;
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "use the code-mode model".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: ThreadSettingsOverrides::default(),
        })
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let requests = response_mock.requests();
    assert_eq!(requests.len(), 2);
    let direct_tools = request_tool_names(&requests[0].body_json());
    let code_mode_tools = request_tool_names(&requests[1].body_json());
    assert!(
        direct_tools
            .iter()
            .all(|name| name != codex_code_mode::PUBLIC_TOOL_NAME
                && name != codex_code_mode::WAIT_TOOL_NAME),
        "unconfigured model should stay direct: {direct_tools:?}"
    );
    assert!(
        [
            codex_code_mode::PUBLIC_TOOL_NAME,
            codex_code_mode::WAIT_TOOL_NAME,
        ]
        .iter()
        .all(|name| code_mode_tools.iter().any(|tool| tool == name)),
        "configured model should advertise code-mode entrypoints: {code_mode_tools:?}"
    );

    Ok(())
}
