//! Session- and turn-scoped helpers for talking to model provider APIs.
//!
//! `ModelClient` is intended to live for the lifetime of a Codex session and holds stable
//! session state such as the conversation id. Provider/auth setup is resolved from the request
//! provider so per-turn model changes can cross provider boundaries.
//!
//! Per-turn settings (model selection, reasoning controls, telemetry context, and turn metadata)
//! are passed explicitly to streaming and unary methods so that the turn lifetime is visible at the
//! call site.
//!
//! A [`ModelClientSession`] is created per turn and is used to stream one or more provider
//! requests during that turn.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use codex_api::AgentClient as ApiAgentClient;
use codex_api::AgentOptions as ApiAgentOptions;
use codex_api::ApiError;
use codex_api::AuthProvider;
use codex_api::MemorySummarizeOutput as ApiMemorySummarizeOutput;
use codex_api::Provider as ApiProvider;
use codex_api::RawMemory as ApiRawMemory;
use codex_api::RealtimeCallClient as ApiRealtimeCallClient;
use codex_api::RealtimeSessionConfig as ApiRealtimeSessionConfig;
use codex_api::RequestTelemetry;
use codex_api::ReqwestTransport;
use codex_api::ResponsesClient as ApiResponsesClient;
use codex_api::ResponsesOptions as ApiResponsesOptions;
use codex_api::SharedAuthProvider;
use codex_api::SseTelemetry;
use codex_api::TransportError;
use codex_api::agent_adapters::anthropic::AnthropicMessagesOptions;
use codex_api::agent_adapters::chat_completions::ChatCompletionsOptions;
use codex_api::auth_header_telemetry;
use codex_api::build_session_headers;
use codex_app_server_protocol::AuthMode;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_login::RefreshTokenError;
use codex_login::UnauthorizedRecovery;
use codex_login::default_client::build_reqwest_client;
use codex_otel::SessionTelemetry;

use codex_protocol::SessionId;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;
use codex_protocol::config_types::Verbosity as VerbosityConfig;
use codex_protocol::models::TranscriptItem;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use codex_protocol::protocol::SessionSource;
use codex_rollout_trace::InferenceTraceAttempt;
use codex_rollout_trace::InferenceTraceContext;
use eventsource_stream::Event;
use eventsource_stream::EventStreamError;
use futures::StreamExt;
use http::HeaderMap as ApiHeaderMap;
use http::HeaderValue;
use http::StatusCode as HttpStatusCode;
use reqwest::StatusCode;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::instrument;
use tracing::warn;

use crate::agent_request::AgentRequestBuildParams;
use crate::agent_request::build_agent_request;
use crate::anthropic_cache_fold::AnthropicCacheFoldState;
use crate::attestation::AttestationContext;
use crate::attestation::AttestationProvider;
use crate::attestation::X_OAI_ATTESTATION_HEADER;
use crate::client_common::ModelStreamEvent;
use crate::client_common::Prompt;
use crate::client_common::ResponseStream;
use crate::feedback_tags;
use crate::provider_adapters;
use crate::responses_request::ResponsesRequestParams;
use crate::util::emit_feedback_auth_recovery_tags;
use codex_api::map_api_error;
use codex_feedback::FeedbackRequestTags;
use codex_feedback::emit_feedback_request_tags_with_auth_env;
use codex_login::auth_env_telemetry::AuthEnvTelemetry;
use codex_login::auth_env_telemetry::collect_auth_env_telemetry;
use codex_model_provider::SharedModelProvider;
use codex_model_provider::create_model_provider;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_response_debug_context::extract_response_debug_context;
use codex_response_debug_context::extract_response_debug_context_from_api_error;
use codex_response_debug_context::telemetry_transport_error_message;

pub const X_ASTRAL_INSTALLATION_ID_HEADER: &str = "x-astral-installation-id";
pub const X_ASTRAL_TURN_METADATA_HEADER: &str = "x-astral-turn-metadata";
pub const X_ASTRAL_PARENT_THREAD_ID_HEADER: &str = "x-astral-parent-thread-id";
pub const X_ASTRAL_WINDOW_ID_HEADER: &str = "x-astral-window-id";
const ANTHROPIC_MESSAGES_ENDPOINT: &str = "/messages";
const CHAT_COMPLETIONS_ENDPOINT: &str = "/chat/completions";
const RESPONSES_ENDPOINT: &str = "/responses";
const DEFAULT_ANTHROPIC_MAX_TOKENS: u64 = 4096;

fn anthropic_max_tokens(model_info: &ModelInfo) -> u64 {
    model_info
        .max_output_tokens
        .and_then(|max_output_tokens| u64::try_from(max_output_tokens).ok())
        .filter(|max_output_tokens| *max_output_tokens > 0)
        .unwrap_or(DEFAULT_ANTHROPIC_MAX_TOKENS)
}

/// Session-scoped state shared by all [`ModelClient`] clones.
///
/// This is intentionally kept minimal so `ModelClient` does not need to hold a full `Config`. Most
/// configuration is per turn and is passed explicitly to streaming/unary methods.
#[derive(Debug)]
struct ModelClientState {
    session_id: SessionId,
    thread_id: ThreadId,
    window_generation: AtomicU64,
    installation_id: String,
    provider: SharedModelProvider,
    parent_thread_id: Option<ThreadId>,
    beta_features_header: Option<String>,
    attestation_provider: Option<Arc<dyn AttestationProvider>>,
    anthropic_cache_fold: Mutex<AnthropicCacheFoldState>,
}

