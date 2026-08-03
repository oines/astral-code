use codex_config::config_toml::WebSearchRuntimeConfig;
use codex_core::web_search_action_detail;
use codex_extension_api::ExtensionTurnItem;
use codex_extension_api::FunctionCallError;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolOutput;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_protocol::items::WebSearchItem;
use codex_protocol::models::WebSearchAction;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ToolExposure;
use codex_tools::default_namespace_description;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::approx_bytes_for_tokens;
use codex_utils_output_truncation::formatted_truncate_text;
use schemars::JsonSchema;
use schemars::schema_for;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::fetch;
use crate::fetch::WebFetchInput;
use crate::output::WebToolOutput;
use crate::provider;
use crate::provider::WebSearchResult;
use crate::request::WebSearchRequest;

pub(crate) const WEB_NAMESPACE: &str = "web";
pub(crate) const SEARCH_TOOL_NAME: &str = "search";
pub(crate) const FETCH_TOOL_NAME: &str = "fetch";

const WEB_SEARCH_DESCRIPTION: &str = "Search the web and return a concise list of relevant results with titles, URLs, snippets, and dates when available. Only query is required; omit domains, recency, and limit to use the configured provider's defaults.";
const WEB_FETCH_DESCRIPTION: &str =
    "Fetch a web page by URL and return cleaned markdown or text content with noisy data removed.";
const MAX_WEB_SEARCH_OUTPUT_TOKENS: usize = 8_000;

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
struct WebSearchInput {
    /// Search query.
    query: String,
    /// Optional hostnames to restrict the upstream search to.
    domains: Option<Vec<String>>,
    /// Optional maximum result age in whole days.
    #[schemars(range(min = 1))]
    recency: Option<u32>,
    /// Optional result count passed directly to the upstream provider.
    #[schemars(range(min = 1))]
    limit: Option<usize>,
}

pub(crate) struct WebSearchTool {
    pub(crate) client: reqwest::Client,
    pub(crate) config: WebSearchRuntimeConfig,
}

impl ToolExecutor<ToolCall> for WebSearchTool {
    fn tool_name(&self) -> ToolName {
        ToolName::namespaced(WEB_NAMESPACE, SEARCH_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        web_tool_spec::<WebSearchInput>(SEARCH_TOOL_NAME, WEB_SEARCH_DESCRIPTION)
    }

    fn exposure(&self) -> ToolExposure {
        ToolExposure::Direct
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn handle(&self, call: ToolCall) -> codex_extension_api::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(call))
    }
}

impl WebSearchTool {
    async fn handle_call(&self, call: ToolCall) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let input: WebSearchInput = parse_input(&call)?;
        let request = WebSearchRequest::from_input(
            input.query,
            input.domains,
            input.recency,
            input.limit,
            chrono::Utc::now().date_naive(),
        )
        .map_err(FunctionCallError::RespondToModel)?;
        let query = request.query.clone();
        let action = WebSearchAction::Search {
            query: Some(query.clone()),
            queries: None,
        };
        call.turn_item_emitter
            .emit_started(web_search_item(&call.call_id, action.clone()))
            .await;
        let search_result = provider::search(&self.client, &self.config, request).await;
        call.turn_item_emitter
            .emit_completed(web_search_item(&call.call_id, action))
            .await;
        let results = match search_result {
            Ok(results) => results,
            Err(error) => {
                let output = bound_search_output(
                    &format!("Web search failed for query \"{query}\": {error}"),
                    call.truncation_policy,
                );
                return Ok(Box::new(WebToolOutput::failure(output)));
            }
        };

        Ok(Box::new(WebToolOutput::new(format_search_results(
            &query,
            &results,
            call.truncation_policy,
        ))))
    }
}

pub(crate) struct WebFetchTool {
    pub(crate) client: reqwest::Client,
}

impl ToolExecutor<ToolCall> for WebFetchTool {
    fn tool_name(&self) -> ToolName {
        ToolName::namespaced(WEB_NAMESPACE, FETCH_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        web_tool_spec::<WebFetchInput>(FETCH_TOOL_NAME, WEB_FETCH_DESCRIPTION)
    }

    fn exposure(&self) -> ToolExposure {
        ToolExposure::Direct
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn handle(&self, call: ToolCall) -> codex_extension_api::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(call))
    }
}

