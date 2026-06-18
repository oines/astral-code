use std::net::IpAddr;
use std::sync::OnceLock;

use regex::Regex;
use reqwest::StatusCode;
use reqwest::header::CONTENT_LENGTH;
use reqwest::header::CONTENT_TYPE;
use reqwest::header::HeaderMap;
use reqwest::header::LOCATION;
use schemars::JsonSchema;
use serde::Deserialize;
use url::Host;
use url::Url;

const MAX_FETCH_BYTES: usize = 5 * 1024 * 1024;
const MAX_FETCH_CHARS: usize = 40_000;
const MAX_REDIRECTS: usize = 5;

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum WebFetchFormat {
    #[default]
    Markdown,
    Text,
}

impl std::fmt::Display for WebFetchFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Markdown => f.write_str("markdown"),
            Self::Text => f.write_str("text"),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct WebFetchInput {
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) format: WebFetchFormat,
}

pub(crate) async fn fetch(
    client: &reqwest::Client,
    input: WebFetchInput,
) -> Result<String, String> {
    let original_url = validate_fetch_url(&input.url)?;
    let response = fetch_with_redirects(client, original_url.clone()).await?;
    let content_type = header_value(response.headers(), CONTENT_TYPE.as_str())
        .unwrap_or_else(|| "unknown".to_string());
    let final_url = response.url().to_string();
    let (bytes, download_truncated) = read_limited(response).await?;
    let body = String::from_utf8_lossy(&bytes);
    let (content, output_truncated) = readable_content(&body, &content_type, input.format);
    let truncated = download_truncated || output_truncated;

    Ok(format!(
        "URL: {original_url}\nFinal URL: {final_url}\nContent-Type: {content_type}\nFormat: {}\nTruncated: {truncated}\nContent:\n{content}",
        input.format
    ))
}

async fn fetch_with_redirects(
    client: &reqwest::Client,
    mut url: Url,
) -> Result<reqwest::Response, String> {
    for _ in 0..=MAX_REDIRECTS {
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|err| format!("fetch request failed for {url}: {err}"))?;

        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| format!("redirect from {url} did not include a Location header"))?;
            let next_url = url
                .join(location)
                .map_err(|err| format!("invalid redirect from {url}: {err}"))?;
            validate_fetch_url(next_url.as_str())?;
            url = next_url;
            continue;
        }

        if !response.status().is_success() {
            return Err(status_error(response.status(), response.url().as_str()));
        }

        return Ok(response);
    }

    Err(format!("fetch exceeded {MAX_REDIRECTS} redirects"))
}

async fn read_limited(mut response: reqwest::Response) -> Result<(Vec<u8>, bool), String> {
    if let Some(content_length) = response.content_length()
        && content_length > MAX_FETCH_BYTES as u64
    {
        return Err(format!(
            "response is too large: Content-Length {content_length} exceeds {MAX_FETCH_BYTES} bytes"
        ));
    }

    if let Some(content_length) = header_value(response.headers(), CONTENT_LENGTH.as_str())
        && let Ok(content_length) = content_length.parse::<usize>()
        && content_length > MAX_FETCH_BYTES
    {
        return Err(format!(
            "response is too large: Content-Length {content_length} exceeds {MAX_FETCH_BYTES} bytes"
        ));
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("failed reading response body: {err}"))?
    {
        if bytes.len() + chunk.len() > MAX_FETCH_BYTES {
            let remaining = MAX_FETCH_BYTES.saturating_sub(bytes.len());
            bytes.extend_from_slice(&chunk[..remaining]);
            return Ok((bytes, true));
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok((bytes, false))
}

fn status_error(status: StatusCode, url: &str) -> String {
    format!("fetch returned HTTP {status} for {url}")
}

fn readable_content(body: &str, content_type: &str, format: WebFetchFormat) -> (String, bool) {
    let mut content = if is_html(content_type, body) {
        let cleaned_html = clean_html_before_conversion(body);
        let markdown = html_to_markdown(&cleaned_html);
        match format {
            WebFetchFormat::Markdown => markdown,
            WebFetchFormat::Text => markdown_to_text(&markdown),
        }
    } else {
        match format {
            WebFetchFormat::Markdown => sanitize_text(body),
            WebFetchFormat::Text => sanitize_text(body),
        }
    };

    content = post_process_text(&content);
    truncate_content(&content, MAX_FETCH_CHARS)
}

fn validate_fetch_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|err| format!("invalid URL: {err}"))?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => return Err(format!("unsupported URL scheme `{scheme}`")),
    }

    let Some(host) = url.host() else {
        return Err("URL must include a host".to_string());
    };
    match host {
        Host::Domain(domain) if is_local_domain(domain) => {
            return Err(format!("refusing to fetch local host `{domain}`"));
        }
        Host::Ipv4(ip) if is_blocked_ip(IpAddr::V4(ip)) => {
            return Err(format!("refusing to fetch private address `{ip}`"));
        }
        Host::Ipv6(ip) if is_blocked_ip(IpAddr::V6(ip)) => {
            return Err(format!("refusing to fetch private address `{ip}`"));
        }
        Host::Domain(_) | Host::Ipv4(_) | Host::Ipv6(_) => {}
    }

    Ok(url)
}

