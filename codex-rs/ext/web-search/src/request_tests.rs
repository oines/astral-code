use chrono::NaiveDate;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::WebSearchRequest;
use super::brave_query;
use super::exa_body;
use super::jina_url;
use super::serpapi_query;
use super::tavily_body;

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 31).expect("date should be valid")
}

fn minimal_request() -> WebSearchRequest {
    WebSearchRequest::from_input(" rust async ".to_string(), None, None, None, today())
        .expect("request should be valid")
}

fn filtered_request() -> WebSearchRequest {
    WebSearchRequest::from_input(
        "rust async".to_string(),
        Some(vec![
            "Example.COM".to_string(),
            "docs.rs".to_string(),
            "example.com".to_string(),
        ]),
        Some(7),
        Some(6),
        today(),
    )
    .expect("request should be valid")
}

#[test]
fn omitted_options_are_not_sent_to_any_provider() {
    let request = minimal_request();

    assert_eq!(tavily_body(&request), json!({"query": "rust async"}));
    assert_eq!(exa_body(&request), json!({"query": "rust async"}));
    assert_eq!(
        jina_url(&request).expect("Jina URL should build").as_str(),
        "https://s.jina.ai/?q=rust+async"
    );
    assert_eq!(brave_query(&request), vec![("q", "rust async".to_string())]);
    assert_eq!(
        serpapi_query(&request, "serp-key").expect("SerpAPI query should build"),
        vec![
            ("engine", "google".to_string()),
            ("q", "rust async".to_string()),
            ("api_key", "serp-key".to_string()),
        ]
    );
}

#[test]
fn options_map_to_each_providers_native_request_shape() {
    let request = filtered_request();

    assert_eq!(
        tavily_body(&request),
        json!({
            "query": "rust async",
            "max_results": 6,
            "include_domains": ["example.com", "docs.rs"],
            "start_date": "2026-07-24",
        })
    );
    assert_eq!(
        exa_body(&request),
        json!({
            "query": "rust async",
            "numResults": 6,
            "includeDomains": ["example.com", "docs.rs"],
            "startPublishedDate": "2026-07-24T00:00:00.000Z",
        })
    );
    assert_eq!(
        brave_query(&request),
        vec![
            (
                "q",
                "rust async (site:example.com OR site:docs.rs)".to_string(),
            ),
            ("count", "6".to_string()),
            ("freshness", "2026-07-24to2026-07-31".to_string(),),
        ]
    );
    let serpapi_request = WebSearchRequest::from_input(
        "rust async".to_string(),
        Some(vec!["example.com".to_string(), "docs.rs".to_string()]),
        Some(7),
        None,
        today(),
    )
    .expect("request should be valid");
    assert_eq!(
        serpapi_query(&serpapi_request, "serp-key").expect("SerpAPI query should build"),
        vec![
            ("engine", "google".to_string()),
            (
                "q",
                "rust async (site:example.com OR site:docs.rs)".to_string(),
            ),
            (
                "tbs",
                "cdr:1,cd_min:07/24/2026,cd_max:07/31/2026".to_string(),
            ),
            ("api_key", "serp-key".to_string()),
        ]
    );
}

#[test]
fn provider_specific_parameter_support_is_explicit() {
    let request = WebSearchRequest::from_input(
        "rust async".to_string(),
        Some(vec!["example.com".to_string(), "docs.rs".to_string()]),
        None,
        Some(6),
        today(),
    )
    .expect("request should be valid");
    assert_eq!(
        jina_url(&request).expect("Jina URL should build").as_str(),
        "https://s.jina.ai/?q=rust+async&site=example.com&site=docs.rs&count=6"
    );

    assert_eq!(
        jina_url(&filtered_request()),
        Err(
            "Jina search does not support recency; omit recency or configure another web search provider"
                .to_string()
        )
    );
    let limited_request =
        WebSearchRequest::from_input("rust async".to_string(), None, None, Some(6), today())
            .expect("request should be valid");
    assert_eq!(
        serpapi_query(&limited_request, "serp-key"),
        Err(
            "SerpAPI Google search does not support limit; omit limit or configure another web search provider"
                .to_string()
        )
    );
}

#[test]
fn rejects_invalid_values_and_treats_an_empty_domain_list_as_absent() {
    assert_eq!(
        WebSearchRequest::from_input("query".to_string(), Some(Vec::new()), None, None, today(),),
        Ok(
            WebSearchRequest::from_input("query".to_string(), None, None, None, today())
                .expect("request should be valid")
        )
    );
    assert_eq!(
        WebSearchRequest::from_input(
            "query".to_string(),
            Some(vec!["https://example.com".to_string()]),
            None,
            None,
            today(),
        ),
        Err("invalid domain; use a hostname without a scheme".to_string())
    );
    let oversized_domain = format!("https://{}", "a".repeat(50_000));
    assert_eq!(
        WebSearchRequest::from_input(
            "query".to_string(),
            Some(vec![oversized_domain]),
            None,
            None,
            today(),
        ),
        Err("invalid domain; use a hostname without a scheme".to_string())
    );
    assert_eq!(
        WebSearchRequest::from_input("query".to_string(), None, Some(0), None, today()),
        Err("recency must be greater than zero".to_string())
    );
    assert_eq!(
        WebSearchRequest::from_input("query".to_string(), None, None, Some(0), today()),
        Err("limit must be greater than zero".to_string())
    );
}
