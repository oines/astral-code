use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use codex_api::AgentClient;
use codex_api::AgentOptions;
use codex_api::ApiError;
use codex_api::AuthError;
use codex_api::AuthProvider;
use codex_api::Provider;
use codex_api::agent_adapters::chat_completions::ChatCompletionsOptions;
use codex_api::agent_protocol::AgentMessage;
use codex_api::agent_protocol::AgentRequest;
use codex_api::agent_protocol::ContentBlock;
use codex_api::agent_protocol::MessageRole;
use codex_client::HttpTransport;
use codex_client::Request;
use codex_client::RequestBody;
use codex_client::Response;
use codex_client::StreamResponse;
use codex_client::TransportError;
use http::HeaderMap;
use http::HeaderValue;
use http::StatusCode;
use pretty_assertions::assert_eq;

fn assert_path_ends_with(requests: &[Request], suffix: &str) {
    assert_eq!(requests.len(), 1);
    let url = &requests[0].url;
    assert!(
        url.ends_with(suffix),
        "expected url to end with {suffix}, got {url}"
    );
}

#[derive(Debug, Default, Clone)]
struct RecordingState {
    stream_requests: Arc<Mutex<Vec<Request>>>,
}

impl RecordingState {
    fn record(&self, req: Request) {
        let mut guard = self
            .stream_requests
            .lock()
            .unwrap_or_else(|err| panic!("mutex poisoned: {err}"));
        guard.push(req);
    }

    fn take_stream_requests(&self) -> Vec<Request> {
        let mut guard = self
            .stream_requests
            .lock()
            .unwrap_or_else(|err| panic!("mutex poisoned: {err}"));
        std::mem::take(&mut *guard)
    }
}

#[derive(Clone)]
struct RecordingTransport {
    state: RecordingState,
}

impl RecordingTransport {
    fn new(state: RecordingState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl HttpTransport for RecordingTransport {
    async fn execute(&self, _req: Request) -> Result<Response, TransportError> {
        Err(TransportError::Build("execute should not run".to_string()))
    }

    async fn stream(&self, req: Request) -> Result<StreamResponse, TransportError> {
        self.state.record(req);

        let stream = futures::stream::iter(Vec::<Result<Bytes, TransportError>>::new());
        Ok(StreamResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            bytes: Box::pin(stream),
        })
    }
}

#[derive(Clone, Default)]
struct NoAuth;

impl AuthProvider for NoAuth {
    fn add_auth_headers(&self, _headers: &mut HeaderMap) {}
}

#[derive(Clone)]
struct StaticAuth {
    token: String,
}

impl StaticAuth {
    fn new(token: &str) -> Self {
        Self {
            token: token.to_string(),
        }
    }
}

impl AuthProvider for StaticAuth {
    fn add_auth_headers(&self, headers: &mut HeaderMap) {
        let token = &self.token;
        if let Ok(header) = HeaderValue::from_str(&format!("Bearer {token}")) {
            headers.insert(http::header::AUTHORIZATION, header);
        }
    }
}

fn provider(name: &str) -> Provider {
    Provider {
        name: name.to_string(),
        base_url: "https://example.com/v1".to_string(),
        query_params: None,
        headers: HeaderMap::new(),
        retry: codex_api::RetryConfig {
            max_attempts: 1,
            base_delay: Duration::from_millis(1),
            retry_429: false,
            retry_5xx: false,
            retry_transport: true,
        },
        stream_idle_timeout: Duration::from_millis(10),
    }
}

#[derive(Clone)]
struct FlakyTransport {
    state: Arc<Mutex<i64>>,
}

impl Default for FlakyTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl FlakyTransport {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(0)),
        }
    }

    fn attempts(&self) -> i64 {
        *self
            .state
            .lock()
            .unwrap_or_else(|err| panic!("mutex poisoned: {err}"))
    }
}

#[derive(Clone)]
struct FailsOnceAuth {
    attempts: Arc<Mutex<i64>>,
    error: Arc<AuthError>,
}

impl FailsOnceAuth {
    fn transient() -> Self {
        Self {
            attempts: Arc::new(Mutex::new(0)),
            error: Arc::new(AuthError::Transient(
                "sts temporarily unavailable".to_string(),
            )),
        }
    }

    fn build() -> Self {
        Self {
            attempts: Arc::new(Mutex::new(0)),
            error: Arc::new(AuthError::Build("invalid auth configuration".to_string())),
        }
    }

    fn attempts(&self) -> i64 {
        *self
            .attempts
            .lock()
            .unwrap_or_else(|err| panic!("mutex poisoned: {err}"))
    }
}

#[async_trait]
impl AuthProvider for FailsOnceAuth {
    fn add_auth_headers(&self, _headers: &mut HeaderMap) {}

    async fn apply_auth(&self, request: Request) -> Result<Request, AuthError> {
        let mut attempts = self
            .attempts
            .lock()
            .unwrap_or_else(|err| panic!("mutex poisoned: {err}"));
        *attempts += 1;

        if *attempts == 1 {
            return match self.error.as_ref() {
                AuthError::Build(message) => Err(AuthError::Build(message.clone())),
                AuthError::Transient(message) => Err(AuthError::Transient(message.clone())),
            };
        }

        Ok(request)
    }
}

#[async_trait]
impl HttpTransport for FlakyTransport {
    async fn execute(&self, _req: Request) -> Result<Response, TransportError> {
        Err(TransportError::Build("execute should not run".to_string()))
    }

