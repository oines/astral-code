#![allow(clippy::expect_used)]

use anyhow::Result;
use codex_features::Feature;
use core_test_support::responses;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_custom_tool_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn packaged_code_mode_host_completes_handshake_and_executes() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let host_program = codex_utils_cargo_bin::cargo_bin("codex-code-mode-host")?;
    let mut builder = test_codex()
        .with_model("test-gpt-5.1-codex")
        .with_code_mode_host_program(host_program)
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodeMode)
                .expect("enable code mode");
        });
    let test = builder.build(&server).await?;
    responses::mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_custom_tool_call("call-1", "exec", "text('packaged-host-marker');"),
            ev_completed("resp-1"),
        ]),
    )
    .await;
    let follow_up = responses::mount_sse_once(
        &server,
        sse(vec![
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-2"),
        ]),
    )
    .await;

    test.submit_turn("run the packaged code mode host").await?;

    let output = follow_up.single_request().custom_tool_call_output("call-1");
    assert!(
        output.to_string().contains("packaged-host-marker"),
        "the packaged host should complete its handshake and execute the cell: {output}"
    );

    Ok(())
}
