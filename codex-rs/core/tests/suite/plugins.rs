#![cfg(not(target_os = "windows"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use anyhow::Result;
use codex_login::CodexAuth;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::stdio_server_bin;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use core_test_support::wait_for_mcp_server;
use std::sync::Arc;
use tempfile::TempDir;
use wiremock::MockServer;

const SAMPLE_PLUGIN_CONFIG_NAME: &str = "sample@test";
const SAMPLE_PLUGIN_DISPLAY_NAME: &str = "sample";
const SAMPLE_PLUGIN_DESCRIPTION: &str = "inspect sample data";

fn sample_plugin_root(home: &TempDir) -> std::path::PathBuf {
    home.path().join("plugins/cache/test/sample/local")
}

fn write_sample_plugin_manifest_and_config(home: &TempDir) -> std::path::PathBuf {
    let plugin_root = sample_plugin_root(home);
    std::fs::create_dir_all(plugin_root.join(".codex-plugin")).expect("create plugin manifest dir");
    std::fs::write(
        plugin_root.join(".codex-plugin/plugin.json"),
        format!(
            r#"{{"name":"{SAMPLE_PLUGIN_DISPLAY_NAME}","description":"{SAMPLE_PLUGIN_DESCRIPTION}"}}"#
        ),
    )
    .expect("write plugin manifest");
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            "[features]\nplugins = true\n\n[plugins.\"{SAMPLE_PLUGIN_CONFIG_NAME}\"]\nenabled = true\n"
        ),
    )
    .expect("write config");
    plugin_root
}

fn write_plugin_skill_plugin(home: &TempDir) -> std::path::PathBuf {
    let plugin_root = write_sample_plugin_manifest_and_config(home);
    let skill_dir = plugin_root.join("skills/sample-search");
    std::fs::create_dir_all(skill_dir.as_path()).expect("create plugin skill dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\ndescription: inspect sample data\n---\n\n# body\n",
    )
    .expect("write plugin skill");
    skill_dir.join("SKILL.md")
}

fn write_plugin_mcp_plugin(home: &TempDir, command: &str) {
    let plugin_root = write_sample_plugin_manifest_and_config(home);
    std::fs::write(
        plugin_root.join(".mcp.json"),
        format!(
            r#"{{
  "mcpServers": {{
    "sample": {{
      "command": "{command}",
      "cwd": ".",
      "startup_timeout_sec": 60.0
    }}
  }}
}}"#
        ),
    )
    .expect("write plugin mcp config");
}

async fn build_plugin_test_codex(
    server: &MockServer,
    codex_home: Arc<TempDir>,
) -> Result<TestCodex> {
    let mut builder = test_codex()
        .with_home(codex_home)
        .with_auth(CodexAuth::create_dummy_api_key_auth_for_testing());
    Ok(builder
        .build(server)
        .await
        .expect("create new conversation"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capability_sections_render_in_developer_message_in_order() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;

    let resp_mock = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp1"), ev_completed("resp1")]),
    )
    .await;

    let codex_home = Arc::new(TempDir::new()?);
    write_plugin_skill_plugin(codex_home.as_ref());
    let test_codex = build_plugin_test_codex(&server, Arc::clone(&codex_home)).await?;
    let codex = Arc::clone(&test_codex.codex);

    codex
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "hello".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let request = resp_mock.single_request();
    let developer_messages = request.message_input_texts("developer");
    let developer_text = developer_messages.join("\n\n");
    let skills_pos = developer_text
        .find("## Skills")
        .expect("expected skills section in developer message");
    let plugins_pos = developer_text
        .find("## Plugins")
        .expect("expected plugins section in developer message");
    assert!(
        skills_pos < plugins_pos,
        "expected Skills -> Plugins order: {developer_messages:?}"
    );
    assert!(
        developer_text.contains("`sample`"),
        "expected enabled plugin name in developer message: {developer_messages:?}"
    );
    assert!(
        developer_text.contains("`sample`: inspect sample data"),
        "expected plugin description in developer message: {developer_messages:?}"
    );
    assert!(
        developer_text.contains("skill entries are prefixed with `plugin_name:`"),
        "expected plugin skill naming guidance in developer message: {developer_messages:?}"
    );
    assert!(
        developer_text.contains("sample:sample-search: inspect sample data"),
        "expected namespaced plugin skill summary in developer message: {developer_messages:?}"
    );

    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_plugin_mentions_inject_plugin_guidance() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    let mock = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;

    let codex_home = Arc::new(TempDir::new()?);
    let rmcp_test_server_bin = match stdio_server_bin() {
        Ok(bin) => bin,
        Err(err) => {
            eprintln!("test_stdio_server binary not available, skipping test: {err}");
            return Ok(());
        }
    };
    write_plugin_skill_plugin(codex_home.as_ref());
    write_plugin_mcp_plugin(codex_home.as_ref(), &rmcp_test_server_bin);

    let test_codex = build_plugin_test_codex(&server, codex_home).await?;
    let codex = Arc::clone(&test_codex.codex);
    wait_for_mcp_server(&codex, "sample").await?;

    codex
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Mention {
                name: "sample".into(),
                path: format!("plugin://{SAMPLE_PLUGIN_CONFIG_NAME}"),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let request = mock.single_request();
    let developer_messages = request.message_input_texts("developer");
    assert!(
        developer_messages
            .iter()
            .any(|text| text.contains("Skills from this plugin")),
        "expected plugin skills guidance: {developer_messages:?}"
    );
    assert!(
        developer_messages
            .iter()
            .any(|text| text.contains("MCP servers from this plugin")),
        "expected visible plugin MCP guidance: {developer_messages:?}"
    );
    Ok(())
}
