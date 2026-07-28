use std::collections::HashMap;
use std::collections::VecDeque;

use codex_app_server_protocol::AttestationGenerateParams;
use codex_app_server_protocol::AttestationGenerateResponse;
use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::CommandExecutionRequestApprovalResponse;
use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::DynamicToolCallResponse;
use codex_app_server_protocol::FileChangeApprovalDecision;
use codex_app_server_protocol::FileChangeRequestApprovalParams;
use codex_app_server_protocol::FileChangeRequestApprovalResponse;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::McpServerElicitationRequestParams;
use codex_app_server_protocol::McpServerElicitationRequestResponse;
use codex_app_server_protocol::PermissionsRequestApprovalParams;
use codex_app_server_protocol::PermissionsRequestApprovalResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::ToolRequestUserInputResponse;
use serde_json::Value;

/// A server-initiated request waiting for Astral TUI or a client tool handler.
///
/// Variants keep the app-server v2 payload intact. The TUI may project these
/// into different visual controls without weakening approval decisions or
/// teaching the runtime about surface-specific tool names.
#[derive(Debug, Clone, PartialEq)]
pub enum PendingRequest {
    CommandExecution {
        request_id: RequestId,
        params: CommandExecutionRequestApprovalParams,
    },
    FileChange {
        request_id: RequestId,
        params: FileChangeRequestApprovalParams,
    },
    Permissions {
        request_id: RequestId,
        params: PermissionsRequestApprovalParams,
    },
    UserInput {
        request_id: RequestId,
        params: ToolRequestUserInputParams,
    },
    McpElicitation {
        request_id: RequestId,
        params: McpServerElicitationRequestParams,
    },
    DynamicTool {
        request_id: RequestId,
        params: DynamicToolCallParams,
    },
    Attestation {
        request_id: RequestId,
        params: AttestationGenerateParams,
    },
    LegacyApplyPatch {
        request_id: RequestId,
    },
    LegacyExecCommand {
        request_id: RequestId,
    },
}

impl PendingRequest {
    pub fn request_id(&self) -> &RequestId {
        match self {
            Self::CommandExecution { request_id, .. }
            | Self::FileChange { request_id, .. }
            | Self::Permissions { request_id, .. }
            | Self::UserInput { request_id, .. }
            | Self::McpElicitation { request_id, .. }
            | Self::DynamicTool { request_id, .. }
            | Self::Attestation { request_id, .. }
            | Self::LegacyApplyPatch { request_id }
            | Self::LegacyExecCommand { request_id } => request_id,
        }
    }

    pub fn thread_id(&self) -> Option<&str> {
        match self {
            Self::CommandExecution { params, .. } => Some(&params.thread_id),
            Self::FileChange { params, .. } => Some(&params.thread_id),
            Self::Permissions { params, .. } => Some(&params.thread_id),
            Self::UserInput { params, .. } => Some(&params.thread_id),
            Self::McpElicitation { params, .. } => Some(&params.thread_id),
            Self::DynamicTool { params, .. } => Some(&params.thread_id),
            Self::Attestation { .. }
            | Self::LegacyApplyPatch { .. }
            | Self::LegacyExecCommand { .. } => None,
        }
    }

    fn response_kind(&self) -> &'static str {
        match self {
            Self::CommandExecution { .. } => "command execution approval",
            Self::FileChange { .. } => "file change approval",
            Self::Permissions { .. } => "permissions approval",
            Self::UserInput { .. } => "user input",
            Self::McpElicitation { .. } => "MCP elicitation",
            Self::DynamicTool { .. } => "dynamic tool call",
            Self::Attestation { .. } => "attestation",
            Self::LegacyApplyPatch { .. } => "legacy apply-patch approval",
            Self::LegacyExecCommand { .. } => "legacy exec approval",
        }
    }
}