/// Resolved API client setup for a single request attempt.
///
/// Keeping this as a single bundle ensures normal request paths share the same
/// auth/provider setup flow.
struct CurrentClientSetup {
    auth: Option<CodexAuth>,
    api_provider: ApiProvider,
    api_auth: SharedAuthProvider,
    auth_env_telemetry: AuthEnvTelemetry,
}

#[derive(Clone, Copy)]
struct RequestRouteTelemetry {
    endpoint: &'static str,
}

impl RequestRouteTelemetry {
    fn for_endpoint(endpoint: &'static str) -> Self {
        Self { endpoint }
    }
}

/// A session-scoped client for model-provider API calls.
///
/// This holds configuration and state that should be shared across turns within a Codex session
/// (thread id and default realtime provider).
///
/// Turn-scoped settings (model selection, reasoning controls, telemetry context, and turn
/// metadata) are passed explicitly to the relevant methods to keep turn lifetime visible at the
/// call site.
#[derive(Debug, Clone)]
pub struct ModelClient {
    state: Arc<ModelClientState>,
    prompt_cache_key_override: Option<String>,
}

/// A turn-scoped streaming session created from a [`ModelClient`].
///
/// Create a fresh `ModelClientSession` for each Codex turn so per-turn settings and retry state do
/// not leak across requests.
pub struct ModelClientSession {
    client: ModelClient,
}

/// Result of opening a WebRTC Realtime call.
///
/// The SDP answer goes back to the client. The call id and auth headers stay on the server so the
/// ordinary Realtime WebSocket machinery can join the same in-progress call as a sideband
/// controller.
pub(crate) struct RealtimeWebrtcCallStart {
    pub(crate) sdp: String,
    pub(crate) call_id: String,
    pub(crate) sideband_headers: ApiHeaderMap,
}

/// Reuses the API-auth material that created the WebRTC call for the sideband WebSocket join.
///
/// API-key sessions send the same API bearer on the sideband path.
fn sideband_websocket_auth_headers(api_auth: &dyn AuthProvider) -> ApiHeaderMap {
    let mut headers = ApiHeaderMap::new();
    api_auth.add_auth_headers(&mut headers);
    headers
}

impl ModelClient {
    #[allow(clippy::too_many_arguments)]
    /// Creates a new session-scoped `ModelClient`.
    ///
    /// All arguments are expected to be stable for the lifetime of a Codex session. Per-turn values
    /// are passed to [`ModelClientSession::stream`] (and other turn-scoped methods) explicitly.
    pub fn new(
        auth_manager: Option<Arc<AuthManager>>,
        session_id: SessionId,
        thread_id: ThreadId,
        installation_id: String,
        provider_info: ModelProviderInfo,
        session_source: SessionSource,
        parent_thread_id: Option<ThreadId>,
        model_verbosity: Option<VerbosityConfig>,
        enable_request_compression: bool,
        include_timing_metrics: bool,
        beta_features_header: Option<String>,
        attestation_provider: Option<Arc<dyn AttestationProvider>>,
    ) -> Self {
        let _ = enable_request_compression;
        let _ = include_timing_metrics;
        let _ = session_source;
        let _ = model_verbosity;
        let model_provider = create_model_provider(provider_info, auth_manager);
        Self {
            state: Arc::new(ModelClientState {
                session_id,
                thread_id,
                window_generation: AtomicU64::new(0),
                installation_id,
                provider: model_provider,
                parent_thread_id,
                beta_features_header,
                attestation_provider,
                anthropic_cache_fold: Mutex::new(AnthropicCacheFoldState::default()),
            }),
            prompt_cache_key_override: None,
        }
    }

    pub(crate) fn with_prompt_cache_key_override(
        mut self,
        prompt_cache_key_override: Option<String>,
    ) -> Self {
        self.prompt_cache_key_override = prompt_cache_key_override;
        self
    }

    fn prompt_cache_key(&self) -> String {
        self.prompt_cache_key_override
            .clone()
            .unwrap_or_else(|| self.state.thread_id.to_string())
    }

    async fn anthropic_cache_fold_options(
        &self,
        request: &codex_api::agent_protocol::AgentRequest,
    ) -> Option<codex_api::agent_adapters::anthropic::AnthropicCacheFoldOptions> {
        self.state
            .anthropic_cache_fold
            .lock()
            .await
            .options_for_request(request)
    }

    async fn disable_anthropic_cache_fold(&self) {
        self.state.anthropic_cache_fold.lock().await.disable();
    }

    pub(crate) async fn reset_anthropic_cache_fold(&self) {
        self.state.anthropic_cache_fold.lock().await.reset();
    }

    /// Creates a fresh turn-scoped streaming session.
    ///
    /// This constructor does not perform network I/O itself.
    pub fn new_session(&self) -> ModelClientSession {
        ModelClientSession {
            client: self.clone(),
        }
    }

    /// Returns the provider captured when this session-scoped client was created.
    ///
    /// Normal turn execution should pass the current [`TurnContext`](crate::TurnContext) provider
    /// instead. This accessor exists for auxiliary request paths that construct a standalone
    /// `ModelClient` from an already-effective config.
    pub fn default_provider(&self) -> SharedModelProvider {
        Arc::clone(&self.state.provider)
    }

    pub(crate) fn auth_manager(&self) -> Option<Arc<AuthManager>> {
        self.state.provider.auth_manager()
    }