impl WebFetchTool {
    async fn handle_call(&self, call: ToolCall) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let input: WebFetchInput = parse_input(&call)?;
        let action = WebSearchAction::OpenPage {
            url: Some(visible_fetch_url(&input.url)),
        };
        let fetch_result = fetch::fetch(&self.client, input).await;
        call.turn_item_emitter
            .emit_completed(web_search_item(&call.call_id, action))
            .await;
        let output = match fetch_result {
            Ok(output) => WebToolOutput::new(output),
            Err(error) => WebToolOutput::failure(format!("Web fetch failed: {error}")),
        };

        Ok(Box::new(output))
    }
}

fn visible_fetch_url(url: &str) -> String {
    const MAX_VISIBLE_URL_CHARS: usize = 512;
    let trimmed = url.trim();
    if trimmed.chars().count() <= MAX_VISIBLE_URL_CHARS {
        return trimmed.to_string();
    }
    let prefix = trimmed
        .chars()
        .take(MAX_VISIBLE_URL_CHARS)
        .collect::<String>();
    format!("{prefix}...")
}

fn parse_input<T>(call: &ToolCall) -> Result<T, FunctionCallError>
where
    T: DeserializeOwned,
{
    let arguments = call.function_arguments()?;
    serde_json::from_str(arguments)
        .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))
}

fn web_tool_spec<T>(name: &str, description: &str) -> ToolSpec
where
    T: JsonSchema,
{
    let schema = schema_for!(T);
    let schema_value = serde_json::to_value(schema)
        .unwrap_or_else(|err| panic!("{name} schema should serialize to JSON: {err}"));
    let parameters = parse_tool_input_schema(&schema_value)
        .unwrap_or_else(|err| panic!("{name} schema should parse: {err}"));

    ToolSpec::Namespace(ResponsesApiNamespace {
        name: WEB_NAMESPACE.to_string(),
        description: default_namespace_description(WEB_NAMESPACE),
        tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
            name: name.to_string(),
            description: description.to_string(),
            strict: true,
            parameters,
            output_schema: None,
            defer_loading: None,
        })],
    })
}

fn web_search_item(call_id: &str, action: WebSearchAction) -> ExtensionTurnItem {
    ExtensionTurnItem::WebSearch(WebSearchItem {
        id: call_id.to_string(),
        query: web_search_action_detail(&action),
        action,
    })
}

pub(crate) fn format_search_results(
    query: &str,
    results: &[WebSearchResult],
    truncation_policy: TruncationPolicy,
) -> String {
    let output = if results.is_empty() {
        format!("Search query: \"{query}\"\nNo search results found.")
    } else {
        let mut output = format!(
            "Search query: \"{query}\"\nResults returned: {}\n",
            results.len()
        );
        for (index, result) in results.iter().enumerate() {
            output.push('\n');
            output.push_str(&format!("{}. Title: {}\n", index + 1, result.title));
            output.push_str(&format!("   URL: {}\n", result.url));
            if let Some(published_at) = &result.published_at {
                output.push_str(&format!("   Published: {published_at}\n"));
            }
            if let Some(snippet) = &result.snippet {
                output.push_str(&format!("   Snippet: {snippet}\n"));
            }
        }
        output
    };
    bound_search_output(&output, truncation_policy)
}

fn bound_search_output(output: &str, truncation_policy: TruncationPolicy) -> String {
    let truncation_policy = match truncation_policy {
        TruncationPolicy::Bytes(bytes) => TruncationPolicy::Bytes(
            bytes.min(approx_bytes_for_tokens(MAX_WEB_SEARCH_OUTPUT_TOKENS)),
        ),
        TruncationPolicy::Tokens(tokens) => {
            TruncationPolicy::Tokens(tokens.min(MAX_WEB_SEARCH_OUTPUT_TOKENS))
        }
    };
    formatted_truncate_text(output, truncation_policy)
}

#[cfg(test)]
#[path = "tool_tests.rs"]
mod tests;
