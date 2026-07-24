#![allow(clippy::expect_used, clippy::unwrap_used, unused_imports)]

use anyhow::Context;
use assert_cmd::prelude::*;
use codex_apply_patch::CODEX_CORE_APPLY_PATCH_ARG1;
use core_test_support::responses::chat_completions_apply_patch_tool_call_sse;
use core_test_support::responses::chat_completions_completed_sse;
use core_test_support::responses::mount_chat_completions_sse_sequence;
use core_test_support::responses::mount_models_once;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex_exec::exec_test_model_catalog;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

/// While we may add an `apply-patch` subcommand to the `codex` CLI multitool
/// at some point, we must ensure that the smaller `codex-exec` CLI can still
/// emulate the `apply_patch` CLI.
#[test]
fn test_standalone_exec_cli_can_use_apply_patch() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let relative_path = "source.txt";
    let absolute_path = tmp.path().join(relative_path);
    fs::write(&absolute_path, "original content\n")?;

    Command::new(codex_utils_cargo_bin::cargo_bin("codex-exec")?)
        .arg(CODEX_CORE_APPLY_PATCH_ARG1)
        .arg(
            r#"*** Begin Patch
*** Update File: source.txt
@@
-original content
+modified by apply_patch
*** End Patch"#,
        )
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout("Success. Updated the following files:\nM source.txt\n")
        .stderr(predicates::str::is_empty());
    assert_eq!(
        fs::read_to_string(absolute_path)?,
        "modified by apply_patch\n"
    );
    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_apply_patch_tool() -> anyhow::Result<()> {
    use core_test_support::skip_if_no_network;
    use core_test_support::test_codex_exec::test_codex_exec;

    skip_if_no_network!(Ok(()));

    let test = test_codex_exec();
    let tmp_path = test.cwd_path().to_path_buf();
    let add_patch = r#"*** Begin Patch
*** Add File: test.md
+Hello world
*** End Patch"#;
    let update_patch = r#"*** Begin Patch
*** Update File: test.md
@@
-Hello world
+Final text
*** End Patch"#;
    let response_streams = vec![
        chat_completions_apply_patch_tool_call_sse("request_0", add_patch),
        chat_completions_apply_patch_tool_call_sse("request_1", update_patch),
        chat_completions_completed_sse(),
    ];
    let server = start_mock_server().await;
    mount_models_once(&server, exec_test_model_catalog()).await;
    let response_mock = mount_chat_completions_sse_sequence(&server, response_streams).await;

    test.cmd_with_server(&server)
        .arg("--skip-git-repo-check")
        .arg("--dangerously-bypass-approvals-and-sandbox")
        .arg("-c")
        .arg("tools.surface=\"codex\"")
        .arg("foo")
        .assert()
        .success();

    let requests = response_mock.requests();
    assert_eq!(
        requests.len(),
        3,
        "expected apply_patch turn to continue after tool calls, got requests: {:#?}",
        requests
            .iter()
            .map(core_test_support::responses::ResponsesRequest::body_json)
            .collect::<Vec<_>>()
    );

    let final_path = tmp_path.join("test.md");
    let contents = std::fs::read_to_string(&final_path)
        .unwrap_or_else(|e| panic!("failed reading {}: {e}", final_path.display()));
    assert_eq!(contents, "Final text\n");
    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_apply_patch_freeform_tool() -> anyhow::Result<()> {
    use core_test_support::skip_if_no_network;
    use core_test_support::test_codex_exec::test_codex_exec;

    skip_if_no_network!(Ok(()));

    let test = test_codex_exec();
    let freeform_add_patch = r#"*** Begin Patch
*** Add File: app.py
+class BaseClass:
+  def method():
+    return False
*** End Patch"#;
    let freeform_update_patch = r#"*** Begin Patch
*** Update File: app.py
@@  def method():
-    return False
+
+    return True
*** End Patch"#;
    let response_streams = vec![
        chat_completions_apply_patch_tool_call_sse("request_0", freeform_add_patch),
        chat_completions_apply_patch_tool_call_sse("request_1", freeform_update_patch),
        chat_completions_completed_sse(),
    ];
    let server = start_mock_server().await;
    mount_models_once(&server, exec_test_model_catalog()).await;
    let response_mock = mount_chat_completions_sse_sequence(&server, response_streams).await;

    test.cmd_with_server(&server)
        .arg("--skip-git-repo-check")
        .arg("--dangerously-bypass-approvals-and-sandbox")
        .arg("-c")
        .arg("tools.surface=\"codex\"")
        .arg("foo")
        .assert()
        .success();

    let requests = response_mock.requests();
    assert_eq!(
        requests.len(),
        3,
        "expected apply_patch turn to continue after tool calls, got requests: {:#?}",
        requests
            .iter()
            .map(core_test_support::responses::ResponsesRequest::body_json)
            .collect::<Vec<_>>()
    );

    // Verify final file contents
    let final_path = test.cwd_path().join("app.py");
    let contents = std::fs::read_to_string(&final_path)
        .unwrap_or_else(|e| panic!("failed reading {}: {e}", final_path.display()));
    assert_eq!(
        contents,
        include_str!("../fixtures/apply_patch_freeform_final.txt")
    );
    Ok(())
}