    pub(crate) fn set_window_generation(&self, window_generation: u64) {
        self.state
            .window_generation
            .store(window_generation, Ordering::Relaxed);
    }

    pub(crate) fn advance_window_generation(&self) {
        self.state.window_generation.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn current_window_id(&self) -> String {
        let thread_id = self.state.thread_id;
        let window_generation = self.state.window_generation.load(Ordering::Relaxed);
        format!("{thread_id}:{window_generation}")
    }

    pub(crate) async fn create_realtime_call_with_headers(
        &self,
        sdp: String,
        session_config: ApiRealtimeSessionConfig,
        mut extra_headers: ApiHeaderMap,
    ) -> Result<RealtimeWebrtcCallStart> {
        // Create the media call over HTTP first, then retain matching auth so realtime can attach
        // the server-side control WebSocket to the call id from that HTTP response.
        let client_setup = self.current_client_setup(&self.state.provider).await?;
        if let Some(header_value) = self
            .generate_attestation_header_for(&self.state.provider)
            .await
        {
            extra_headers.insert(X_OAI_ATTESTATION_HEADER, header_value);
        }
        let mut sideband_headers = extra_headers.clone();
        sideband_headers.extend(sideband_websocket_auth_headers(
            client_setup.api_auth.as_ref(),
        ));
        let transport = ReqwestTransport::new(build_reqwest_client());
        let response =
            ApiRealtimeCallClient::new(transport, client_setup.api_provider, client_setup.api_auth)
                .create_with_session_and_headers(sdp, session_config, extra_headers)
                .await
                .map_err(map_api_error)?;
        Ok(RealtimeWebrtcCallStart {
            sdp: response.sdp,
            call_id: response.call_id,
            sideband_headers,
        })
    }

    /// Builds memory summaries for each provided normalized raw memory.
    ///
    /// Astral does not call OpenAI's `/memories/trace_summarize` sideband endpoint; callers receive
    /// no remote memory summaries until a provider-neutral memory summarizer exists.
    pub async fn summarize_memories(
        &self,
        raw_memories: Vec<ApiRawMemory>,
        _model_info: &ModelInfo,
        _effort: Option<ReasoningEffortConfig>,
        _session_telemetry: &SessionTelemetry,
    ) -> Result<Vec<ApiMemorySummarizeOutput>> {
        let _ = raw_memories;
        Ok(Vec::new())
    }

    fn build_session_context_headers(&self) -> ApiHeaderMap {
        let mut extra_headers = ApiHeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&self.state.installation_id) {
            extra_headers.insert(X_ASTRAL_INSTALLATION_ID_HEADER, val);
        }
        if let Some(parent_thread_id) = parent_thread_id_header_value(self.state.parent_thread_id)
            && let Ok(val) = HeaderValue::from_str(&parent_thread_id)
        {
            extra_headers.insert(X_ASTRAL_PARENT_THREAD_ID_HEADER, val);
        }
        if let Ok(val) = HeaderValue::from_str(&self.current_window_id()) {
            extra_headers.insert(X_ASTRAL_WINDOW_ID_HEADER, val);
        }
        extra_headers
    }

    async fn build_agent_headers(
        &self,
        provider: &SharedModelProvider,
        turn_metadata_header: Option<&str>,
    ) -> ApiHeaderMap {
        let session_id = self.state.session_id.to_string();
        let thread_id = self.state.thread_id.to_string();
        let mut headers = ApiHeaderMap::new();
        if let Some(value) = self.state.beta_features_header.as_deref()
            && !value.is_empty()
            && let Ok(header_value) = HeaderValue::from_str(value)
        {
            headers.insert("x-astral-beta-features", header_value);
        }
        if let Some(turn_metadata_header) = parse_turn_metadata_header(turn_metadata_header) {
            headers.insert(X_ASTRAL_TURN_METADATA_HEADER, turn_metadata_header);
        }
        if let Ok(header_value) = HeaderValue::from_str(&thread_id) {
            headers.insert("x-client-request-id", header_value);
        }
        headers.extend(build_session_headers(Some(session_id), Some(thread_id)));
        headers.extend(self.build_session_context_headers());
        if let Some(header_value) = self.generate_attestation_header_for(provider).await {
            headers.insert(X_OAI_ATTESTATION_HEADER, header_value);
        }
        headers
    }

    async fn generate_attestation_header_for(
        &self,
        provider: &SharedModelProvider,
    ) -> Option<HeaderValue> {
        if !provider.supports_attestation() {
            return None;
        }

        self.state
            .attestation_provider
            .as_ref()?
            .header_for_request(AttestationContext {
                thread_id: self.state.thread_id,
            })
            .await
    }

    fn auth_env_telemetry_for_provider(provider: &SharedModelProvider) -> AuthEnvTelemetry {
        let astral_api_key_env_enabled = provider
            .auth_manager()
            .as_ref()
            .is_some_and(|manager| manager.astral_api_key_env_enabled());
        collect_auth_env_telemetry(provider.info(), astral_api_key_env_enabled)
    }

    /// Returns auth + provider configuration resolved from the current session auth state.
    ///
    /// This centralizes setup used by normal request paths so they stay in lockstep when
    /// auth/provider resolution changes.
    async fn current_client_setup(
        &self,
        provider: &SharedModelProvider,
    ) -> Result<CurrentClientSetup> {
        let auth = provider.auth().await;
        let api_provider = provider.api_provider().await?;
        let api_auth = provider.api_auth().await?;
        let auth_env_telemetry = Self::auth_env_telemetry_for_provider(provider);
        Ok(CurrentClientSetup {
            auth,
            api_provider,
            api_auth,
            auth_env_telemetry,
        })
    }
}

