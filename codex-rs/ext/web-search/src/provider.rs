use std::collections::HashSet;

use codex_config::config_toml::WebSearchProvider;
use codex_config::config_toml::WebSearchRuntimeConfig;
use reqwest::header::ACCEPT;
use serde_json::Value;
use url::Url;

use crate::request::WebSearchRequest;
use crate::request::brave_query;
use crate::request::exa_body;
use crate::request::jina_url;
use crate::request::serpapi_query;
use crate::request::tavily_body;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WebSearchResult {
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) snippet: Option<String>,
    pub(crate) published_at: Option<String>,
}

pub(crate) async fn search(
    client: &reqwest::Client,
    config: &WebSearchRuntimeConfig,
    request: WebSearchRequest,
) -> Result<Vec<WebSearchResult>, String> {
    let value = match config.provider {
        WebSearchProvider::Tavily => tavily_search(client, config, &request).await?,
        WebSearchProvider::Exa => exa_search(client, config, &request).await?,
        WebSearchProvider::Jina => jina_search(client, config, &request).await?,
        WebSearchProvider::Brave => brave_search(client, config, &request).await?,
        WebSearchProvider::SerpApi => serpapi_search(client, config, &request).await?,
    };

    let results = match config.provider {
        WebSearchProvider::Tavily => parse_tavily_results(&value),
        WebSearchProvider::Exa => parse_exa_results(&value),
        WebSearchProvider::Jina => parse_jina_results(&value),
        WebSearchProvider::Brave => parse_brave_results(&value),
        WebSearchProvider::SerpApi => parse_serpapi_results(&value),
    };

    Ok(normalize_results(results))
}

async fn tavily_search(
    client: &reqwest::Client,
    config: &WebSearchRuntimeConfig,
    request: &WebSearchRequest,
) -> Result<Value, String> {
    let response = client
        .post("https://api.tavily.com/search")
        .bearer_auth(config.api_key.expose_secret())
        .json(&tavily_body(request))
        .send()
        .await
        .map_err(|err| request_error("Tavily", err))?;

    response_json(response, "Tavily").await
}

async fn exa_search(
    client: &reqwest::Client,
    config: &WebSearchRuntimeConfig,
    request: &WebSearchRequest,
) -> Result<Value, String> {
    let response = client
        .post("https://api.exa.ai/search")
        .header("x-api-key", config.api_key.expose_secret())
        .json(&exa_body(request))
        .send()
        .await
        .map_err(|err| request_error("Exa", err))?;

    response_json(response, "Exa").await
}

async fn jina_search(
    client: &reqwest::Client,
    config: &WebSearchRuntimeConfig,
    request: &WebSearchRequest,
) -> Result<Value, String> {
    let url = jina_url(request)?;

    let response = client
        .get(url)
        .bearer_auth(config.api_key.expose_secret())
        .header(ACCEPT, "application/json")
        .send()
        .await
        .map_err(|err| request_error("Jina", err))?;

    response_json(response, "Jina").await
}

async fn brave_search(
    client: &reqwest::Client,
    config: &WebSearchRuntimeConfig,
    request: &WebSearchRequest,
) -> Result<Value, String> {
    let response = client
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("X-Subscription-Token", config.api_key.expose_secret())
        .header(ACCEPT, "application/json")
        .query(&brave_query(request))
        .send()
        .await
        .map_err(|err| request_error("Brave", err))?;

    response_json(response, "Brave").await
}

async fn serpapi_search(
    client: &reqwest::Client,
    config: &WebSearchRuntimeConfig,
    request: &WebSearchRequest,
) -> Result<Value, String> {
    let response = client
        .get("https://serpapi.com/search.json")
        .query(&serpapi_query(request, config.api_key.expose_secret()))
        .send()
        .await
        .map_err(|err| request_error("SerpAPI", err))?;

    response_json(response, "SerpAPI").await
}

async fn response_json(response: reqwest::Response, provider: &str) -> Result<Value, String> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| format!("{provider} search response read failed: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "{provider} search returned HTTP {status}: {}",
            truncate_for_error(&text)
        ));
    }

    serde_json::from_str(&text).map_err(|err| {
        format!(
            "{provider} search returned invalid JSON: {err}; body: {}",
            truncate_for_error(&text)
        )
    })
}

