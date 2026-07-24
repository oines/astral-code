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
use schemars::JsonSchema;
use schemars::schema_for;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::fetch;
use crate::fetch::WebFetchInput;
use crate::output::WebToolOutput;
use crate::provider;
use crate::provider::WebSearchRequest;
use crate::provider::WebSearchResult;

pub(crate) const WEB_NAMESPACE: &str = "web";
pub(crate) const SEARCH_TOOL_NAME: &str = "search";
pub(crate) const FETCH_TOOL_NAME: &str = "fetch";

const WEB_SEARCH_DESCRIPTION: &str = "Search the web and return a concise list of relevant results with titles, URLs, snippets, and dates when available.";
const WEB_FETCH_DESCRIPTION: &str =
    "Fetch a web page by URL and return cleaned markdown or text content with noisy data removed.";

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
struct WebSearchInput {
    query: String,
    #[schemars(range(min = 1, max = 20))]
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
        let query = input.query.trim();
        if query.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "query must not be empty".to_string(),
            ));
        }

        let limit = input
            .limit
            .unwrap_or(self.config.default_limit)
            .clamp(1, self.config.max_limit);
        let action = WebSearchAction::Search {
            query: Some(query.to_string()),
            queries: None,
        };
        call.turn_item_emitter
            .emit_started(web_search_item(&call.call_id, action.clone()))
            .await;
        let search_result = provider::search(
            &self.client,
            &self.config,
            WebSearchRequest {
                query: query.to_string(),
                limit,
            },
        )
        .await;
        call.turn_item_emitter
            .emit_completed(web_search_item(&call.call_id, action))
            .await;
        let results = match search_result {
            Ok(results) => results,
            Err(error) => {
                return Ok(Box::new(WebToolOutput::failure(format!(
                    "Web search failed for query \"{query}\": {error}"
                ))));
            }
        };

        Ok(Box::new(WebToolOutput::new(format_search_results(
            query, &results,
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

pub(crate) fn format_search_results(query: &str, results: &[WebSearchResult]) -> String {
    if results.is_empty() {
        return format!("Search query: \"{query}\"\nNo search results found.");
    }

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
}

#[cfg(test)]
#[path = "tool_tests.rs"]
mod tests;
