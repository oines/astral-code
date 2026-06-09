use super::*;

const FEEDBACK_UPLOAD_DISABLED_MESSAGE: &str = "feedback upload is disabled in astral-code";

#[derive(Clone)]
pub(crate) struct FeedbackRequestProcessor;

impl FeedbackRequestProcessor {
    pub(crate) fn new(
        _auth_manager: Arc<AuthManager>,
        _thread_manager: Arc<ThreadManager>,
        _config: Arc<Config>,
        _feedback: CodexFeedback,
        _log_db: Option<LogDbLayer>,
        _state_db: Option<StateDbHandle>,
    ) -> Self {
        Self
    }

    pub(crate) async fn feedback_upload(
        &self,
        _params: FeedbackUploadParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        Err(invalid_request(FEEDBACK_UPLOAD_DISABLED_MESSAGE))
    }
}
