use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::ExecCommandHandler;
use crate::tools::handlers::ExecCommandHandlerOptions;
use crate::tools::handlers::ShellCommandHandler;
use crate::tools::handlers::ShellCommandHandlerOptions;
use crate::tools::handlers::parse_arguments;
use crate::tools::handlers::rewrite_function_string_argument;
use crate::tools::handlers::updated_hook_command;
use crate::tools::hook_names::HookToolName;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::PostToolUsePayload;
use crate::tools::registry::PreToolUsePayload;
use crate::tools::registry::ToolExecutor;
use codex_tools::BASH_TOOL_NAME;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::astral_core_tool_by_name;
use codex_tools::parse_tool_input_schema_without_compaction;
use serde::Deserialize;
use serde_json::Map;
use serde_json::Value;

const BACKGROUND_BASH_YIELD_TIME_MS: u64 = 250;

pub struct AstralBashHandler {
    backend: AstralBashBackend,
}

enum AstralBashBackend {
    UnifiedExec(ExecCommandHandler),
    ShellCommand(ShellCommandHandler),
}

impl AstralBashHandler {
    pub(crate) fn new(options: ExecCommandHandlerOptions) -> Self {
        Self {
            backend: AstralBashBackend::UnifiedExec(ExecCommandHandler::new(options)),
        }
    }

    pub(crate) fn new_shell_command(options: ShellCommandHandlerOptions) -> Self {
        Self {
            backend: AstralBashBackend::ShellCommand(ShellCommandHandler::new(options)),
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for AstralBashHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(BASH_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        let tool = astral_core_tool_by_name(BASH_TOOL_NAME).unwrap_or_else(|| {
            panic!("astral core tool `{BASH_TOOL_NAME}` should have a schema");
        });
        let parameters = parse_tool_input_schema_without_compaction(&tool.input_schema)
            .unwrap_or_else(|err| {
                panic!("astral core tool `{BASH_TOOL_NAME}` schema should parse: {err}");
            });

        ToolSpec::Function(ResponsesApiTool {
            name: tool.name,
            description: tool.description,
            strict: false,
            defer_loading: None,
            parameters,
            output_schema: None,
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        match &self.backend {
            AstralBashBackend::UnifiedExec(exec) => {
                exec.handle(to_exec_invocation(invocation)?).await
            }
            AstralBashBackend::ShellCommand(shell) => {
                shell.handle(to_shell_command_invocation(invocation)?).await
            }
        }
    }
}

impl CoreToolRuntime for AstralBashHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    fn waits_for_runtime_cancellation(&self) -> bool {
        matches!(&self.backend, AstralBashBackend::ShellCommand(_))
    }

    fn pre_tool_use_payload(&self, invocation: &ToolInvocation) -> Option<PreToolUsePayload> {
        let ToolPayload::Function { arguments } = &invocation.payload else {
            return None;
        };

        parse_arguments::<AstralBashArgs>(arguments)
            .ok()
            .map(|args| PreToolUsePayload {
                tool_name: HookToolName::bash(),
                tool_input: serde_json::json!({ "command": args.command }),
            })
    }

    fn with_updated_hook_input(
        &self,
        mut invocation: ToolInvocation,
        updated_input: Value,
    ) -> Result<ToolInvocation, FunctionCallError> {
        let ToolPayload::Function { arguments } = invocation.payload else {
            return Err(FunctionCallError::RespondToModel(
                "hook input rewrite received unsupported Bash payload".to_string(),
            ));
        };
        invocation.payload = ToolPayload::Function {
            arguments: rewrite_function_string_argument(
                &arguments,
                BASH_TOOL_NAME,
                "command",
                updated_hook_command(&updated_input)?,
            )?,
        };
        Ok(invocation)
    }

    fn post_tool_use_payload(
        &self,
        invocation: &ToolInvocation,
        result: &dyn ToolOutput,
    ) -> Option<PostToolUsePayload> {
        let tool_use_id = result.post_tool_use_id(&invocation.call_id);
        let tool_input = result
            .post_tool_use_input(&invocation.payload)
            .or_else(|| bash_tool_input(&invocation.payload))?;
        Some(PostToolUsePayload {
            tool_name: HookToolName::bash(),
            tool_use_id: tool_use_id.clone(),
            tool_input,
            tool_response: result.post_tool_use_response(&tool_use_id, &invocation.payload)?,
        })
    }
}

#[derive(Deserialize)]
struct AstralBashArgs {
    command: String,
}

fn to_exec_invocation(mut invocation: ToolInvocation) -> Result<ToolInvocation, FunctionCallError> {
    let ToolPayload::Function { arguments } = invocation.payload else {
        return Err(FunctionCallError::RespondToModel(
            "Bash handler received unsupported payload".to_string(),
        ));
    };

    invocation.tool_name = ToolName::plain("exec_command");
    invocation.payload = ToolPayload::Function {
        arguments: rewrite_bash_arguments(&arguments)?,
    };
    Ok(invocation)
}

fn to_shell_command_invocation(
    mut invocation: ToolInvocation,
) -> Result<ToolInvocation, FunctionCallError> {
    let ToolPayload::Function { arguments } = invocation.payload else {
        return Err(FunctionCallError::RespondToModel(
            "Bash handler received unsupported payload".to_string(),
        ));
    };

    invocation.tool_name = ToolName::plain("shell_command");
    invocation.payload = ToolPayload::Function {
        arguments: rewrite_bash_arguments_for_shell_command(&arguments)?,
    };
    Ok(invocation)
}

fn rewrite_bash_arguments(arguments: &str) -> Result<String, FunctionCallError> {
    let value: Value = parse_arguments(arguments)?;
    let Value::Object(mut object) = value else {
        return Err(FunctionCallError::RespondToModel(
            "Bash arguments must be an object".to_string(),
        ));
    };

    move_field_if_absent(&mut object, "command", "cmd");
    move_field_if_absent(&mut object, "cwd", "workdir");
    move_field_if_absent(&mut object, "timeout", "timeout_ms");
    apply_run_in_background_yield(&mut object);

    serde_json::to_string(&object).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to serialize rewritten Bash arguments: {err}"
        ))
    })
}

