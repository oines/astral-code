use crate::agent_adapters::anthropic::AnthropicMessagesOptions;
use crate::agent_adapters::anthropic::to_messages_request;
use crate::agent_adapters::chat_completions::ChatCompletionsOptions;
use crate::agent_adapters::chat_completions::to_chat_completions_request;
use crate::auth::SharedAuthProvider;
use crate::common::ResponseStream;
use crate::endpoint::session::EndpointSession;
use crate::error::ApiError;
use crate::provider::Provider;
use crate::sse::agent::AgentStreamFormat;
use crate::sse::agent::spawn_agent_stream;
use crate::telemetry::SseTelemetry;
use codex_agent_protocol::AgentRequest;
use codex_client::HttpTransport;
use codex_client::RequestTelemetry;
use http::HeaderMap;
use http::HeaderName;
use http::HeaderValue;
use http::Method;
use std::sync::Arc;
use tracing::instrument;

const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct AgentClient<T: HttpTransport> {
    session: EndpointSession<T>,
    sse_telemetry: Option<Arc<dyn SseTelemetry>>,
}

#[derive(Default)]
pub struct AgentOptions {
    pub extra_headers: HeaderMap,
}

impl<T: HttpTransport> AgentClient<T> {
    pub fn new(transport: T, provider: Provider, auth: SharedAuthProvider) -> Self {
        Self {
            session: EndpointSession::new(transport, provider, auth),
            sse_telemetry: None,
        }
    }

    pub fn with_telemetry(
        self,
        request: Option<Arc<dyn RequestTelemetry>>,
        sse: Option<Arc<dyn SseTelemetry>>,
    ) -> Self {
        Self {
            session: self.session.with_request_telemetry(request),
            sse_telemetry: sse,
        }
    }

    #[instrument(
        name = "agent.stream_anthropic_messages",
        level = "info",
        skip_all,
        fields(
            transport = "agent_http",
            http.method = "POST",
            api.path = "messages"
        )
    )]
    pub async fn stream_anthropic_messages(
        &self,
        request: AgentRequest,
        messages_options: AnthropicMessagesOptions,
        options: AgentOptions,
    ) -> Result<ResponseStream, ApiError> {
        let body = to_messages_request(&request, messages_options);
        let mut headers = options.extra_headers;
        let version_header = HeaderName::from_static("anthropic-version");
        if !headers.contains_key(&version_header) {
            headers.insert(version_header, HeaderValue::from_static(ANTHROPIC_VERSION));
        }

        let stream_response = self
            .session
            .stream_with(Method::POST, "messages", headers, Some(body), |req| {
                req.headers.insert(
                    http::header::ACCEPT,
                    HeaderValue::from_static("text/event-stream"),
                );
            })
            .await?;

        Ok(spawn_agent_stream(
            stream_response,
            self.session.provider().stream_idle_timeout,
            self.sse_telemetry.clone(),
            AgentStreamFormat::AnthropicMessages,
        ))
    }

    #[instrument(
        name = "agent.stream_chat_completions",
        level = "info",
        skip_all,
        fields(
            transport = "agent_http",
            http.method = "POST",
            api.path = "chat/completions"
        )
    )]
    pub async fn stream_chat_completions(
        &self,
        request: AgentRequest,
        chat_options: ChatCompletionsOptions,
        options: AgentOptions,
    ) -> Result<ResponseStream, ApiError> {
        let body = to_chat_completions_request(&request, chat_options);
        let stream_response = self
            .session
            .stream_with(
                Method::POST,
                "chat/completions",
                options.extra_headers,
                Some(body),
                |req| {
                    req.headers.insert(
                        http::header::ACCEPT,
                        HeaderValue::from_static("text/event-stream"),
                    );
                },
            )
            .await?;

        Ok(spawn_agent_stream(
            stream_response,
            self.session.provider().stream_idle_timeout,
            self.sse_telemetry.clone(),
            AgentStreamFormat::ChatCompletions,
        ))
    }
}
