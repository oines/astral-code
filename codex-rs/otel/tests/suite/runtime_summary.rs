use codex_otel::MetricsClient;
use codex_otel::MetricsConfig;
use codex_otel::Result;
use codex_otel::RuntimeMetricTotals;
use codex_otel::RuntimeMetricsSummary;
use codex_otel::SessionTelemetry;
use codex_otel::TelemetryAuthMode;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use eventsource_stream::Event as StreamEvent;
use opentelemetry_sdk::metrics::InMemoryMetricExporter;
use pretty_assertions::assert_eq;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

#[test]
fn runtime_metrics_summary_collects_tool_api_and_streaming_metrics() -> Result<()> {
    let exporter = InMemoryMetricExporter::default();
    let metrics = MetricsClient::new(
        MetricsConfig::in_memory("test", "codex-cli", env!("CARGO_PKG_VERSION"), exporter)
            .with_runtime_reader(),
    )?;
    let manager = SessionTelemetry::new(
        ThreadId::new(),
        "gpt-5.1",
        "gpt-5.1",
        Some("account-id".to_string()),
        /*account_email*/ None,
        Some(TelemetryAuthMode::ApiKey),
        "test_originator".to_string(),
        /*log_user_prompts*/ true,
        "tty".to_string(),
        SessionSource::Cli,
    )
    .with_metrics(metrics);

    manager.reset_runtime_metrics();

    manager.tool_result_with_tags(
        "shell",
        "call-1",
        "{\"cmd\":\"echo\"}",
        Duration::from_millis(250),
        /*success*/ true,
        "ok",
        &[],
        /*extra_trace_fields*/ &[],
    );
    manager.record_api_request(
        /*attempt*/ 1,
        Some(200),
        /*error*/ None,
        Duration::from_millis(300),
        /*auth_header_attached*/ false,
        /*auth_header_name*/ None,
        /*retry_after_unauthorized*/ false,
        /*recovery_mode*/ None,
        /*recovery_phase*/ None,
        "/chat/completions",
        /*request_id*/ None,
        /*cf_ray*/ None,
        /*auth_error*/ None,
        /*auth_error_code*/ None,
    );
    manager.record_websocket_request(
        Duration::from_millis(400),
        /*error*/ None,
        /*connection_reused*/ false,
    );
    let sse_response: std::result::Result<
        Option<std::result::Result<StreamEvent, eventsource_stream::EventStreamError<&str>>>,
        tokio::time::error::Elapsed,
    > = Ok(Some(Ok(StreamEvent {
        event: "chat.completion.chunk".to_string(),
        data: "{}".to_string(),
        id: String::new(),
        retry: None,
    })));
    manager.log_sse_event(&sse_response, Duration::from_millis(120));
    let ws_response: std::result::Result<
        Option<std::result::Result<Message, tokio_tungstenite::tungstenite::Error>>,
        codex_api::ApiError,
    > = Ok(Some(Ok(Message::Text(
        r#"{"type":"response.created"}"#.into(),
    ))));
    manager.record_websocket_event(&ws_response, Duration::from_millis(80));
    manager.record_duration(
        "codex.turn.ttft.duration_ms",
        Duration::from_millis(95),
        &[],
    );
    manager.record_duration(
        "codex.turn.ttfm.duration_ms",
        Duration::from_millis(180),
        &[],
    );

    let summary = manager
        .runtime_metrics_summary()
        .expect("runtime metrics summary should be available");
    let expected = RuntimeMetricsSummary {
        tool_calls: RuntimeMetricTotals {
            count: 1,
            duration_ms: 250,
        },
        api_calls: RuntimeMetricTotals {
            count: 1,
            duration_ms: 300,
        },
        streaming_events: RuntimeMetricTotals {
            count: 1,
            duration_ms: 120,
        },
        websocket_calls: RuntimeMetricTotals {
            count: 1,
            duration_ms: 400,
        },
        websocket_events: RuntimeMetricTotals {
            count: 1,
            duration_ms: 80,
        },
        turn_ttft_ms: 95,
        turn_ttfm_ms: 180,
    };
    assert_eq!(summary, expected);

    Ok(())
}