fn is_local_domain(domain: &str) -> bool {
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    domain == "localhost" || domain.ends_with(".localhost")
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
        }
    }
}

fn is_html(content_type: &str, body: &str) -> bool {
    content_type.split(';').next().is_some_and(|content_type| {
        matches!(
            content_type.trim().to_ascii_lowercase().as_str(),
            "text/html" | "application/xhtml+xml"
        )
    }) || body
        .get(..body.len().min(512))
        .is_some_and(|prefix| prefix.to_ascii_lowercase().contains("<html"))
}

fn clean_html_before_conversion(html: &str) -> String {
    let mut cleaned = html.to_string();
    for regex in [
        html_comment_re(),
        html_noise_block_re(),
        html_noise_container_re(),
        html_image_tag_re(),
        data_uri_re(),
    ] {
        cleaned = regex.replace_all(&cleaned, " ").into_owned();
    }
    cleaned
}

fn html_to_markdown(html: &str) -> String {
    match html_to_markdown_rs::convert(html, None) {
        Ok(result) => result.content.unwrap_or_default(),
        Err(_) => strip_html_tags(html),
    }
}

fn markdown_to_text(markdown: &str) -> String {
    let without_images = markdown_image_re().replace_all(markdown, " ");
    let without_links = markdown_link_re().replace_all(&without_images, "$text");
    without_links
        .lines()
        .map(|line| {
            line.trim_start_matches('#')
                .trim_start_matches('>')
                .trim_start_matches("- ")
                .trim_start_matches("* ")
                .replace(['`', '*', '_'], "")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sanitize_text(text: &str) -> String {
    data_uri_re()
        .replace_all(text, " [removed data URI] ")
        .into_owned()
}

fn post_process_text(text: &str) -> String {
    let without_data = sanitize_text(text);
    let mut output = Vec::new();
    let mut previous_line = String::new();
    let mut repeated_line_count = 0usize;
    let mut previous_blank = false;

    for line in without_data.lines() {
        let cleaned_line = clean_long_tokens(line).trim().to_string();
        if cleaned_line.is_empty() {
            if !previous_blank {
                output.push(String::new());
            }
            previous_blank = true;
            continue;
        }
        previous_blank = false;

        if cleaned_line == previous_line {
            repeated_line_count += 1;
            if repeated_line_count >= 3 {
                continue;
            }
        } else {
            previous_line.clone_from(&cleaned_line);
            repeated_line_count = 0;
        }

        output.push(cleaned_line);
    }

    output.join("\n").trim().to_string()
}

fn clean_long_tokens(line: &str) -> String {
    line.split_whitespace()
        .map(|token| {
            let char_count = token.chars().count();
            if char_count <= 500 {
                token.to_string()
            } else if token.starts_with("http://") || token.starts_with("https://") {
                "[removed long URL]".to_string()
            } else if base64_like_token_re().is_match(token) {
                "[removed long encoded data]".to_string()
            } else {
                "[removed long token]".to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_content(content: &str, max_chars: usize) -> (String, bool) {
    let mut iter = content.char_indices();
    let Some((index, _)) = iter.nth(max_chars) else {
        return (content.to_string(), false);
    };
    let mut truncated = content[..index].trim_end().to_string();
    truncated.push_str(&format!(
        "\n\n[Content truncated after {max_chars} characters.]"
    ));
    (truncated, true)
}

fn strip_html_tags(html: &str) -> String {
    html_tag_re().replace_all(html, " ").into_owned()
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn html_comment_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| static_regex(r"(?is)<!--.*?-->", "html comment"))
}

fn html_noise_block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?is)<(?:script|style|noscript|svg|canvas|iframe|template|form|nav|footer|header)\b[^>]*>.*?</(?:script|style|noscript|svg|canvas|iframe|template|form|nav|footer|header)\s*>",
        )
        .unwrap_or_else(|error| panic!("invalid html noise block regex: {error}"))
    })
}

fn html_noise_container_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?is)<(?:div|aside|section)\b[^>]*(?:id|class)\s*=\s*["'][^"']*(?:advert|ads?|promo|cookie|banner|tracking|subscribe|newsletter|sidebar|social|share|sponsor)[^"']*["'][^>]*>.*?</(?:div|aside|section)\s*>"#,
        )
        .unwrap_or_else(|error| panic!("invalid html noise container regex: {error}"))
    })
}

fn html_image_tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| static_regex(r"(?is)<(?:img|picture|source)\b[^>]*>", "html image tag"))
}

fn html_tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| static_regex(r"(?is)<[^>]+>", "html tag"))
}

fn data_uri_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        static_regex(
            r"(?is)data:[a-z0-9.+/-]+/[a-z0-9.+-]+;base64,[a-z0-9+/=._%-]{80,}",
            "data URI",
        )
    })
}

fn base64_like_token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| static_regex(r"^[A-Za-z0-9+/]{500,}={0,2}$", "base64 token"))
}

fn markdown_image_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| static_regex(r"!\[[^\]]*\]\([^)]*\)", "markdown image"))
}

fn markdown_link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| static_regex(r"\[(?P<text>[^\]]+)\]\([^)]*\)", "markdown link"))
}

fn static_regex(pattern: &str, name: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|error| panic!("invalid {name} regex: {error}"))
}

#[cfg(test)]
#[path = "fetch_tests.rs"]
mod tests;
