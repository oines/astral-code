use super::*;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn canonicalize_function(name: &str, arguments: Value) -> anyhow::Result<(ToolName, Value)> {
    let (tool_name, payload) = canonicalize_astral_tool_call(
        ToolName::plain(name),
        ToolPayload::Function {
            arguments: arguments.to_string(),
        },
    )?;
    let ToolPayload::Function { arguments } = payload else {
        panic!("expected function payload for {name}");
    };
    Ok((tool_name, serde_json::from_str(&arguments)?))
}

#[test]
fn canonicalizes_bash_to_unified_exec() -> anyhow::Result<()> {
    let (tool_name, arguments) = canonicalize_function(
        BASH_TOOL_NAME,
        json!({
            "command": "npm test",
            "cwd": "/workspace/app",
            "timeout": 120000,
            "description": "Run tests"
        }),
    )?;

    assert_eq!(tool_name, ToolName::plain("exec_command"));
    assert_eq!(
        arguments,
        json!({
            "cmd": "npm test",
            "workdir": "/workspace/app",
            "timeout_ms": 120000,
            "description": "Run tests"
        })
    );
    Ok(())
}

#[test]
fn canonicalizes_monitor_to_write_stdin() -> anyhow::Result<()> {
    let (tool_name, arguments) = canonicalize_function(
        MONITOR_TOOL_NAME,
        json!({
            "shell_id": 42,
            "chars": "y\n",
            "yield_time_ms": 30000,
            "max_output_tokens": 2000
        }),
    )?;

    assert_eq!(tool_name, ToolName::plain("write_stdin"));
    assert_eq!(
        arguments,
        json!({
            "session_id": 42,
            "chars": "y\n",
            "yield_time_ms": 30000,
            "max_output_tokens": 2000
        })
    );
    Ok(())
}

#[test]
fn canonicalizes_file_tools_to_unified_exec_commands() -> anyhow::Result<()> {
    let (tool_name, arguments) = canonicalize_function(
        READ_TOOL_NAME,
        json!({ "file_path": "/workspace/src/lib.rs", "offset": 3, "limit": 5 }),
    )?;
    assert_eq!(tool_name, ToolName::plain("exec_command"));
    let cmd = arguments["cmd"].as_str().expect("Read command");
    assert!(cmd.contains("python3"));
    assert!(cmd.contains("/workspace/src/lib.rs"));
    assert!(cmd.contains(" 3 5"));

    let (tool_name, arguments) = canonicalize_function(
        WRITE_TOOL_NAME,
        json!({ "file_path": "/workspace/notes.txt", "content": "hello world\n" }),
    )?;
    assert_eq!(tool_name, ToolName::plain("exec_command"));
    let cmd = arguments["cmd"].as_str().expect("Write command");
    assert!(cmd.contains("python3"));
    assert!(cmd.contains("/workspace/notes.txt"));
    assert!(cmd.contains("hello world"));

    let (tool_name, arguments) = canonicalize_function(
        EDIT_TOOL_NAME,
        json!({
            "file_path": "/workspace/notes.txt",
            "old_string": "hello",
            "new_string": "goodbye",
            "replace_all": true
        }),
    )?;
    assert_eq!(tool_name, ToolName::plain("exec_command"));
    let cmd = arguments["cmd"].as_str().expect("Edit command");
    assert!(cmd.contains("/workspace/notes.txt"));
    assert!(cmd.contains("goodbye"));
    assert!(cmd.ends_with(" true"));
    Ok(())
}

#[test]
fn canonicalizes_search_tools_to_unified_exec_commands() -> anyhow::Result<()> {
    let (tool_name, arguments) = canonicalize_function(
        GLOB_TOOL_NAME,
        json!({ "pattern": "**/*.rs", "path": "/workspace" }),
    )?;
    assert_eq!(tool_name, ToolName::plain("exec_command"));
    let cmd = arguments["cmd"].as_str().expect("Glob command");
    assert!(cmd.contains("python3"));
    assert!(cmd.contains("/workspace"));
    assert!(cmd.contains("**/*.rs"));

    let (tool_name, arguments) = canonicalize_function(
        GREP_TOOL_NAME,
        json!({
            "pattern": "struct .*Args",
            "path": "/workspace",
            "glob": "*.rs",
            "output_mode": "content",
            "-n": true,
            "-C": 2,
            "head_limit": 10
        }),
    )?;
    assert_eq!(tool_name, ToolName::plain("exec_command"));
    let cmd = arguments["cmd"].as_str().expect("Grep command");
    assert!(cmd.contains("python3"));
    assert!(cmd.contains("struct .*Args"));
    assert!(cmd.contains("head_limit"));
    Ok(())
}

