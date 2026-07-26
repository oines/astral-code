use std::sync::Arc;

use astral_tui::AstralSession;
use clap::Parser;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadSource;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_arg0::Arg0DispatchPaths;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_core::config::ConfigBuilder;
use codex_tui::Cli;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::AppServerTarget;
use super::ThreadConfigLoader;
use super::can_reuse_daemon;
use super::config_lookup_cwd;
use super::resolve_launch_model_provider;
use super::start_client;

#[test]
fn daemon_reuse_requires_replayable_config() {
    let default_loader = LoaderOverrides::default();
    assert!(can_reuse_daemon(
        &[],
        &default_loader,
        /*strict_config*/ false,
        /*bypass_hook_trust*/ false,
    ));

    let custom_loader = LoaderOverrides {
        ignore_user_config: true,
        ..LoaderOverrides::default()
    };
    assert!(!can_reuse_daemon(
        &[],
        &custom_loader,
        /*strict_config*/ false,
        /*bypass_hook_trust*/ false,
    ));
    assert!(!can_reuse_daemon(
        &[("model".to_string(), "gpt-test".into())],
        &default_loader,
        /*strict_config*/ false,
        /*bypass_hook_trust*/ false,
    ));
}

#[test]
fn remote_cwd_is_not_used_for_local_config_loading() {
    assert_eq!(
        config_lookup_cwd(
            Some(std::path::Path::new("/remote/workspace")),
            /*uses_remote_workspace*/ true,
        )
        .expect("remote cwd is opaque"),
        None
    );
}

#[test]
fn local_config_loading_uses_explicit_absolute_cwd() {
    let cwd = AbsolutePathBuf::current_dir().expect("current cwd");

    assert_eq!(
        config_lookup_cwd(Some(cwd.as_path()), /*uses_remote_workspace*/ false,)
            .expect("local cwd"),
        Some(cwd)
    );
}

#[tokio::test]
async fn historical_workspace_roots_append_explicit_add_dirs() {
    let codex_home = tempfile::tempdir().expect("temporary Astral home");
    let history = tempfile::tempdir().expect("historical cwd");
    let configured = tempfile::tempdir().expect("configured writable root");
    let additional = tempfile::tempdir().expect("additional writable root");
    let config_path = codex_home.path().join("config.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"
sandbox_mode = "workspace-write"

[sandbox_workspace_write]
writable_roots = [{}]

[projects.{}]
trust_level = "trusted"
"#,
            serde_json::json!(configured.path().to_string_lossy()),
            serde_json::json!(history.path().to_string_lossy()),
        ),
    )
    .expect("write config");
    let loader = ThreadConfigLoader {
        cli_kv_overrides: Vec::new(),
        loader_overrides: LoaderOverrides {
            user_config_path: Some(
                AbsolutePathBuf::try_from(config_path).expect("absolute config path"),
            ),
            ..LoaderOverrides::without_managed_config_for_tests()
        },
        strict_config: false,
        cloud_config_bundle: CloudConfigBundleLoader::default(),
    };
    let history =
        AbsolutePathBuf::try_from(history.path().to_path_buf()).expect("absolute history");

    let roots = loader
        .workspace_roots_for_cwd(&history, &[additional.path().to_path_buf()])
        .await
        .expect("load historical workspace roots");

    assert_eq!(
        roots,
        vec![
            history,
            AbsolutePathBuf::try_from(additional.path().to_path_buf())
                .expect("absolute additional root"),
            AbsolutePathBuf::try_from(configured.path().to_path_buf())
                .expect("absolute configured root"),
        ]
    );
}

#[tokio::test]
async fn oss_uses_the_configured_provider() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let config_path = temp_dir.path().join("config.toml");
    std::fs::write(&config_path, "oss_provider = \"ollama\"\n").expect("write config");
    let cli = Cli::parse_from([
        "astral",
        "--oss",
        "-C",
        temp_dir.path().to_str().expect("utf-8 path"),
    ]);
    let loader_overrides = LoaderOverrides {
        user_config_path: Some(AbsolutePathBuf::try_from(config_path).expect("absolute config")),
        ..LoaderOverrides::without_managed_config_for_tests()
    };

    let provider = resolve_launch_model_provider(
        &cli,
        &[],
        &loader_overrides,
        CloudConfigBundleLoader::default(),
        /*uses_remote_workspace*/ false,
    )
    .await
    .expect("resolve provider");

    assert_eq!(provider.as_deref(), Some("ollama"));
}