fn rewrite_bash_arguments_for_shell_command(arguments: &str) -> Result<String, FunctionCallError> {
    let value: Value = parse_arguments(arguments)?;
    let Value::Object(mut object) = value else {
        return Err(FunctionCallError::RespondToModel(
            "Bash arguments must be an object".to_string(),
        ));
    };

    move_field_if_absent(&mut object, "cwd", "workdir");
    move_field_if_absent(&mut object, "timeout", "timeout_ms");
    object.remove("run_in_background");

    serde_json::to_string(&object).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to serialize rewritten Bash arguments: {err}"
        ))
    })
}

fn move_field_if_absent(object: &mut Map<String, Value>, from: &str, to: &str) {
    if object.contains_key(to) {
        return;
    }
    if let Some(value) = object.remove(from) {
        object.insert(to.to_string(), value);
    }
}

fn apply_run_in_background_yield(object: &mut Map<String, Value>) {
    let run_in_background = object
        .remove("run_in_background")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if run_in_background && !object.contains_key("yield_time_ms") {
        object.insert(
            "yield_time_ms".to_string(),
            Value::Number(BACKGROUND_BASH_YIELD_TIME_MS.into()),
        );
    }
}

fn bash_tool_input(payload: &ToolPayload) -> Option<Value> {
    let ToolPayload::Function { arguments } = payload else {
        return None;
    };

    parse_arguments::<AstralBashArgs>(arguments)
        .ok()
        .map(|args| serde_json::json!({ "command": args.command }))
}

#[cfg(test)]
#[path = "astral_bash_tests.rs"]
mod tests;
