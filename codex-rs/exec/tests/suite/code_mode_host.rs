#![allow(clippy::expect_used)]

use assert_cmd::prelude::*;
use codex_login::ASTRAL_API_KEY_ENV_VAR;
use codex_model_provider_info::ASTRAL_BASE_URL_ENV_VAR;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex_exec::TEST_MODEL;
use core_test_support::test_codex_exec::exec_test_model_catalog;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::process::Command;

const CALL_ID: &str = "packaged-code-mode-call";
const OUTPUT_MARKER: &str = "packaged-host-marker";
const EXEC_SOURCE: &str = "text(['packaged', 'host', 'marker'].join('-'));";

fn chat_completions_exec_sse() -> String {
    responses::chat_completions_sse(vec![json!({
        "id": "chatcmpl-packaged-host",
        "model": TEST_MODEL,
        "choices": [{
            "delta": {
                "role": "assistant",
                "tool_calls": [{
                    "index": 0,
                    "id": CALL_ID,
                    "type": "function",
                    "function": {
                        "name": "exec",
                        "arguments": serde_json::to_string(&json!({
                            "input": EXEC_SOURCE,
                        }))
                        .expect("serialize exec arguments"),
                    },
                }],
            },
            "finish_reason": "tool_calls",
        }],
    })])
}

fn packaged_command(
    entrypoint: &Path,
    cwd: &Path,
    home: &Path,
    model_catalog: &Path,
    server_uri: &str,
) -> anyhow::Result<Command> {
    let mut command = Command::new(entrypoint);
    command
        .current_dir(cwd)
        .env("ASTRAL_HOME", home)
        .env("ASTRAL_SQLITE_HOME", home)
        .env(ASTRAL_API_KEY_ENV_VAR, "dummy")
        .env(ASTRAL_BASE_URL_ENV_VAR, format!("{server_uri}/v1"))
        .env_remove("CODEX_CODE_MODE_HOST_PATH")
        .arg("--skip-git-repo-check")
        .arg("--dangerously-bypass-approvals-and-sandbox")
        .arg("-c")
        .arg(format!("model={}", serde_json::to_string(TEST_MODEL)?))
        .arg("-c")
        .arg(format!(
            "model_catalog_json={}",
            serde_json::to_string(&model_catalog.display().to_string())?
        ))
        .arg("-c")
        .arg("features.code_mode=true")
        .arg("-c")
        .arg("features.code_mode_host=true")
        .arg("run the packaged code mode host");
    Ok(command)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn packaged_sibling_code_mode_host_is_discovered_and_executes() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let package = tempfile::tempdir()?;
    let bin_dir = package.path().join("bin");
    fs::create_dir(&bin_dir)?;
    let entrypoint = bin_dir.join(format!("astral{}", std::env::consts::EXE_SUFFIX));
    let host = bin_dir.join(format!(
        "codex-code-mode-host{}",
        std::env::consts::EXE_SUFFIX
    ));
    fs::copy(codex_utils_cargo_bin::cargo_bin("codex-exec")?, &entrypoint)?;

    let home = tempfile::tempdir()?;
    let cwd = tempfile::tempdir()?;
    let model_catalog = home.path().join("models.json");
    fs::write(
        &model_catalog,
        serde_json::to_vec(&exec_test_model_catalog())?,
    )?;

    // A present but invalid sibling must produce an explicit host error. If sibling discovery
    // regresses to a different missing path, the provider would silently fall back in-process and
    // this phase would incorrectly emit OUTPUT_MARKER.
    fs::write(&host, b"not a code mode host")?;
    let invalid_host_server = responses::start_mock_server().await;
    let invalid_host_mock = responses::mount_chat_completions_sse_sequence(
        &invalid_host_server,
        vec![
            chat_completions_exec_sse(),
            responses::chat_completions_text_sse("done"),
        ],
    )
    .await;
    packaged_command(
        &entrypoint,
        cwd.path(),
        home.path(),
        &model_catalog,
        &invalid_host_server.uri(),
    )?
    .assert()
    .success();
    let invalid_host_requests = invalid_host_mock.requests();
    assert_eq!(invalid_host_requests.len(), 2);
    assert!(invalid_host_requests[1].body_contains_text("failed to spawn code-mode host"));
    assert!(!invalid_host_requests[1].body_contains_text(OUTPUT_MARKER));

    fs::copy(
        codex_utils_cargo_bin::cargo_bin("codex-code-mode-host")?,
        &host,
    )?;
    let server = responses::start_mock_server().await;
    let response_mock = responses::mount_chat_completions_sse_sequence(
        &server,
        vec![
            chat_completions_exec_sse(),
            responses::chat_completions_text_sse("done"),
        ],
    )
    .await;

    packaged_command(
        &entrypoint,
        cwd.path(),
        home.path(),
        &model_catalog,
        &server.uri(),
    )?
    .assert()
    .success();

    let requests = response_mock.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].body_contains_text(OUTPUT_MARKER));

    Ok(())
}