impl From<ServerRequest> for PendingRequest {
    fn from(request: ServerRequest) -> Self {
        match request {
            ServerRequest::CommandExecutionRequestApproval { request_id, params } => {
                Self::CommandExecution { request_id, params }
            }
            ServerRequest::FileChangeRequestApproval { request_id, params } => {
                Self::FileChange { request_id, params }
            }
            ServerRequest::PermissionsRequestApproval { request_id, params } => {
                Self::Permissions { request_id, params }
            }
            ServerRequest::ToolRequestUserInput { request_id, params } => {
                Self::UserInput { request_id, params }
            }
            ServerRequest::McpServerElicitationRequest { request_id, params } => {
                Self::McpElicitation { request_id, params }
            }
            ServerRequest::DynamicToolCall { request_id, params } => {
                Self::DynamicTool { request_id, params }
            }
            ServerRequest::AttestationGenerate { request_id, params } => {
                Self::Attestation { request_id, params }
            }
            ServerRequest::ApplyPatchApproval { request_id, .. } => {
                Self::LegacyApplyPatch { request_id }
            }
            ServerRequest::ExecCommandApproval { request_id, .. } => {
                Self::LegacyExecCommand { request_id }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PendingRequestResponse {
    CommandExecution(CommandExecutionApprovalDecision),
    FileChange(FileChangeApprovalDecision),
    Permissions(PermissionsRequestApprovalResponse),
    UserInput(ToolRequestUserInputResponse),
    McpElicitation(McpServerElicitationRequestResponse),
    DynamicTool(DynamicToolCallResponse),
    Attestation { token: String },
    Reject { code: i64, message: String },
}

impl PendingRequestResponse {
    fn kind(&self) -> &'static str {
        match self {
            Self::CommandExecution(_) => "command execution approval",
            Self::FileChange(_) => "file change approval",
            Self::Permissions(_) => "permissions approval",
            Self::UserInput(_) => "user input",
            Self::McpElicitation(_) => "MCP elicitation",
            Self::DynamicTool(_) => "dynamic tool call",
            Self::Attestation { .. } => "attestation",
            Self::Reject { .. } => "rejection",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RequestResolution {
    Success {
        request_id: RequestId,
        result: Value,
    },
    Reject {
        request_id: RequestId,
        error: JSONRPCErrorError,
    },
}

impl RequestResolution {
    pub fn request_id(&self) -> &RequestId {
        match self {
            Self::Success { request_id, .. } | Self::Reject { request_id, .. } => request_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingRequestError {
    NotFound(RequestId),
    WrongResponse {
        expected: &'static str,
        received: &'static str,
    },
    Serialize(String),
}

impl std::fmt::Display for PendingRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(request_id) => write!(f, "request {request_id} is not pending"),
            Self::WrongResponse { expected, received } => {
                write!(f, "expected {expected} response, received {received}")
            }
            Self::Serialize(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PendingRequestError {}

/// Ordered pending-request state shared by event handling and interactive UI.
#[derive(Debug, Default)]
pub struct PendingRequests {
    order: VecDeque<RequestId>,
    requests: HashMap<RequestId, PendingRequest>,
}

impl PendingRequests {
    pub fn note(&mut self, request: ServerRequest) {
        let request = PendingRequest::from(request);
        let request_id = request.request_id().clone();
        if !self.requests.contains_key(&request_id) {
            self.order.push_back(request_id.clone());
        }
        self.requests.insert(request_id, request);
    }

    pub fn front(&self) -> Option<&PendingRequest> {
        self.order
            .front()
            .and_then(|request_id| self.requests.get(request_id))
    }

    pub fn get(&self, request_id: &RequestId) -> Option<&PendingRequest> {
        self.requests.get(request_id)
    }

    pub fn len(&self) -> usize {
        self.requests.len()
    }

    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    pub fn remove_resolved(&mut self, request_id: &RequestId) -> Option<PendingRequest> {
        let request = self.requests.remove(request_id)?;
        self.order.retain(|pending_id| pending_id != request_id);
        Some(request)
    }

    /// Builds the typed response without removing the pending request.
    ///
    /// The caller must keep the request until the response reaches app-server,
    /// otherwise a transport failure would make the interaction impossible to
    /// retry.
    pub fn prepare_resolution(
        &self,
        request_id: &RequestId,
        response: PendingRequestResponse,
    ) -> Result<RequestResolution, PendingRequestError> {
        let request = self
            .requests
            .get(request_id)
            .ok_or_else(|| PendingRequestError::NotFound(request_id.clone()))?;

        let result = match (request, response) {
            (_, PendingRequestResponse::Reject { code, message }) => {
                return Ok(RequestResolution::Reject {
                    request_id: request_id.clone(),
                    error: JSONRPCErrorError {
                        code,
                        message,
                        data: None,
                    },
                });
            }
            (
                PendingRequest::CommandExecution { .. },
                PendingRequestResponse::CommandExecution(decision),
            ) => serde_json::to_value(CommandExecutionRequestApprovalResponse { decision }),
            (PendingRequest::FileChange { .. }, PendingRequestResponse::FileChange(decision)) => {
                serde_json::to_value(FileChangeRequestApprovalResponse { decision })
            }
            (PendingRequest::Permissions { .. }, PendingRequestResponse::Permissions(response)) => {
                serde_json::to_value(response)
            }
            (PendingRequest::UserInput { .. }, PendingRequestResponse::UserInput(response)) => {
                serde_json::to_value(response)
            }
            (
                PendingRequest::McpElicitation { .. },
                PendingRequestResponse::McpElicitation(response),
            ) => serde_json::to_value(response),
            (PendingRequest::DynamicTool { .. }, PendingRequestResponse::DynamicTool(response)) => {
                serde_json::to_value(response)
            }
            (PendingRequest::Attestation { .. }, PendingRequestResponse::Attestation { token }) => {
                serde_json::to_value(AttestationGenerateResponse { token })
            }
            (request, response) => {
                return Err(PendingRequestError::WrongResponse {
                    expected: request.response_kind(),
                    received: response.kind(),
                });
            }
        }
        .map_err(|error| PendingRequestError::Serialize(error.to_string()))?;

        Ok(RequestResolution::Success {
            request_id: request_id.clone(),
            result,
        })
    }
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