    async fn stream(&self, _req: Request) -> Result<StreamResponse, TransportError> {
        let mut attempts = self
            .state
            .lock()
            .unwrap_or_else(|err| panic!("mutex poisoned: {err}"));
        *attempts += 1;

        if *attempts == 1 {
            return Err(TransportError::Network("first attempt fails".to_string()));
        }

        let stream = futures::stream::iter(vec![Ok(Bytes::from(
            r#"data: {"id":"chatcmpl-1","model":"gpt-test","choices":[{"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"chatcmpl-1","choices":[{"delta":{"content":"hi"},"finish_reason":null}]}

data: {"id":"chatcmpl-1","choices":[{"delta":{},"finish_reason":"stop"}]}

"#,
        ))]);

        Ok(StreamResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            bytes: Box::pin(stream),
        })
    }
}

fn agent_request() -> AgentRequest {
    AgentRequest {
        model: "gpt-test".to_string(),
        messages: vec![AgentMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: "hi".to_string(),
            }],
            id: None,
        }],
        ..Default::default()
    }
}

async fn stream_chat_completions<T: HttpTransport>(
    client: &AgentClient<T>,
    options: AgentOptions,
) -> std::result::Result<codex_api::ResponseStream, ApiError> {
    client
        .stream_chat_completions(agent_request(), ChatCompletionsOptions::default(), options)
        .await
}

#[tokio::test]
async fn agent_client_uses_chat_completions_path() -> Result<()> {
    let state = RecordingState::default();
    let transport = RecordingTransport::new(state.clone());
    let client = AgentClient::new(transport, provider("openai"), Arc::new(NoAuth));

    let _stream = stream_chat_completions(&client, AgentOptions::default()).await?;

    let requests = state.take_stream_requests();
    assert_path_ends_with(&requests, "/chat/completions");
    Ok(())
}

#[tokio::test]
async fn streaming_agent_client_adds_auth_headers() -> Result<()> {
    let state = RecordingState::default();
    let transport = RecordingTransport::new(state.clone());
    let auth = Arc::new(StaticAuth::new("secret-token"));
    let client = AgentClient::new(transport, provider("openai"), auth);

    let _stream = stream_chat_completions(&client, AgentOptions::default()).await?;

    let requests = state.take_stream_requests();
    assert_eq!(requests.len(), 1);
    let req = &requests[0];

    let auth_header = req.headers.get(http::header::AUTHORIZATION);
    assert!(auth_header.is_some(), "missing auth header");
    assert_eq!(
        auth_header.unwrap().to_str().ok(),
        Some("Bearer secret-token")
    );

    let accept_header = req.headers.get(http::header::ACCEPT);
    assert!(accept_header.is_some(), "missing Accept header");
    assert_eq!(
        accept_header.unwrap().to_str().ok(),
        Some("text/event-stream")
    );
    Ok(())
}

#[tokio::test]
async fn streaming_agent_client_retries_on_transport_error() -> Result<()> {
    let transport = FlakyTransport::new();

    let mut provider = provider("openai");
    provider.retry.max_attempts = 2;

    let client = AgentClient::new(transport.clone(), provider, Arc::new(NoAuth));

    let _stream = stream_chat_completions(&client, AgentOptions::default()).await?;
    assert_eq!(transport.attempts(), 2);
    Ok(())
}

#[tokio::test]
async fn streaming_agent_client_retries_on_transient_auth_error() -> Result<()> {
    let state = RecordingState::default();
    let transport = RecordingTransport::new(state.clone());
    let auth = FailsOnceAuth::transient();

    let mut provider = provider("openai");
    provider.retry.max_attempts = 2;

    let client = AgentClient::new(transport, provider, Arc::new(auth.clone()));
    let _stream = stream_chat_completions(&client, AgentOptions::default()).await?;

    assert_eq!(auth.attempts(), 2);
    assert_eq!(state.take_stream_requests().len(), 1);
    Ok(())
}

#[tokio::test]
async fn streaming_agent_client_does_not_retry_auth_build_error() -> Result<()> {
    let state = RecordingState::default();
    let transport = RecordingTransport::new(state.clone());
    let auth = FailsOnceAuth::build();

    let mut provider = provider("openai");
    provider.retry.max_attempts = 2;

    let client = AgentClient::new(transport, provider, Arc::new(auth.clone()));
    let result = stream_chat_completions(&client, AgentOptions::default()).await;
    let err = match result {
        Ok(_) => panic!("auth build errors should fail without retry"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        ApiError::Transport(TransportError::Build(message))
            if message == "invalid auth configuration"
    ));
    assert_eq!(auth.attempts(), 1);
    assert_eq!(state.take_stream_requests().len(), 0);
    Ok(())
}

#[tokio::test]
async fn streaming_agent_client_attaches_extra_headers_and_chat_body() -> Result<()> {
    let state = RecordingState::default();
    let transport = RecordingTransport::new(state.clone());
    let client = AgentClient::new(transport, provider("openai"), Arc::new(NoAuth));

    let mut extra_headers = HeaderMap::new();
    extra_headers.insert("x-test-header", HeaderValue::from_static("present"));
    let _stream = stream_chat_completions(&client, AgentOptions { extra_headers }).await?;

    let requests = state.take_stream_requests();
    assert_eq!(requests.len(), 1);
    let req = &requests[0];

    assert_eq!(
        req.headers
            .get("x-test-header")
            .and_then(|v| v.to_str().ok()),
        Some("present")
    );

    let body = req
        .body
        .as_ref()
        .and_then(RequestBody::json)
        .expect("chat completions body should be JSON");
    assert_eq!(
        body.get("model").and_then(|model| model.as_str()),
        Some("gpt-test")
    );
    assert_eq!(
        body.get("stream").and_then(|stream| stream.as_bool()),
        Some(true)
    );
    assert_eq!(
        body.get("messages")
            .and_then(|messages| messages.as_array())
            .and_then(|messages| messages.first())
            .and_then(|message| message.get("role"))
            .and_then(|role| role.as_str()),
        Some("user")
    );

    Ok(())
}