#[tokio::test]
async fn oss_without_configuration_defers_interactive_selection() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let config_path = temp_dir.path().join("config.toml");
    std::fs::write(&config_path, "").expect("write config");
    let cli = Cli::parse_from([
        "astral",
        "--oss",
        "-C",
        temp_dir.path().to_str().expect("utf-8 path"),
    ]);
    let loader_overrides = LoaderOverrides {
        user_config_path: Some(AbsolutePathBuf::try_from(config_path).expect("absolute config")),
        ..LoaderOverrides::without_managed_config_for_tests()
    };

    let provider = resolve_launch_model_provider(
        &cli,
        &[],
        &loader_overrides,
        CloudConfigBundleLoader::default(),
        /*uses_remote_workspace*/ false,
    )
    .await
    .expect("resolve provider");

    assert_eq!(provider, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedded_app_server_supports_astral_thread_lifecycle() {
    let codex_home = tempfile::tempdir().expect("temporary Astral home");
    let model_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                concat!(
                    "data: {\"id\":\"chatcmpl-1\",\"model\":\"gpt-5.2\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"done\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n",
                    "data: [DONE]\n\n",
                ),
                "text/event-stream",
            ),
        )
        .expect(1)
        .mount(&model_server)
        .await;
    let config_path = codex_home.path().join("config.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"
model = "gpt-5.2"
model_provider = "mock"

[model_providers.mock]
name = "Astral integration test"
base_url = "{}/v1"
wire_api = "chat_completions"
request_max_retries = 0
stream_max_retries = 0
"#,
            model_server.uri()
        ),
    )
    .expect("write isolated config");
    let loader_overrides = LoaderOverrides {
        user_config_path: Some(
            AbsolutePathBuf::try_from(config_path).expect("absolute config path"),
        ),
        ..LoaderOverrides::without_managed_config_for_tests()
    };
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .fallback_cwd(Some(codex_home.path().to_path_buf()))
        .loader_overrides(loader_overrides.clone())
        .build()
        .await
        .expect("build isolated config");
    let arg0_paths = Arg0DispatchPaths {
        codex_self_exe: Some(
            codex_utils_cargo_bin::cargo_bin("astral").expect("resolve Astral executable"),
        ),
        ..Arg0DispatchPaths::default()
    };
    let client = start_client(
        AppServerTarget::Embedded,
        arg0_paths,
        Arc::new(config),
        Vec::new(),
        loader_overrides,
        /*strict_config*/ false,
        CloudConfigBundleLoader::default(),
    )
    .await
    .expect("start embedded app-server");
    let mut session = AstralSession::new(client);
    let start = ThreadStartParams {
        model: Some("gpt-5.2".to_string()),
        cwd: Some(codex_home.path().to_string_lossy().to_string()),
        thread_source: Some(ThreadSource::User),
        ..ThreadStartParams::default()
    };

    let thread_id = session
        .start(start)
        .await
        .expect("start thread")
        .thread
        .id
        .clone();
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), session.next_event())
        .await
        .expect("thread-start notification timeout")
        .expect("thread-start notification");
    assert!(matches!(
        event,
        AppServerEvent::ServerNotification(ServerNotification::ThreadStarted(params))
            if params.thread.id == thread_id
    ));
    session
        .start_turn(vec![UserInput::Text {
            text: "integration smoke test".to_string(),
            text_elements: Vec::new(),
        }])
        .await
        .expect("start turn");
    let completed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(AppServerEvent::ServerNotification(ServerNotification::TurnCompleted(
                params,
            ))) = session.next_event().await
            {
                break params.turn.status;
            }
        }
    })
    .await
    .expect("turn-completed notification timeout");
    assert_eq!(completed, TurnStatus::Completed);

    let resumed_id = session
        .resume(ThreadResumeParams {
            thread_id: thread_id.clone(),
            ..ThreadResumeParams::default()
        })
        .await
        .expect("resume thread")
        .thread
        .id
        .clone();
    let forked_id = session
        .fork(ThreadForkParams {
            thread_id: thread_id.clone(),
            thread_source: Some(ThreadSource::User),
            ..ThreadForkParams::default()
        })
        .await
        .expect("fork thread")
        .thread
        .id
        .clone();

    assert_eq!(resumed_id, thread_id);
    assert_ne!(forked_id, thread_id);
    session.shutdown().await.expect("shutdown app-server");
}