#[test]
fn canonicalizes_todo_write_to_update_plan() -> anyhow::Result<()> {
    let (tool_name, arguments) = canonicalize_function(
        TODO_WRITE_TOOL_NAME,
        json!({
            "explanation": "Switch to runtime mapping",
            "todos": [
                { "content": "Add bridge", "status": "in_progress", "activeForm": "Adding bridge" },
                { "content": "Run tests", "status": "pending" }
            ]
        }),
    )?;

    assert_eq!(tool_name, ToolName::plain("update_plan"));
    assert_eq!(
        arguments,
        json!({
            "explanation": "Switch to runtime mapping",
            "plan": [
                { "step": "Add bridge", "status": "in_progress" },
                { "step": "Run tests", "status": "pending" }
            ]
        })
    );
    Ok(())
}

#[test]
fn canonicalizes_ask_user_question_to_request_user_input() -> anyhow::Result<()> {
    let (tool_name, arguments) = canonicalize_function(
        ASK_USER_QUESTION_TOOL_NAME,
        json!({
            "questions": [{
                "header": "Scope",
                "question": "How deep should the rewrite go?",
                "options": [
                    { "label": "Thin", "description": "Keep it narrow" },
                    { "label": "Deep", "description": "Rewrite internals" }
                ],
                "multiSelect": false
            }]
        }),
    )?;

    assert_eq!(tool_name, ToolName::plain("request_user_input"));
    assert_eq!(
        arguments,
        json!({
            "questions": [{
                "id": "question_1",
                "header": "Scope",
                "question": "How deep should the rewrite go?",
                "options": [
                    { "label": "Thin", "description": "Keep it narrow" },
                    { "label": "Deep", "description": "Rewrite internals" }
                ]
            }]
        })
    );
    Ok(())
}

#[test]
fn canonicalizes_request_permissions_from_blocked_input() -> anyhow::Result<()> {
    let (tool_name, arguments) = canonicalize_function(
        REQUEST_PERMISSIONS_TOOL_NAME,
        json!({
            "reason": "Need network for dependency download",
            "tool_name": "Bash",
            "input": {
                "command": "npm install",
                "additional_permissions": {
                    "network": { "enabled": true }
                }
            }
        }),
    )?;

    assert_eq!(tool_name, ToolName::plain("request_permissions"));
    assert_eq!(
        arguments,
        json!({
            "environment_id": null,
            "reason": "Need network for dependency download",
            "permissions": {
                "network": { "enabled": true }
            }
        })
    );
    Ok(())
}

#[test]
fn canonicalizes_tool_search_to_native_payload() -> anyhow::Result<()> {
    let (tool_name, payload) = canonicalize_astral_tool_call(
        ToolName::plain(TOOL_SEARCH_FLAVOR_TOOL_NAME),
        ToolPayload::Function {
            arguments: json!({ "query": "gmail", "limit": 3 }).to_string(),
        },
    )?;

    assert_eq!(tool_name, ToolName::plain(TOOL_SEARCH_TOOL_NAME));
    let ToolPayload::ToolSearch { arguments } = payload else {
        panic!("ToolSearch should become native ToolSearch payload");
    };
    assert_eq!(arguments.query, "gmail");
    assert_eq!(arguments.limit, Some(3));
    Ok(())
}

#[test]
fn canonicalizes_multi_agent_tools() -> anyhow::Result<()> {
    let (tool_name, arguments) = canonicalize_function(
        AGENT_TOOL_NAME,
        json!({
            "description": "audit adapters",
            "prompt": "Inspect provider adapters and report gaps",
            "subagent_type": "reviewer",
            "model": "astral-fast"
        }),
    )?;
    assert_eq!(tool_name, ToolName::plain("spawn_agent"));
    assert_eq!(
        arguments,
        json!({
            "message": "Inspect provider adapters and report gaps",
            "task_name": "audit adapters",
            "agent_type": "reviewer",
            "model": "astral-fast"
        })
    );

    let (tool_name, arguments) = canonicalize_function(
        SEND_MESSAGE_TOOL_NAME,
        json!({
            "to": "agent-1",
            "summary": "new input",
            "message": { "text": "Please include runtime tests" }
        }),
    )?;
    assert_eq!(tool_name, ToolName::plain("send_message"));
    assert_eq!(
        arguments,
        json!({
            "target": "agent-1",
            "message": "{\"text\":\"Please include runtime tests\"}"
        })
    );

    let (tool_name, arguments) =
        canonicalize_function(TASK_STOP_TOOL_NAME, json!({ "task_id": "agent-1" }))?;
    assert_eq!(tool_name, ToolName::plain("interrupt_agent"));
    assert_eq!(arguments, json!({ "target": "agent-1" }));
    Ok(())
}

#[test]
fn canonical_name_leaves_namespaced_tools_alone() {
    let tool_name = ToolName::namespaced("mcp__server__", BASH_TOOL_NAME);
    assert_eq!(canonical_astral_tool_name(&tool_name), tool_name);
}
