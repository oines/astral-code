use crate::exec::is_likely_sandbox_denied;
use crate::guardian::GuardianApprovalRequest;
use crate::guardian::review_approval_request;
use crate::sandboxing::SandboxPermissions;
use crate::session::turn_context::TurnEnvironment;
use crate::tools::handlers::astral_file_tools::AstralFileToolExecutionOutput;
use crate::tools::handlers::astral_file_tools::FileReadStateStore;
use crate::tools::handlers::astral_file_tools::execute_astral_file_tool;
use crate::tools::hook_names::HookToolName;
use crate::tools::sandboxing::Approvable;
use crate::tools::sandboxing::ApprovalCtx;
use crate::tools::sandboxing::ExecApprovalRequirement;
use crate::tools::sandboxing::PermissionRequestPayload;
use crate::tools::sandboxing::SandboxAttempt;
use crate::tools::sandboxing::Sandboxable;
use crate::tools::sandboxing::ToolCtx;
use crate::tools::sandboxing::ToolError;
use crate::tools::sandboxing::ToolRuntime;
use crate::tools::sandboxing::with_cached_approval;
use codex_exec_server::FileSystemSandboxContext;
use codex_protocol::error::CodexErr;
use codex_protocol::error::SandboxErr;
use codex_protocol::exec_output::ExecToolCallOutput;
use codex_protocol::exec_output::StreamOutput;
use codex_protocol::models::AdditionalPermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::ReviewDecision;
use codex_sandboxing::SandboxType;
use codex_sandboxing::SandboxablePreference;
use codex_sandboxing::policy_transforms::effective_permission_profile;
use codex_tools::FunctionCallError;
use codex_utils_absolute_path::AbsolutePathBuf;
use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;

use crate::tools::handlers::AstralFileToolKind;

#[derive(Clone, Debug, Eq, PartialEq, Hash, serde::Serialize)]
pub(crate) struct AstralFileToolApprovalKey {
    environment_id: String,
    command: Vec<String>,
    cwd: AbsolutePathBuf,
    sandbox_permissions: SandboxPermissions,
    additional_permissions: Option<AdditionalPermissionProfile>,
}

#[derive(Debug)]
pub(crate) struct AstralFileToolRequest {
    pub(crate) kind: AstralFileToolKind,
    pub(crate) arguments: String,
    pub(crate) approval_command: Vec<String>,
    pub(crate) hook_input: Value,
    pub(crate) turn_environment: TurnEnvironment,
    pub(crate) cwd: AbsolutePathBuf,
    pub(crate) environment_id: Option<String>,
    pub(crate) read_state: Arc<FileReadStateStore>,
    pub(crate) sandbox_permissions: SandboxPermissions,
    pub(crate) additional_permissions: Option<AdditionalPermissionProfile>,
    pub(crate) permissions_preapproved: bool,
    pub(crate) exec_approval_requirement: ExecApprovalRequirement,
}

#[derive(Default)]
pub(crate) struct AstralFileToolRuntime;

impl AstralFileToolRuntime {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn file_system_sandbox_context_for_attempt(
        req: &AstralFileToolRequest,
        attempt: &SandboxAttempt<'_>,
    ) -> Option<FileSystemSandboxContext> {
        if attempt.sandbox == SandboxType::None {
            return None;
        }

        let permissions =
            effective_permission_profile(attempt.permissions, approved_additional_permissions(req));
        Some(FileSystemSandboxContext {
            permissions,
            cwd: Some(attempt.sandbox_cwd.clone()),
            windows_sandbox_level: attempt.windows_sandbox_level,
            windows_sandbox_private_desktop: attempt.windows_sandbox_private_desktop,
            use_legacy_landlock: attempt.use_legacy_landlock,
        })
    }
}

fn approved_additional_permissions(
    req: &AstralFileToolRequest,
) -> Option<&AdditionalPermissionProfile> {
    if req.permissions_preapproved
        || matches!(
            req.exec_approval_requirement,
            ExecApprovalRequirement::NeedsApproval { .. }
        )
    {
        req.additional_permissions.as_ref()
    } else {
        None
    }
}

impl Sandboxable for AstralFileToolRuntime {
    fn sandbox_preference(&self) -> SandboxablePreference {
        SandboxablePreference::Auto
    }

    fn escalate_on_failure(&self) -> bool {
        true
    }
}

impl Approvable<AstralFileToolRequest> for AstralFileToolRuntime {
    type ApprovalKey = AstralFileToolApprovalKey;

