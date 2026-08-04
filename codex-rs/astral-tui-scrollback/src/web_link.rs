//! Shared detection and validation for plain HTTP(S) links.
//!
//! Markdown links already carry their destination through the parser. Models
//! also emit bare URLs frequently, so both the semantic Surface renderer and
//! terminal-native rows use this one grammar instead of disagreeing about
//! which text is clickable.

use std::ops::Range;

use url::Url;

/// One validated bare HTTP(S) URL found in source text.
///
/// The byte range addresses the original UTF-8 string; renderers can convert
/// it to display columns only after applying their own wrapping rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebLinkMatch {
    byte_range: Range<usize>,
    destination: String,
}

impl WebLinkMatch {
    /// Byte range occupied by the visible URL in the source string.
    pub fn byte_range(&self) -> Range<usize> {
        self.byte_range.clone()
    }

    /// Sanitized HTTP(S) destination used for terminal hyperlink metadata.
    pub fn destination(&self) -> &str {
        &self.destination
    }
}

/// Find bare HTTP(S) URLs without interpreting Markdown link syntax.
pub fn find_web_links(text: &str) -> Vec<WebLinkMatch> {
    let mut links = Vec::new();
    let mut search_from = 0usize;
    for raw_token in text.split_ascii_whitespace() {
        let Some(relative_start) = text[search_from..].find(raw_token) else {
            continue;
        };
        let raw_start = search_from + relative_start;
        search_from = raw_start + raw_token.len();
        let trimmed_start = raw_token
            .find(|ch: char| !is_leading_punctuation(ch))
            .unwrap_or(raw_token.len());
        let trimmed_end = trailing_url_end(&raw_token[trimmed_start..]) + trimmed_start;
        if trimmed_start >= trimmed_end {
            continue;
        }
        let byte_range = raw_start + trimmed_start..raw_start + trimmed_end;
        let Some(destination) = normalize_web_destination(&text[byte_range.clone()]) else {
            continue;
        };
        links.push(WebLinkMatch {
            byte_range,
            destination,
        });
    }
    links
}

/// Sanitize and validate one terminal hyperlink destination.
pub fn normalize_web_destination(destination: &str) -> Option<String> {
    let safe_destination = destination
        .chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>();
    let parsed = Url::parse(&safe_destination).ok()?;
    matches!(parsed.scheme(), "http" | "https")
        .then(|| parsed.host_str())
        .flatten()?;
    Some(safe_destination)
}

fn is_leading_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | '.' | ';' | '!' | '\'' | '"'
    )
}

fn trailing_url_end(candidate: &str) -> usize {
    let mut end = candidate.len();
    while end > 0 {
        let remaining = &candidate[..end];
        let Some(ch) = remaining.chars().next_back() else {
            break;
        };
        let trim = matches!(ch, ',' | '.' | ';' | '!' | '\'' | '"')
            || matches!(ch, ')' | ']' | '}' | '>')
                && has_unmatched_closing_delimiter(remaining, ch);
        if !trim {
            break;
        }
        end -= ch.len_utf8();
    }
    end
}

fn has_unmatched_closing_delimiter(candidate: &str, closing: char) -> bool {
    let opening = match closing {
        ')' => '(',
        ']' => '[',
        '}' => '{',
        '>' => '<',
        _ => return false,
    };
    candidate.chars().filter(|ch| *ch == closing).count()
        > candidate.chars().filter(|ch| *ch == opening).count()
}
