use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::RequestId;

use super::SurfaceState;
use crate::PendingRequest;
use crate::mcp_form::McpFormState;
use crate::request_user_input::RequestUserInputState;

impl SurfaceState {
    pub(crate) fn sync_request_states(&mut self) {
        match self.pending_requests.front().cloned() {
            Some(PendingRequest::UserInput { params, .. }) => {
                self.request_user_input.sync(&params);
            }
            Some(PendingRequest::McpElicitation { params, .. }) => {
                if let McpServerElicitationRequest::Form {
                    requested_schema, ..
                } = params.request
                {
                    self.mcp_form.sync(&requested_schema);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn request_user_input(&self) -> &RequestUserInputState {
        &self.request_user_input
    }

    pub(crate) fn request_user_input_mut(&mut self) -> &mut RequestUserInputState {
        &mut self.request_user_input
    }

    pub(crate) fn mcp_form(&self) -> &McpFormState {
        &self.mcp_form
    }

    pub(crate) fn mcp_form_mut(&mut self) -> &mut McpFormState {
        &mut self.mcp_form
    }

    pub(crate) fn remove_pending_request(
        &mut self,
        request_id: &RequestId,
    ) -> Option<PendingRequest> {
        let request = self.pending_requests.remove_resolved(request_id)?;
        match &request {
            PendingRequest::UserInput { .. } => self.request_user_input.reset(),
            PendingRequest::McpElicitation { params, .. }
                if matches!(params.request, McpServerElicitationRequest::Form { .. }) =>
            {
                self.mcp_form.reset();
            }
            _ => {}
        }
        Some(request)
    }
}
