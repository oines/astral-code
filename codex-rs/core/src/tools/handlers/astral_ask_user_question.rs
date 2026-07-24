use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::RequestUserInputHandler;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::config_types::ModeKind;
use codex_protocol::request_user_input::RequestUserInputArgs;
use codex_protocol::request_user_input::RequestUserInputQuestion;
use codex_protocol::request_user_input::RequestUserInputQuestionOption;
use codex_tools::ASK_USER_QUESTION_TOOL_NAME;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::astral_core_tool_by_name;
use codex_tools::parse_tool_input_schema_without_compaction;
use serde::Deserialize;

pub struct AstralAskUserQuestionHandler {
    request_user_input: RequestUserInputHandler,
}

impl AstralAskUserQuestionHandler {
    pub(crate) fn new(available_modes: Vec<ModeKind>) -> Self {
        Self {
            request_user_input: RequestUserInputHandler { available_modes },
        }
    }

    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        self.request_user_input
            .handle(to_request_user_input_invocation(invocation)?)
            .await
    }
}

impl ToolExecutor<ToolInvocation> for AstralAskUserQuestionHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(ASK_USER_QUESTION_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        let tool = astral_core_tool_by_name(ASK_USER_QUESTION_TOOL_NAME).unwrap_or_else(|| {
            panic!("astral core tool `{ASK_USER_QUESTION_TOOL_NAME}` should have a schema");
        });
        let parameters = parse_tool_input_schema_without_compaction(&tool.input_schema)
            .unwrap_or_else(|err| {
                panic!(
                    "astral core tool `{ASK_USER_QUESTION_TOOL_NAME}` schema should parse: {err}"
                );
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

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(invocation))
    }
}

impl CoreToolRuntime for AstralAskUserQuestionHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

#[derive(Deserialize)]
struct AskUserQuestionArgs {
    questions: Vec<AskUserQuestion>,
}

#[derive(Deserialize)]
struct AskUserQuestion {
    #[serde(default)]
    id: Option<String>,
    header: String,
    question: String,
    #[serde(default)]
    options: Option<Vec<AskUserQuestionOption>>,
}

#[derive(Deserialize)]
struct AskUserQuestionOption {
    label: String,
    description: String,
}

fn to_request_user_input_invocation(
    mut invocation: ToolInvocation,
) -> Result<ToolInvocation, FunctionCallError> {
    let ToolPayload::Function { arguments } = invocation.payload else {
        return Err(FunctionCallError::RespondToModel(
            "AskUserQuestion handler received unsupported payload".to_string(),
        ));
    };

    invocation.tool_name = ToolName::plain("request_user_input");
    invocation.payload = ToolPayload::Function {
        arguments: rewrite_ask_user_question_arguments(&arguments)?,
    };
    Ok(invocation)
}

fn rewrite_ask_user_question_arguments(arguments: &str) -> Result<String, FunctionCallError> {
    let args: AskUserQuestionArgs = parse_arguments(arguments)?;
    let questions = args
        .questions
        .into_iter()
        .enumerate()
        .map(|(index, question)| RequestUserInputQuestion {
            id: question
                .id
                .unwrap_or_else(|| format!("question_{}", index + 1)),
            header: question.header,
            question: question.question,
            is_other: false,
            is_secret: false,
            options: question.options.map(|options| {
                options
                    .into_iter()
                    .map(|option| RequestUserInputQuestionOption {
                        label: option.label,
                        description: option.description,
                    })
                    .collect()
            }),
        })
        .collect();

    serde_json::to_string(&RequestUserInputArgs { questions }).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to serialize rewritten AskUserQuestion arguments: {err}"
        ))
    })
}
