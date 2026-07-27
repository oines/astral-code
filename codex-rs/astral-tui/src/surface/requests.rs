use super::SurfaceState;
use crate::PendingRequest;
use crate::request_user_input::RequestUserInputState;

impl SurfaceState {
    pub(crate) fn sync_request_user_input(&mut self) {
        let params = self
            .pending_requests
            .front()
            .and_then(|request| match request {
                PendingRequest::UserInput { params, .. } => Some(params.clone()),
                _ => None,
            });
        if let Some(params) = params {
            self.request_user_input.sync(&params);
        }
    }

    pub(crate) fn request_user_input(&self) -> &RequestUserInputState {
        &self.request_user_input
    }

    pub(crate) fn request_user_input_mut(&mut self) -> &mut RequestUserInputState {
        &mut self.request_user_input
    }

    pub(crate) fn reset_request_user_input(&mut self) {
        self.request_user_input.reset();
    }
}