impl ModelClientSession {
    #[allow(clippy::too_many_arguments)]
    #[instrument(
        name = "model_client.stream_responses_api",
        level = "info",
        skip_all,
        fields(
            model = %model_info.slug,
            wire_api = "responses",
            transport = "responses_http",
            http.method = "POST"
        )
    )]
    async fn stream_responses_api(
        &self,
        provider: SharedModelProvider,
        prompt: &Prompt,
        model_info: &ModelInfo,
        session_telemetry: &SessionTelemetry,
        effort: Option<ReasoningEffortConfig>,
        summary: ReasoningSummaryConfig,
        service_tier: Option<String>,
        turn_metadata_header: Option<&str>,
        inference_trace: &InferenceTraceContext,
    ) -> Result<ResponseStream> {
        let mut auth_recovery = provider.unauthorized_recovery();
        let mut pending_retry = PendingUnauthorizedRetry::default();
        loop {
            let client_setup = self.client.current_client_setup(&provider).await?;
            let transport = ReqwestTransport::new(build_reqwest_client());
            let request_auth_context = AuthRequestTelemetryContext::new(
                client_setup.auth.as_ref().map(CodexAuth::auth_mode),
                client_setup.api_auth.as_ref(),
                pending_retry,
            );
            let (request_telemetry, sse_telemetry) = Self::build_streaming_telemetry(
                session_telemetry,
                request_auth_context,
                RequestRouteTelemetry::for_endpoint(RESPONSES_ENDPOINT),
                client_setup.auth_env_telemetry.clone(),
            );
            let mut options = ApiResponsesOptions {
                extra_headers: self
                    .client
                    .build_agent_headers(&provider, turn_metadata_header)
                    .await,
            };
            let request = provider_adapters::build_responses_request(
                provider.as_ref(),
                ResponsesRequestParams {
                    prompt,
                    model_info,
                    effort: effort.clone(),
                    summary,
                    service_tier: service_tier.clone(),
                    prompt_cache_key: self.client.prompt_cache_key(),
                },
            )?;
            let inference_trace_attempt = inference_trace.start_attempt();
            inference_trace_attempt.add_request_headers(&mut options.extra_headers);
            inference_trace_attempt.record_started(&request);
            let client = ApiResponsesClient::new(
                transport,
                client_setup.api_provider,
                client_setup.api_auth,
            )
            .with_telemetry(Some(request_telemetry), Some(sse_telemetry));

            match client.stream_request(request, options).await {
                Ok(stream) => {
                    return Ok(map_response_stream(
                        stream,
                        session_telemetry.clone(),
                        inference_trace_attempt,
                    ));
                }
                Err(ApiError::Transport(
                    unauthorized_transport @ TransportError::Http { status, .. },
                )) if status == StatusCode::UNAUTHORIZED => {
                    let response_debug_context =
                        extract_response_debug_context(&unauthorized_transport);
                    inference_trace_attempt.record_failed(
                        &unauthorized_transport,
                        response_debug_context.request_id.as_deref(),
                        /*output_items*/ &[],
                    );
                    pending_retry = PendingUnauthorizedRetry::from_recovery(
                        handle_unauthorized(
                            unauthorized_transport,
                            &mut auth_recovery,
                            session_telemetry,
                        )
                        .await?,
                    );
                }
                Err(err) => {
                    let response_debug_context =
                        extract_response_debug_context_from_api_error(&err);
                    let err = map_api_error(err);
                    inference_trace_attempt.record_failed(
                        &err,
                        response_debug_context.request_id.as_deref(),
                        /*output_items*/ &[],
                    );
                    return Err(err);
                }
            }
        }
    }

    /// Streams a turn via a provider-neutral wire API.
    ///
    /// The request is built as Astral's internal Agent IR, then encoded by the
    /// codex-api endpoint adapter for the selected provider protocol.
    #[allow(clippy::too_many_arguments)]
    #[instrument(
        name = "model_client.stream_agent_api",
        level = "info",
        skip_all,
        fields(
            model = %model_info.slug,
            wire_api = %wire_api,
            transport = "agent_http",
            http.method = "POST",
            turn.has_metadata_header = turn_metadata_header.is_some()
        )
    )]
    async fn stream_agent_api(
        &self,
        provider: SharedModelProvider,
        prompt: &Prompt,
        model_info: &ModelInfo,
        session_telemetry: &SessionTelemetry,
        effort: Option<ReasoningEffortConfig>,
        summary: ReasoningSummaryConfig,
        service_tier: Option<String>,
        turn_metadata_header: Option<&str>,
        inference_trace: &InferenceTraceContext,
        wire_api: WireApi,
        anthropic_cached_fold_enabled: bool,
    ) -> Result<ResponseStream> {
        let mut auth_recovery = provider.unauthorized_recovery();
        let mut pending_retry = PendingUnauthorizedRetry::default();
        loop {
            let client_setup = self.client.current_client_setup(&provider).await?;
            let transport = ReqwestTransport::new(build_reqwest_client());
            let request_auth_context = AuthRequestTelemetryContext::new(
                client_setup.auth.as_ref().map(CodexAuth::auth_mode),
                client_setup.api_auth.as_ref(),
                pending_retry,
            );
            let route = match wire_api {
                WireApi::AnthropicMessages => ANTHROPIC_MESSAGES_ENDPOINT,
                WireApi::ChatCompletions => CHAT_COMPLETIONS_ENDPOINT,
                WireApi::Responses => {
                    return Err(CodexErr::InvalidRequest(
                        "Responses requests must use the direct Responses transport".to_string(),
                    ));
                }
            };
            let (request_telemetry, sse_telemetry) = Self::build_streaming_telemetry(
                session_telemetry,
                request_auth_context,
                RequestRouteTelemetry::for_endpoint(route),
                client_setup.auth_env_telemetry.clone(),
            );
            let mut options = ApiAgentOptions::default();
            options.extra_headers.extend(
                self.client
                    .build_agent_headers(&provider, turn_metadata_header)
                    .await,
            );
            let provider_info = provider.info();
            let provider_flavor = provider_info.effective_provider_flavor();
            let provider_flavor_source = if provider_info.provider_flavor.is_some() {
                "explicit"
            } else {
                "inferred"
            };
            debug!(
                provider_name = %provider_info.name,
                provider_flavor = %provider_flavor,
                provider_flavor_source,
                "selected provider flavor"
            );
            let request = build_agent_request(AgentRequestBuildParams {
                prompt,
                model_info,
                effort: effort.clone(),
                summary,
                service_tier: service_tier.clone(),
                prompt_cache_key: self.client.prompt_cache_key(),
                provider_flavor: Some(provider_flavor.to_string()),
                provider_request_body: provider_info.request_body.clone(),
                provider_request_body_remove: provider_info.request_body_remove.clone(),
            })?;
            let anthropic_cache_fold = if matches!(wire_api, WireApi::AnthropicMessages)
                && anthropic_cached_fold_enabled
            {
                self.client.anthropic_cache_fold_options(&request).await
            } else {
                None
            };
            let used_anthropic_cache_fold = anthropic_cache_fold.is_some();
            let inference_trace_attempt = inference_trace.start_attempt();
            inference_trace_attempt.add_request_headers(&mut options.extra_headers);
            inference_trace_attempt.record_started(&request);
            let client =
                ApiAgentClient::new(transport, client_setup.api_provider, client_setup.api_auth)
                    .with_telemetry(Some(request_telemetry), Some(sse_telemetry));
            let stream_result = match wire_api {
                WireApi::AnthropicMessages => {
                    client
                        .stream_anthropic_messages(
                            request,
                            AnthropicMessagesOptions {
                                max_tokens: anthropic_max_tokens(model_info),
                                supports_image_input: model_info
                                    .input_modalities
                                    .contains(&InputModality::Image),
                                cache_fold: anthropic_cache_fold,
                                compact_input_placeholders: prompt.compact_input_placeholders,
                            },
                            options,
                        )
                        .await
                }
                WireApi::ChatCompletions => {
                    client
                        .stream_chat_completions(
                            request,
                            ChatCompletionsOptions {
                                max_tokens: None,
                                supports_image_input: model_info
                                    .input_modalities
                                    .contains(&InputModality::Image),
                            },
                            options,
                        )
                        .await
                }
                WireApi::Responses => Err(ApiError::Stream(
                    "responses wire api cannot use agent streaming client".to_string(),
                )),
            };

            match stream_result {
                Ok(stream) => {
                    let stream = map_response_stream(
                        stream,
                        session_telemetry.clone(),
                        inference_trace_attempt,
                    );
                    return Ok(stream);
                }
                Err(ApiError::Transport(
                    unauthorized_transport @ TransportError::Http { status, .. },
                )) if status == StatusCode::UNAUTHORIZED => {
                    let response_debug_context =
                        extract_response_debug_context(&unauthorized_transport);
                    inference_trace_attempt.record_failed(
                        &unauthorized_transport,
                        response_debug_context.request_id.as_deref(),
                        /*output_items*/ &[],
                    );
                    pending_retry = PendingUnauthorizedRetry::from_recovery(
                        handle_unauthorized(
                            unauthorized_transport,
                            &mut auth_recovery,
                            session_telemetry,
                        )
                        .await?,
                    );
                    continue;
                }
                Err(err) => {
                    let response_debug_context =
                        extract_response_debug_context_from_api_error(&err);
                    if matches!(wire_api, WireApi::AnthropicMessages)
                        && used_anthropic_cache_fold
                        && is_anthropic_cache_fold_protocol_error(&err)
                    {
                        warn!(
                            "anthropic cache fold request failed with protocol error; retrying without cache fold"
                        );
                        inference_trace_attempt.record_failed(
                            &err,
                            response_debug_context.request_id.as_deref(),
                            /*output_items*/ &[],
                        );
                        self.client.disable_anthropic_cache_fold().await;
                        continue;
                    }
                    let err = map_api_error(err);
                    inference_trace_attempt.record_failed(
                        &err,
                        response_debug_context.request_id.as_deref(),
                        /*output_items*/ &[],
                    );
                    return Err(err);
                }
            }
        }
    }

    /// Builds request and SSE telemetry for streaming API calls.
    fn build_streaming_telemetry(
        session_telemetry: &SessionTelemetry,
        auth_context: AuthRequestTelemetryContext,
        request_route_telemetry: RequestRouteTelemetry,
        auth_env_telemetry: AuthEnvTelemetry,
    ) -> (Arc<dyn RequestTelemetry>, Arc<dyn SseTelemetry>) {
        let telemetry = Arc::new(ApiTelemetry::new(
            session_telemetry.clone(),
            auth_context,
            request_route_telemetry,
            auth_env_telemetry,
        ));
        let request_telemetry: Arc<dyn RequestTelemetry> = telemetry.clone();
        let sse_telemetry: Arc<dyn SseTelemetry> = telemetry;
        (request_telemetry, sse_telemetry)
    }

    #[allow(clippy::too_many_arguments)]
    /// Streams a single model request within the current turn.
    ///
    /// The caller is responsible for passing per-turn settings explicitly (model selection,
    /// reasoning settings, telemetry context, and turn metadata). The trace context may be enabled
    /// or disabled, but is always explicit so transport paths do not need separate trace/no-trace
    /// branches.
    pub async fn stream(
        &mut self,
        provider: SharedModelProvider,
        prompt: &Prompt,
        model_info: &ModelInfo,
        session_telemetry: &SessionTelemetry,
        effort: Option<ReasoningEffortConfig>,
        summary: ReasoningSummaryConfig,
        service_tier: Option<String>,
        turn_metadata_header: Option<&str>,
        inference_trace: &InferenceTraceContext,
        anthropic_cached_fold_enabled: bool,
    ) -> Result<ResponseStream> {
        let wire_api = provider.info().wire_api;
        match wire_api {
            WireApi::Responses => {
                self.stream_responses_api(
                    provider,
                    prompt,
                    model_info,
                    session_telemetry,
                    effort,
                    summary,
                    service_tier,
                    turn_metadata_header,
                    inference_trace,
                )
                .await
            }
            WireApi::AnthropicMessages | WireApi::ChatCompletions => {
                self.stream_agent_api(
                    provider,
                    prompt,
                    model_info,
                    session_telemetry,
                    effort,
                    summary,
                    service_tier,
                    turn_metadata_header,
                    inference_trace,
                    wire_api,
                    anthropic_cached_fold_enabled,
                )
                .await
            }
        }
    }
}