fn request_error(provider: &str, error: reqwest::Error) -> String {
    format!("{provider} search request failed: {}", error.without_url())
}

pub(crate) fn parse_tavily_results(value: &Value) -> Vec<WebSearchResult> {
    value
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            result_from_fields(
                item,
                &["title"],
                &["url"],
                &["content", "snippet"],
                &["published_date", "publishedDate", "date"],
            )
        })
        .collect()
}

pub(crate) fn parse_exa_results(value: &Value) -> Vec<WebSearchResult> {
    value
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let snippet = string_field(item, &["summary", "text"])
                .or_else(|| first_string_in_array(item.get("highlights")));
            result_from_parts(
                string_field(item, &["title"])?,
                string_field(item, &["url"])?,
                snippet,
                string_field(item, &["publishedDate", "published_date", "date"]),
            )
        })
        .collect()
}

pub(crate) fn parse_jina_results(value: &Value) -> Vec<WebSearchResult> {
    let results = value
        .as_array()
        .or_else(|| value.get("data").and_then(Value::as_array))
        .or_else(|| value.get("results").and_then(Value::as_array));

    results
        .into_iter()
        .flatten()
        .filter_map(|item| {
            result_from_fields(
                item,
                &["title"],
                &["url", "source"],
                &["content", "description", "snippet"],
                &["timestamp", "published_at", "publishedDate", "date"],
            )
        })
        .collect()
}

pub(crate) fn parse_brave_results(value: &Value) -> Vec<WebSearchResult> {
    value
        .pointer("/web/results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            result_from_parts(
                string_field(item, &["title"])?,
                string_field(item, &["url"])?,
                string_field(item, &["description", "snippet"]),
                string_field(item, &["age", "page_age", "published"]),
            )
        })
        .collect()
}

pub(crate) fn parse_serpapi_results(value: &Value) -> Vec<WebSearchResult> {
    value
        .get("organic_results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            result_from_parts(
                string_field(item, &["title"])?,
                string_field(item, &["link", "url"])?,
                string_field(item, &["snippet"]),
                string_field(item, &["date"]),
            )
        })
        .collect()
}

fn result_from_fields(
    value: &Value,
    title_fields: &[&str],
    url_fields: &[&str],
    snippet_fields: &[&str],
    published_fields: &[&str],
) -> Option<WebSearchResult> {
    result_from_parts(
        string_field(value, title_fields)?,
        string_field(value, url_fields)?,
        string_field(value, snippet_fields),
        string_field(value, published_fields),
    )
}

fn result_from_parts(
    title: String,
    url: String,
    snippet: Option<String>,
    published_at: Option<String>,
) -> Option<WebSearchResult> {
    let title = clean_inline_text(&title);
    let url = clean_inline_text(&url);
    if title.is_empty() || url.is_empty() {
        return None;
    }

    Some(WebSearchResult {
        title,
        url,
        snippet: snippet
            .map(|snippet| truncate_chars(&clean_inline_text(&snippet), 900))
            .filter(|snippet| !snippet.is_empty()),
        published_at: published_at
            .map(|published_at| truncate_chars(&clean_inline_text(&published_at), 120))
            .filter(|published_at| !published_at.is_empty()),
    })
}

fn normalize_results(mut results: Vec<WebSearchResult>) -> Vec<WebSearchResult> {
    let mut seen = HashSet::new();
    results.retain(|result| {
        let Some(key) = normalize_url_key(&result.url) else {
            return false;
        };
        seen.insert(key)
    });
    results
}

fn normalize_url_key(url: &str) -> Option<String> {
    let mut parsed = Url::parse(url).ok()?;
    parsed.set_fragment(None);
    Some(parsed.to_string())
}

fn string_field(value: &Value, fields: &[&str]) -> Option<String> {
    fields
        .iter()
        .filter_map(|field| value.get(*field))
        .find_map(|value| match value {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
}

fn first_string_in_array(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn clean_inline_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_for_error(value: &str) -> String {
    truncate_chars(&clean_inline_text(value), 500)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut iter = value.char_indices();
    let Some((index, _)) = iter.nth(max_chars) else {
        return value.to_string();
    };
    value[..index].to_string()
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
