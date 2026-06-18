use core_test_support::responses;
use serde_json::json;
use std::path::Path;

pub fn create_shell_command_sse_response(
    command: Vec<String>,
    workdir: Option<&Path>,
    timeout_ms: Option<u64>,
    call_id: &str,
) -> anyhow::Result<String> {
    let command_str = shlex::try_join(command.iter().map(String::as_str))?;
    let tool_call_arguments = serde_json::to_string(&json!({
        "command": command_str,
        "cwd": workdir.map(|w| w.to_string_lossy()),
        "timeout": timeout_ms
    }))?;
    Ok(responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_function_call(call_id, "Bash", &tool_call_arguments),
        responses::ev_completed("resp-1"),
    ]))
}

pub fn create_final_assistant_message_sse_response(message: &str) -> anyhow::Result<String> {
    Ok(responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_assistant_message("msg-1", message),
        responses::ev_completed("resp-1"),
    ]))
}

pub fn create_apply_patch_sse_response(
    patch_content: &str,
    call_id: &str,
) -> anyhow::Result<String> {
    let tool_call_arguments = serde_json::to_string(&json!({
        "command": format!("apply_patch <<'EOF'\n{patch_content}\nEOF\n"),
    }))?;
    Ok(responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_function_call(call_id, "Bash", &tool_call_arguments),
        responses::ev_completed("resp-1"),
    ]))
}

pub fn create_exec_command_sse_response(call_id: &str) -> anyhow::Result<String> {
    let (cmd, args) = if cfg!(windows) {
        ("cmd.exe", vec!["/d", "/c", "echo hi"])
    } else {
        ("/bin/sh", vec!["-c", "echo hi"])
    };
    let command = std::iter::once(cmd.to_string())
        .chain(args.into_iter().map(str::to_string))
        .collect::<Vec<_>>();
    let tool_call_arguments = serde_json::to_string(&json!({
        "cmd": command.join(" "),
        "yield_time_ms": 500
    }))?;
    Ok(responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_function_call(call_id, "exec_command", &tool_call_arguments),
        responses::ev_completed("resp-1"),
    ]))
}

pub fn create_request_user_input_sse_response(call_id: &str) -> anyhow::Result<String> {
    let tool_call_arguments = serde_json::to_string(&json!({
        "questions": [{
            "id": "confirm_path",
            "header": "Confirm",
            "question": "Proceed with the plan?",
            "options": [{
                "label": "Yes (Recommended)",
                "description": "Continue the current plan."
            }, {
                "label": "No",
                "description": "Stop and revisit the approach."
            }]
        }]
    }))?;

    Ok(responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_function_call(call_id, "AskUserQuestion", &tool_call_arguments),
        responses::ev_completed("resp-1"),
    ]))
}

pub fn create_request_permissions_sse_response(call_id: &str) -> anyhow::Result<String> {
    let tool_call_arguments = serde_json::to_string(&json!({
        "reason": "Select a workspace root",
        "permissions": {
            "file_system": {
                "write": [
                    ".",
                    "../shared"
                ]
            }
        }
    }))?;

    Ok(responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_function_call(call_id, "RequestPermissions", &tool_call_arguments),
        responses::ev_completed("resp-1"),
    ]))
}