    fn approval_keys(&self, req: &AstralFileToolRequest) -> Vec<Self::ApprovalKey> {
        vec![AstralFileToolApprovalKey {
            environment_id: req.turn_environment.environment_id.clone(),
            command: req.approval_command.clone(),
            cwd: req.cwd.clone(),
            sandbox_permissions: req.sandbox_permissions,
            additional_permissions: req.additional_permissions.clone(),
        }]
    }

    fn start_approval_async<'a>(
        &'a mut self,
        req: &'a AstralFileToolRequest,
        ctx: ApprovalCtx<'a>,
    ) -> BoxFuture<'a, ReviewDecision> {
        let keys = self.approval_keys(req);
        let command = req.approval_command.clone();
        let cwd = req.cwd.clone();
        let retry_reason = ctx.retry_reason.clone();
        let reason = retry_reason.clone();
        let session = ctx.session;
        let turn = ctx.turn;
        let call_id = ctx.call_id.to_string();
        let guardian_review_id = ctx.guardian_review_id.clone();
        Box::pin(async move {
            if let Some(review_id) = guardian_review_id {
                return review_approval_request(
                    session,
                    turn,
                    review_id,
                    GuardianApprovalRequest::Shell {
                        id: call_id,
                        command,
                        cwd: cwd.clone(),
                        sandbox_permissions: req.sandbox_permissions,
                        additional_permissions: req.additional_permissions.clone(),
                        justification: None,
                    },
                    retry_reason,
                )
                .await;
            }
            if req.permissions_preapproved && retry_reason.is_none() {
                return ReviewDecision::Approved;
            }
            with_cached_approval(
                &session.services,
                "astral_file_tools",
                keys,
                move || async move {
                    session
                        .request_command_approval(
                            turn,
                            call_id,
                            /*approval_id*/ None,
                            command,
                            cwd,
                            reason,
                            ctx.network_approval_context.clone(),
                            req.exec_approval_requirement
                                .proposed_execpolicy_amendment()
                                .cloned(),
                            req.additional_permissions.clone(),
                            /*available_decisions*/ None,
                        )
                        .await
                },
            )
            .await
        })
    }

    fn wants_no_sandbox_approval(&self, policy: AskForApproval) -> bool {
        match policy {
            AskForApproval::Never => false,
            AskForApproval::Granular(granular_config) => granular_config.allows_sandbox_approval(),
            AskForApproval::OnFailure => true,
            AskForApproval::OnRequest => true,
            AskForApproval::UnlessTrusted => true,
        }
    }

    fn exec_approval_requirement(
        &self,
        req: &AstralFileToolRequest,
    ) -> Option<ExecApprovalRequirement> {
        Some(req.exec_approval_requirement.clone())
    }

    fn permission_request_payload(
        &self,
        req: &AstralFileToolRequest,
    ) -> Option<PermissionRequestPayload> {
        Some(PermissionRequestPayload {
            tool_name: HookToolName::new(req.kind.name()),
            tool_input: req.hook_input.clone(),
        })
    }

    fn sandbox_permissions(&self, req: &AstralFileToolRequest) -> SandboxPermissions {
        req.sandbox_permissions
    }
}

impl ToolRuntime<AstralFileToolRequest, Result<AstralFileToolExecutionOutput, FunctionCallError>>
    for AstralFileToolRuntime
{
    fn sandbox_cwd<'a>(&self, req: &'a AstralFileToolRequest) -> Option<&'a AbsolutePathBuf> {
        Some(&req.cwd)
    }

    async fn run(
        &mut self,
        req: &AstralFileToolRequest,
        attempt: &SandboxAttempt<'_>,
        ctx: &ToolCtx,
    ) -> Result<Result<AstralFileToolExecutionOutput, FunctionCallError>, ToolError> {
        let started_at = Instant::now();
        let sandbox = Self::file_system_sandbox_context_for_attempt(req, attempt);
        let result = execute_astral_file_tool(req, sandbox.as_ref(), ctx).await;
        if let Err(error) = &result {
            let output = output_for_error(error, started_at);
            if is_likely_sandbox_denied(attempt.sandbox, &output) {
                return Err(ToolError::Codex(CodexErr::Sandbox(SandboxErr::Denied {
                    output: Box::new(output),
                    network_policy_decision: None,
                })));
            }
        }
        Ok(result)
    }
}

fn output_for_error(error: &FunctionCallError, started_at: Instant) -> ExecToolCallOutput {
    let message = error.to_string();
    ExecToolCallOutput {
        exit_code: 1,
        stdout: StreamOutput::new(String::new()),
        stderr: StreamOutput::new(message.clone()),
        aggregated_output: StreamOutput::new(message),
        duration: started_at.elapsed(),
        timed_out: false,
    }
}

#[cfg(test)]
#[path = "astral_file_tools_tests.rs"]
mod tests;