fn is_anthropic_cache_fold_protocol_error(err: &ApiError) -> bool {
    match err {
        ApiError::InvalidRequest { .. } => true,
        ApiError::Api { status, .. } => status.as_u16() == 400,
        ApiError::Transport(TransportError::Http { status, .. }) => status.as_u16() == 400,
        ApiError::Stream(_)
        | ApiError::ContextWindowExceeded
        | ApiError::QuotaExceeded
        | ApiError::UsageNotIncluded
        | ApiError::Retryable { .. }
        | ApiError::RateLimit(_)
        | ApiError::CyberPolicy { .. }
        | ApiError::ServerOverloaded
        | ApiError::Transport(_) => false,
    }
}

/// Parses per-turn metadata into an HTTP header value.
///
/// Invalid values are treated as absent so callers can compare and propagate
/// metadata with the same sanitization path used when constructing headers.
fn parse_turn_metadata_header(turn_metadata_header: Option<&str>) -> Option<HeaderValue> {
    turn_metadata_header.and_then(|value| HeaderValue::from_str(value).ok())
}

fn parent_thread_id_header_value(parent_thread_id: Option<ThreadId>) -> Option<String> {
    parent_thread_id.map(|parent_thread_id| parent_thread_id.to_string())
}

const RESPONSE_STREAM_CHANNEL_CAPACITY: usize = 1600;
const STREAM_DROPPED_REASON: &str = "response stream dropped before provider terminal event";

