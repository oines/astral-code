#![cfg(not(target_os = "windows"))]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use codex_login::default_client::ASTRAL_INTERNAL_ORIGINATOR_OVERRIDE_ENV_VAR;
use core_test_support::responses;
use core_test_support::test_codex_exec::test_codex_exec;
use wiremock::matchers::header;

/// Verify that when the server reports an error, `codex-exec` exits with a
/// non-zero status code so automation can detect failures.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_astral_exec_originator() -> anyhow::Result<()> {
    let test = test_codex_exec();

    let server = responses::start_mock_server().await;
    responses::mount_chat_completions_sse_once_match(
        &server,
        header("Originator", "astral_exec"),
        responses::chat_completions_text_sse("Hello, world!"),
    )
    .await;

    test.cmd_with_server(&server)
        .env_remove(ASTRAL_INTERNAL_ORIGINATOR_OVERRIDE_ENV_VAR)
        .arg("--skip-git-repo-check")
        .arg("tell me something")
        .assert()
        .code(0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn supports_originator_override() -> anyhow::Result<()> {
    let test = test_codex_exec();

    let server = responses::start_mock_server().await;
    responses::mount_chat_completions_sse_once_match(
        &server,
        header("Originator", "astral_exec_override"),
        responses::chat_completions_text_sse("Hello, world!"),
    )
    .await;

    test.cmd_with_server(&server)
        .env(
            ASTRAL_INTERNAL_ORIGINATOR_OVERRIDE_ENV_VAR,
            "astral_exec_override",
        )
        .arg("--skip-git-repo-check")
        .arg("tell me something")
        .assert()
        .code(0);

    Ok(())
}
