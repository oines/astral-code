use serde_json::Value;
use serde_json::json;
use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WebSearchRequest {
    pub(crate) query: String,
    pub(crate) limit: usize,
}

pub(crate) fn tavily_body(request: &WebSearchRequest) -> Value {
    json!({
        "query": request.query,
        "max_results": request.limit,
    })
}

pub(crate) fn exa_body(request: &WebSearchRequest) -> Value {
    json!({
        "query": request.query,
        "numResults": request.limit,
    })
}

pub(crate) fn jina_url(request: &WebSearchRequest) -> Result<Url, String> {
    let mut url =
        Url::parse("https://s.jina.ai/").map_err(|err| format!("invalid Jina URL: {err}"))?;
    url.query_pairs_mut().append_pair("q", &request.query);
    Ok(url)
}

pub(crate) fn brave_query(request: &WebSearchRequest) -> Vec<(&'static str, String)> {
    vec![
        ("q", request.query.clone()),
        ("count", request.limit.to_string()),
    ]
}

pub(crate) fn serpapi_query(
    request: &WebSearchRequest,
    api_key: &str,
) -> Vec<(&'static str, String)> {
    vec![
        ("engine", "google".to_string()),
        ("q", request.query.clone()),
        ("num", request.limit.to_string()),
        ("api_key", api_key.to_string()),
    ]
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