fn map_response_stream(
    api_stream: codex_api::ResponseStream,
    session_telemetry: SessionTelemetry,
    inference_trace_attempt: InferenceTraceAttempt,
) -> ResponseStream {
    let codex_api::ResponseStream {
        rx_event,
        upstream_request_id,
    } = api_stream;
    let api_stream = codex_api::ResponseStream {
        rx_event,
        upstream_request_id: None,
    };
    map_response_events(
        upstream_request_id,
        api_stream,
        session_telemetry,
        inference_trace_attempt,
    )
}

fn map_response_events<S>(
    upstream_request_id: Option<String>,
    api_stream: S,
    session_telemetry: SessionTelemetry,
    inference_trace_attempt: InferenceTraceAttempt,
) -> ResponseStream
where
    S: futures::Stream<Item = std::result::Result<ModelStreamEvent, ApiError>>
        + Unpin
        + Send
        + 'static,
{
    let (tx_event, rx_event) =
        mpsc::channel::<Result<ModelStreamEvent>>(RESPONSE_STREAM_CHANNEL_CAPACITY);
    let consumer_dropped = CancellationToken::new();
    let consumer_dropped_for_stream = consumer_dropped.clone();

    tokio::spawn(async move {
        let mut logged_error = false;
        let mut items_added: Vec<TranscriptItem> = Vec::new();
        let mut api_stream = api_stream;
        let upstream_request_id = upstream_request_id.as_deref();
        if let Some(upstream_request_id) = upstream_request_id {
            feedback_tags!(last_model_request_id = upstream_request_id);
        }
        loop {
            let event = tokio::select! {
                _ = consumer_dropped.cancelled() => {
                    inference_trace_attempt.record_cancelled(
                        STREAM_DROPPED_REASON,
                        upstream_request_id,
                        &items_added,
                    );
                    return;
                }
                event = api_stream.next() => event,
            };
            let Some(event) = event else {
                break;
            };
            match event {
                Ok(ModelStreamEvent::OutputItemDone(item)) => {
                    items_added.push(item.clone());
                    if tx_event
                        .send(Ok(ModelStreamEvent::OutputItemDone(item)))
                        .await
                        .is_err()
                    {
                        inference_trace_attempt.record_cancelled(
                            STREAM_DROPPED_REASON,
                            upstream_request_id,
                            &items_added,
                        );
                        return;
                    }
                }
                Ok(ModelStreamEvent::Completed {
                    response_id,
                    token_usage,
                    end_turn,
                }) => {
                    feedback_tags!(last_model_response_id = &response_id);
                    if let Some(usage) = &token_usage {
                        session_telemetry.sse_event_completed(
                            usage.input_tokens,
                            usage.output_tokens,
                            Some(usage.cached_input_tokens),
                            Some(usage.reasoning_output_tokens),
                            usage.total_tokens,
                        );
                    }
                    inference_trace_attempt.record_completed(
                        &response_id,
                        upstream_request_id,
                        &token_usage,
                        &items_added,
                    );
                    if tx_event
                        .send(Ok(ModelStreamEvent::Completed {
                            response_id,
                            token_usage,
                            end_turn,
                        }))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(event) => {
                    if tx_event.send(Ok(event)).await.is_err() {
                        inference_trace_attempt.record_cancelled(
                            STREAM_DROPPED_REASON,
                            upstream_request_id,
                            &items_added,
                        );
                        return;
                    }
                }
                Err(err) => {
                    let response_debug_context =
                        extract_response_debug_context_from_api_error(&err);
                    let upstream_request_id =
                        upstream_request_id.or(response_debug_context.request_id.as_deref());
                    if let Some(upstream_request_id) = upstream_request_id {
                        feedback_tags!(last_model_request_id = upstream_request_id);
                    }
                    let mapped = map_api_error(err);
                    inference_trace_attempt.record_failed(
                        &mapped,
                        upstream_request_id,
                        &items_added,
                    );
                    if !logged_error {
                        session_telemetry.see_event_completed_failed(&mapped);
                        logged_error = true;
                    }
                    if tx_event.send(Err(mapped)).await.is_err() {
                        return;
                    }
                }
            }
        }
        inference_trace_attempt.record_failed(
            "stream closed before response.completed",
            upstream_request_id,
            &items_added,
        );
    });

    ResponseStream {
        rx_event,
        consumer_dropped: consumer_dropped_for_stream,
    }
}

/// Handles a 401 response by optionally refreshing external API-key auth once.
///
/// When refresh succeeds, the caller should retry the API call; otherwise
/// the mapped `CodexErr` is returned to the caller.
#[derive(Clone, Copy, Debug)]
struct UnauthorizedRecoveryExecution {
    mode: &'static str,
    phase: &'static str,
}

#[derive(Clone, Copy, Debug, Default)]
struct PendingUnauthorizedRetry {
    retry_after_unauthorized: bool,
    recovery_mode: Option<&'static str>,
    recovery_phase: Option<&'static str>,
}

impl PendingUnauthorizedRetry {
    fn from_recovery(recovery: UnauthorizedRecoveryExecution) -> Self {
        Self {
            retry_after_unauthorized: true,
            recovery_mode: Some(recovery.mode),
            recovery_phase: Some(recovery.phase),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct AuthRequestTelemetryContext {
    auth_mode: Option<&'static str>,
    auth_header_attached: bool,
    auth_header_name: Option<&'static str>,
    retry_after_unauthorized: bool,
    recovery_mode: Option<&'static str>,
    recovery_phase: Option<&'static str>,
}

impl AuthRequestTelemetryContext {
    fn new(
        auth_mode: Option<AuthMode>,
        api_auth: &dyn AuthProvider,
        retry: PendingUnauthorizedRetry,
    ) -> Self {
        let auth_telemetry = auth_header_telemetry(api_auth);
        Self {
            auth_mode: auth_mode.map(|mode| match mode {
                AuthMode::ApiKey => "ApiKey",
                AuthMode::Chatgpt => "Chatgpt",
            }),
            auth_header_attached: auth_telemetry.attached,
            auth_header_name: auth_telemetry.name,
            retry_after_unauthorized: retry.retry_after_unauthorized,
            recovery_mode: retry.recovery_mode,
            recovery_phase: retry.recovery_phase,
        }
    }
}

async fn handle_unauthorized(
    transport: TransportError,
    auth_recovery: &mut Option<UnauthorizedRecovery>,
    session_telemetry: &SessionTelemetry,
) -> Result<UnauthorizedRecoveryExecution> {
    let debug = extract_response_debug_context(&transport);
    if let Some(recovery) = auth_recovery
        && recovery.has_next()
    {
        let mode = recovery.mode_name();
        let phase = recovery.step_name();
        return match recovery.next().await {
            Ok(step_result) => {
                session_telemetry.record_auth_recovery(
                    mode,
                    phase,
                    "recovery_succeeded",
                    debug.request_id.as_deref(),
                    debug.cf_ray.as_deref(),
                    debug.auth_error.as_deref(),
                    debug.auth_error_code.as_deref(),
                    /*recovery_reason*/ None,
                    step_result.auth_state_changed(),
                );
                emit_feedback_auth_recovery_tags(
                    mode,
                    phase,
                    "recovery_succeeded",
                    debug.request_id.as_deref(),
                    debug.cf_ray.as_deref(),
                    debug.auth_error.as_deref(),
                    debug.auth_error_code.as_deref(),
                );
                Ok(UnauthorizedRecoveryExecution { mode, phase })
            }
            Err(RefreshTokenError::Permanent(failed)) => {
                session_telemetry.record_auth_recovery(
                    mode,
                    phase,
                    "recovery_failed_permanent",
                    debug.request_id.as_deref(),
                    debug.cf_ray.as_deref(),
                    debug.auth_error.as_deref(),
                    debug.auth_error_code.as_deref(),
                    /*recovery_reason*/ None,
                    /*auth_state_changed*/ None,
                );
                emit_feedback_auth_recovery_tags(
                    mode,
                    phase,
                    "recovery_failed_permanent",
                    debug.request_id.as_deref(),
                    debug.cf_ray.as_deref(),
                    debug.auth_error.as_deref(),
                    debug.auth_error_code.as_deref(),
                );
                Err(CodexErr::RefreshTokenFailed(failed))
            }
            Err(RefreshTokenError::Transient(other)) => {
                session_telemetry.record_auth_recovery(
                    mode,
                    phase,
                    "recovery_failed_transient",
                    debug.request_id.as_deref(),
                    debug.cf_ray.as_deref(),
                    debug.auth_error.as_deref(),
                    debug.auth_error_code.as_deref(),
                    /*recovery_reason*/ None,
                    /*auth_state_changed*/ None,
                );
                emit_feedback_auth_recovery_tags(
                    mode,
                    phase,
                    "recovery_failed_transient",
                    debug.request_id.as_deref(),
                    debug.cf_ray.as_deref(),
                    debug.auth_error.as_deref(),
                    debug.auth_error_code.as_deref(),
                );
                Err(CodexErr::Io(other))
            }
        };
    }

    let (mode, phase, recovery_reason) = match auth_recovery.as_ref() {
        Some(recovery) => (
            recovery.mode_name(),
            recovery.step_name(),
            Some(recovery.unavailable_reason()),
        ),
        None => ("none", "none", Some("auth_manager_missing")),
    };
    session_telemetry.record_auth_recovery(
        mode,
        phase,
        "recovery_not_run",
        debug.request_id.as_deref(),
        debug.cf_ray.as_deref(),
        debug.auth_error.as_deref(),
        debug.auth_error_code.as_deref(),
        recovery_reason,
        /*auth_state_changed*/ None,
    );
    emit_feedback_auth_recovery_tags(
        mode,
        phase,
        "recovery_not_run",
        debug.request_id.as_deref(),
        debug.cf_ray.as_deref(),
        debug.auth_error.as_deref(),
        debug.auth_error_code.as_deref(),
    );

    Err(map_api_error(ApiError::Transport(transport)))
}

struct ApiTelemetry {
    session_telemetry: SessionTelemetry,
    auth_context: AuthRequestTelemetryContext,
    request_route_telemetry: RequestRouteTelemetry,
    auth_env_telemetry: AuthEnvTelemetry,
}

impl ApiTelemetry {
    fn new(
        session_telemetry: SessionTelemetry,
        auth_context: AuthRequestTelemetryContext,
        request_route_telemetry: RequestRouteTelemetry,
        auth_env_telemetry: AuthEnvTelemetry,
    ) -> Self {
        Self {
            session_telemetry,
            auth_context,
            request_route_telemetry,
            auth_env_telemetry,
        }
    }
}

impl RequestTelemetry for ApiTelemetry {
    fn on_request(
        &self,
        attempt: u64,
        status: Option<HttpStatusCode>,
        error: Option<&TransportError>,
        duration: Duration,
    ) {
        let error_message = error.map(telemetry_transport_error_message);
        let status = status.map(|s| s.as_u16());
        let debug = error
            .map(extract_response_debug_context)
            .unwrap_or_default();
        self.session_telemetry.record_api_request(
            attempt,
            status,
            error_message.as_deref(),
            duration,
            self.auth_context.auth_header_attached,
            self.auth_context.auth_header_name,
            self.auth_context.retry_after_unauthorized,
            self.auth_context.recovery_mode,
            self.auth_context.recovery_phase,
            self.request_route_telemetry.endpoint,
            debug.request_id.as_deref(),
            debug.cf_ray.as_deref(),
            debug.auth_error.as_deref(),
            debug.auth_error_code.as_deref(),
        );
        emit_feedback_request_tags_with_auth_env(
            &FeedbackRequestTags {
                endpoint: self.request_route_telemetry.endpoint,
                auth_header_attached: self.auth_context.auth_header_attached,
                auth_header_name: self.auth_context.auth_header_name,
                auth_mode: self.auth_context.auth_mode,
                auth_retry_after_unauthorized: Some(self.auth_context.retry_after_unauthorized),
                auth_recovery_mode: self.auth_context.recovery_mode,
                auth_recovery_phase: self.auth_context.recovery_phase,
                auth_connection_reused: None,
                auth_request_id: debug.request_id.as_deref(),
                auth_cf_ray: debug.cf_ray.as_deref(),
                auth_error: debug.auth_error.as_deref(),
                auth_error_code: debug.auth_error_code.as_deref(),
                auth_recovery_followup_success: self
                    .auth_context
                    .retry_after_unauthorized
                    .then_some(error.is_none()),
                auth_recovery_followup_status: self
                    .auth_context
                    .retry_after_unauthorized
                    .then_some(status)
                    .flatten(),
            },
            &self.auth_env_telemetry,
        );
    }
}

impl SseTelemetry for ApiTelemetry {
    fn on_sse_poll(
        &self,
        result: &std::result::Result<
            Option<std::result::Result<Event, EventStreamError<TransportError>>>,
            tokio::time::error::Elapsed,
        >,
        duration: Duration,
    ) {
        self.session_telemetry.log_sse_event(result, duration);
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
