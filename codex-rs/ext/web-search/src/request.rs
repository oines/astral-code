use std::collections::HashSet;

use chrono::Days;
use chrono::NaiveDate;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use url::Host;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DateRange {
    start: NaiveDate,
    end: NaiveDate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WebSearchRequest {
    pub(crate) query: String,
    domains: Option<Vec<String>>,
    date_range: Option<DateRange>,
    limit: Option<usize>,
}

impl WebSearchRequest {
    pub(crate) fn from_input(
        query: String,
        domains: Option<Vec<String>>,
        recency: Option<u32>,
        limit: Option<usize>,
        today: NaiveDate,
    ) -> Result<Self, String> {
        let query = query.trim().to_string();
        if query.is_empty() {
            return Err("query must not be empty".to_string());
        }
        if limit == Some(0) {
            return Err("limit must be greater than zero".to_string());
        }

        let domains = normalize_domains(domains)?;
        let date_range = recency
            .map(|days| {
                if days == 0 {
                    return Err("recency must be greater than zero".to_string());
                }
                let start = today
                    .checked_sub_days(Days::new(u64::from(days)))
                    .ok_or_else(|| "recency is too large".to_string())?;
                Ok(DateRange { start, end: today })
            })
            .transpose()?;

        Ok(Self {
            query,
            domains,
            date_range,
            limit,
        })
    }
}

pub(crate) fn tavily_body(request: &WebSearchRequest) -> Value {
    let mut body = Map::new();
    body.insert("query".to_string(), Value::String(request.query.clone()));
    if let Some(limit) = request.limit {
        body.insert("max_results".to_string(), json!(limit));
    }
    if let Some(domains) = &request.domains {
        body.insert("include_domains".to_string(), json!(domains));
    }
    if let Some(date_range) = request.date_range {
        body.insert(
            "start_date".to_string(),
            Value::String(date_range.start.format("%Y-%m-%d").to_string()),
        );
    }
    Value::Object(body)
}

pub(crate) fn exa_body(request: &WebSearchRequest) -> Value {
    let mut body = Map::new();
    body.insert("query".to_string(), Value::String(request.query.clone()));
    if let Some(limit) = request.limit {
        body.insert("numResults".to_string(), json!(limit));
    }
    if let Some(domains) = &request.domains {
        body.insert("includeDomains".to_string(), json!(domains));
    }
    if let Some(date_range) = request.date_range {
        body.insert(
            "startPublishedDate".to_string(),
            Value::String(format!(
                "{}T00:00:00.000Z",
                date_range.start.format("%Y-%m-%d")
            )),
        );
    }
    Value::Object(body)
}

pub(crate) fn jina_url(request: &WebSearchRequest) -> Result<Url, String> {
    if request.date_range.is_some() {
        return Err(
            "Jina search does not support recency; omit recency or configure another web search provider"
                .to_string(),
        );
    }

    let mut url =
        Url::parse("https://s.jina.ai/").map_err(|err| format!("invalid Jina URL: {err}"))?;
    let mut query = url.query_pairs_mut();
    query.append_pair("q", &request.query);
    if let Some(domains) = &request.domains {
        for domain in domains {
            query.append_pair("site", domain);
        }
    }
    if let Some(limit) = request.limit {
        query.append_pair("count", &limit.to_string());
    }
    drop(query);
    Ok(url)
}

pub(crate) fn brave_query(request: &WebSearchRequest) -> Vec<(&'static str, String)> {
    let mut query = vec![("q", query_with_domains(request))];
    if let Some(limit) = request.limit {
        query.push(("count", limit.to_string()));
    }
    if let Some(date_range) = request.date_range {
        query.push((
            "freshness",
            format!(
                "{}to{}",
                date_range.start.format("%Y-%m-%d"),
                date_range.end.format("%Y-%m-%d")
            ),
        ));
    }
    query
}

pub(crate) fn serpapi_query(
    request: &WebSearchRequest,
    api_key: &str,
) -> Result<Vec<(&'static str, String)>, String> {
    if request.limit.is_some() {
        return Err(
            "SerpAPI Google search does not support limit; omit limit or configure another web search provider"
                .to_string(),
        );
    }

    let mut query = vec![
        ("engine", "google".to_string()),
        ("q", query_with_domains(request)),
    ];
    if let Some(date_range) = request.date_range {
        query.push((
            "tbs",
            format!(
                "cdr:1,cd_min:{},cd_max:{}",
                date_range.start.format("%m/%d/%Y"),
                date_range.end.format("%m/%d/%Y")
            ),
        ));
    }
    query.push(("api_key", api_key.to_string()));
    Ok(query)
}

fn normalize_domains(domains: Option<Vec<String>>) -> Result<Option<Vec<String>>, String> {
    let Some(domains) = domains else {
        return Ok(None);
    };
    if domains.is_empty() {
        return Ok(None);
    }

    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(domains.len());
    for domain in domains {
        let domain = domain.trim();
        if domain.is_empty() {
            return Err("domains must contain non-empty hostnames".to_string());
        }
        let Host::Domain(domain) = Host::parse(domain)
            .map_err(|_| format!("invalid domain {domain:?}; use a hostname without a scheme"))?
        else {
            return Err(format!(
                "invalid domain {domain:?}; IP addresses are not supported"
            ));
        };
        if seen.insert(domain.clone()) {
            normalized.push(domain);
        }
    }

    Ok(Some(normalized))
}

fn query_with_domains(request: &WebSearchRequest) -> String {
    let Some(domains) = &request.domains else {
        return request.query.clone();
    };
    let sites = domains
        .iter()
        .map(|domain| format!("site:{domain}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    format!("{} ({sites})", request.query)
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
