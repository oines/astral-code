use pretty_assertions::assert_eq;
use serde_json::json;

use super::WebSearchResult;
use super::normalize_results;
use super::parse_brave_results;
use super::parse_exa_results;
use super::parse_jina_results;
use super::parse_serpapi_results;
use super::parse_tavily_results;

#[test]
fn parses_tavily_results() {
    let value = json!({
        "results": [{
            "title": "Tavily Result",
            "url": "https://example.com/a",
            "content": " useful snippet ",
            "score": 0.8
        }]
    });

    assert_eq!(
        parse_tavily_results(&value),
        vec![WebSearchResult {
            title: "Tavily Result".to_string(),
            url: "https://example.com/a".to_string(),
            snippet: Some("useful snippet".to_string()),
            published_at: None,
        }]
    );
}

#[test]
fn parses_exa_results_preferring_summary() {
    let value = json!({
        "results": [{
            "title": "Exa Result",
            "url": "https://example.com/b",
            "publishedDate": "2026-01-02T00:00:00Z",
            "summary": "summary text",
            "text": "fuller text",
            "highlights": ["highlight text"],
            "highlightScores": [0.4]
        }]
    });

    assert_eq!(
        parse_exa_results(&value),
        vec![WebSearchResult {
            title: "Exa Result".to_string(),
            url: "https://example.com/b".to_string(),
            snippet: Some("summary text".to_string()),
            published_at: Some("2026-01-02T00:00:00Z".to_string()),
        }]
    );
}

#[test]
fn parses_jina_data_results() {
    let value = json!({
        "data": [{
            "title": "Jina Result",
            "url": "https://example.com/c",
            "content": "reader content",
            "timestamp": "2026-01-03"
        }]
    });

    assert_eq!(
        parse_jina_results(&value),
        vec![WebSearchResult {
            title: "Jina Result".to_string(),
            url: "https://example.com/c".to_string(),
            snippet: Some("reader content".to_string()),
            published_at: Some("2026-01-03".to_string()),
        }]
    );
}

#[test]
fn parses_brave_results() {
    let value = json!({
        "web": {
            "results": [{
                "title": "Brave Result",
                "url": "https://example.com/d",
                "description": "brave snippet",
                "age": "2 days ago"
            }]
        }
    });

    assert_eq!(
        parse_brave_results(&value),
        vec![WebSearchResult {
            title: "Brave Result".to_string(),
            url: "https://example.com/d".to_string(),
            snippet: Some("brave snippet".to_string()),
            published_at: Some("2 days ago".to_string()),
        }]
    );
}

#[test]
fn parses_serpapi_results() {
    let value = json!({
        "organic_results": [{
            "position": 2,
            "title": "SerpAPI Result",
            "link": "https://example.com/e",
            "snippet": "serp snippet",
            "date": "Jan 4, 2026"
        }]
    });

    assert_eq!(
        parse_serpapi_results(&value),
        vec![WebSearchResult {
            title: "SerpAPI Result".to_string(),
            url: "https://example.com/e".to_string(),
            snippet: Some("serp snippet".to_string()),
            published_at: Some("Jan 4, 2026".to_string()),
        }]
    );
}

#[test]
fn normalization_preserves_provider_order_and_only_removes_duplicate_urls() {
    let results = vec![
        WebSearchResult {
            title: "First".to_string(),
            url: "https://example.com/a#section".to_string(),
            snippet: None,
            published_at: None,
        },
        WebSearchResult {
            title: "Second".to_string(),
            url: "https://example.com/b".to_string(),
            snippet: None,
            published_at: None,
        },
        WebSearchResult {
            title: "Duplicate".to_string(),
            url: "https://example.com/a".to_string(),
            snippet: None,
            published_at: None,
        },
    ];

    assert_eq!(
        normalize_results(results),
        vec![
            WebSearchResult {
                title: "First".to_string(),
                url: "https://example.com/a#section".to_string(),
                snippet: None,
                published_at: None,
            },
            WebSearchResult {
                title: "Second".to_string(),
                url: "https://example.com/b".to_string(),
                snippet: None,
                published_at: None,
            },
        ]
    );
}
