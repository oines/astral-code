use pretty_assertions::assert_eq;
use serde_json::json;

use super::WebSearchRequest;
use super::brave_query;
use super::exa_body;
use super::jina_url;
use super::serpapi_query;
use super::tavily_body;

fn request() -> WebSearchRequest {
    WebSearchRequest {
        query: "rust async".to_string(),
        limit: 5,
    }
}

#[test]
fn builds_minimal_tavily_request_body() {
    assert_eq!(
        tavily_body(&request()),
        json!({
            "query": "rust async",
            "max_results": 5,
        })
    );
}

#[test]
fn builds_minimal_exa_request_body() {
    assert_eq!(
        exa_body(&request()),
        json!({
            "query": "rust async",
            "numResults": 5,
        })
    );
}

#[test]
fn builds_minimal_jina_request_url() {
    assert_eq!(
        jina_url(&request())
            .expect("Jina URL should build")
            .as_str(),
        "https://s.jina.ai/?q=rust+async"
    );
}

#[test]
fn builds_minimal_brave_query() {
    assert_eq!(
        brave_query(&request()),
        vec![("q", "rust async".to_string()), ("count", "5".to_string())]
    );
}

#[test]
fn builds_minimal_serpapi_query() {
    assert_eq!(
        serpapi_query(&request(), "serp-key"),
        vec![
            ("engine", "google".to_string()),
            ("q", "rust async".to_string()),
            ("num", "5".to_string()),
            ("api_key", "serp-key".to_string()),
        ]
    );
}
