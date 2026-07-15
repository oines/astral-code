#![allow(clippy::expect_used)]

use anyhow::Result;
use codex_features::Feature;
use core_test_support::apps_test_server::AppsTestServer;
use core_test_support::apps_test_server::EXPLICIT_CALENDAR_CREATE_TOOL;
use core_test_support::apps_test_server::EXPLICIT_CALENDAR_MCP_NAMESPACE;
use core_test_support::apps_test_server::EXPLICIT_CALENDAR_MCP_SERVER_NAME;
use core_test_support::apps_test_server::search_capable_explicit_calendar_mcp_builder;
use core_test_support::responses;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::namespace_child_tool;
use core_test_support::responses::request_tool_description;
use core_test_support::responses::request_tool_name;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::wait_for_mcp_server;
use serde_json::Value;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_mode_only_exposes_direct_model_only_mcp_namespaces() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let apps_server = AppsTestServer::mount_searchable(&server).await?;
    let response = responses::mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-1"),
        ]),
    )
    .await;

    // The fixed upstream test uses the host-owned codex_apps server. Astral keeps that
    // apps/OAuth runtime out of this port, so exercise the same MCP path through an
    // explicitly configured server instead.
    let mut builder = search_capable_explicit_calendar_mcp_builder(apps_server.hosted_base_url)
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodeModeOnly)
                .expect("test config should allow feature update");
            config
                .features
                .disable(Feature::Collab)
                .expect("test config should allow feature update");
            config.code_mode.direct_only_tool_namespaces =
                vec![EXPLICIT_CALENDAR_MCP_NAMESPACE.to_string()];
        });
    let test = builder.build(&server).await?;
    wait_for_mcp_server(&test.codex, EXPLICIT_CALENDAR_MCP_SERVER_NAME).await?;
    test.submit_turn("inspect directly exposed MCP tools")
        .await?;
    let body = response.single_request().body_json();
    let tools = body
        .get("tools")
        .and_then(Value::as_array)
        .expect("request should contain tools");

    assert!(
        namespace_child_tool(
            &body,
            EXPLICIT_CALENDAR_MCP_NAMESPACE,
            EXPLICIT_CALENDAR_CREATE_TOOL,
        )
        .is_some(),
        "configured MCP namespace should remain top-level: {body}"
    );
    assert!(
        !tools
            .iter()
            .any(|tool| request_tool_name(tool) == Some("tool_search")),
        "configured MCP namespace should not be deferred: {body}"
    );
    let exec_description = tools.iter().find_map(|tool| {
        (request_tool_name(tool) == Some("exec"))
            .then(|| request_tool_description(tool))
            .flatten()
    });
    assert!(
        exec_description.is_some_and(|description| {
            !description.contains("mcp__calendar__calendar_create_event(args:")
        }),
        "direct-model-only MCP namespace should not be available through exec: {body}"
    );

    Ok(())
}
