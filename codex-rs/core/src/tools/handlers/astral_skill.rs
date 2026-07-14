use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_core_skills::HostLoadedSkills;
use codex_core_skills::SkillLoadOutcome;
use codex_core_skills::SkillMetadata;
use codex_tools::ResponsesApiTool;
use codex_tools::SKILL_TOOL_NAME;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::astral_core_tool_by_name;
use codex_tools::parse_tool_input_schema_without_compaction;
use serde::Deserialize;
use std::sync::Arc;

const MAX_SKILL_OUTPUT_CHARS: usize = 24_000;

pub struct AstralSkillHandler;

impl AstralSkillHandler {
    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let ToolInvocation { turn, payload, .. } = invocation;
        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "Skill handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: AstralSkillArgs = parse_arguments(&arguments)?;
        let skill_name = normalize_skill_name(&args.skill);
        if skill_name.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "Skill requires a non-empty `skill` name".to_string(),
            ));
        }

        let outcome = Arc::clone(&turn.turn_skills.outcome);
        let skill = resolve_skill(outcome.as_ref(), skill_name)?;
        let host_skills = HostLoadedSkills::new(Arc::clone(&outcome));
        let contents = host_skills.read_skill_text(skill).await.map_err(|err| {
            FunctionCallError::RespondToModel(format!(
                "unable to read skill `{}` from `{}`: {err}",
                skill.name,
                skill.path_to_skills_md.display()
            ))
        })?;

        let text = format_skill_output(skill, args.args.as_deref(), &contents);
        Ok(boxed_tool_output(FunctionToolOutput::from_text(
            truncate_skill_output(text),
            Some(true),
        )))
    }
}

impl ToolExecutor<ToolInvocation> for AstralSkillHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(SKILL_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        let tool = astral_core_tool_by_name(SKILL_TOOL_NAME).unwrap_or_else(|| {
            panic!("astral core tool `{SKILL_TOOL_NAME}` should have a schema");
        });
        let parameters = parse_tool_input_schema_without_compaction(&tool.input_schema)
            .unwrap_or_else(|err| {
                panic!("astral core tool `{SKILL_TOOL_NAME}` schema should parse: {err}");
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

impl CoreToolRuntime for AstralSkillHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

#[derive(Deserialize)]
struct AstralSkillArgs {
    skill: String,
    #[serde(default)]
    args: Option<String>,
}

fn resolve_skill<'a>(
    outcome: &'a SkillLoadOutcome,
    skill_name: &str,
) -> Result<&'a SkillMetadata, FunctionCallError> {
    let matches = outcome
        .skills
        .iter()
        .filter(|skill| outcome.is_skill_enabled(skill))
        .filter(|skill| normalize_skill_name(&skill.name) == skill_name)
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] => Err(FunctionCallError::RespondToModel(format!(
            "Unknown skill `{skill_name}`. Available skills: {}",
            available_skill_names(outcome)
        ))),
        [skill] => Ok(*skill),
        skills => Err(FunctionCallError::RespondToModel(format!(
            "Skill `{skill_name}` is ambiguous: {}",
            skills
                .iter()
                .map(|skill| skill.path_to_skills_md.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn normalize_skill_name(name: &str) -> &str {
    let trimmed = name.trim();
    trimmed.strip_prefix('/').unwrap_or(trimmed)
}

fn available_skill_names(outcome: &SkillLoadOutcome) -> String {
    let mut names = outcome
        .skills
        .iter()
        .filter(|skill| outcome.is_skill_enabled(skill))
        .map(|skill| skill.name.as_str())
        .take(20)
        .collect::<Vec<_>>();
    if names.is_empty() {
        return "none".to_string();
    }

    let has_more = outcome
        .skills
        .iter()
        .filter(|skill| outcome.is_skill_enabled(skill))
        .nth(20)
        .is_some();
    if has_more {
        names.push("...");
    }
    names.join(", ")
}

fn format_skill_output(skill: &SkillMetadata, args: Option<&str>, contents: &str) -> String {
    let mut output = format!(
        "<skill>\n<name>{}</name>\n<path>{}</path>\n",
        skill.name,
        skill.path_to_skills_md.display()
    );
    if let Some(args) = args.filter(|args| !args.trim().is_empty()) {
        output.push_str("<args>\n");
        output.push_str(args);
        output.push_str("\n</args>\n");
    }
    output.push_str(contents);
    if !contents.ends_with('\n') {
        output.push('\n');
    }
    output.push_str("</skill>");
    output
}

fn truncate_skill_output(text: String) -> String {
    if text.chars().count() <= MAX_SKILL_OUTPUT_CHARS {
        return text;
    }

    let mut truncated = text
        .chars()
        .take(MAX_SKILL_OUTPUT_CHARS)
        .collect::<String>();
    truncated
        .push_str("\n[Skill output truncated; use Read on the SKILL.md path for the full file]\n");
    truncated
}
