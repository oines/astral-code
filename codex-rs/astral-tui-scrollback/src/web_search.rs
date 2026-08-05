use std::borrow::Cow;

use codex_app_server_protocol::WebSearchAction;

use crate::EntryLifecycle;
use crate::block::lifecycle_elapsed_ms;

/// Exact renderer-facing view of one app-server web-search item.
///
/// Search results and citations are deliberately absent: the protocol exposes
/// only the action, its arguments, and the item lifecycle. The TUI therefore
/// never invents result counts or scrapes neighboring assistant text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WebSearchBlock<'a> {
    query: &'a str,
    action: Option<&'a WebSearchAction>,
    running: bool,
    elapsed_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WebSearchKind {
    Search,
    Fetch,
}

impl<'a> WebSearchBlock<'a> {
    pub(crate) fn from_parts(
        query: &'a str,
        action: Option<&'a WebSearchAction>,
        lifecycle: EntryLifecycle,
    ) -> Self {
        Self {
            query,
            action,
            running: matches!(lifecycle, EntryLifecycle::Running { .. }),
            elapsed_ms: lifecycle_elapsed_ms(lifecycle),
        }
    }

    pub fn label(self) -> &'static str {
        match self.action {
            Some(WebSearchAction::OpenPage { .. }) => "Fetch",
            Some(WebSearchAction::FindInPage { .. }) => "Find",
            Some(WebSearchAction::Search { .. }) | Some(WebSearchAction::Other) | None => {
                "Web Search"
            }
        }
    }

    pub fn detail(self) -> Cow<'a, str> {
        let detail = match self.action {
            Some(WebSearchAction::Search { query, queries }) => search_detail(query, queries),
            Some(WebSearchAction::OpenPage { url }) => option_text(url),
            Some(WebSearchAction::FindInPage { url, pattern }) => match (pattern, url) {
                (Some(pattern), Some(url)) => Cow::Owned(format!("{pattern:?} in {url}")),
                (Some(pattern), None) => Cow::Owned(format!("{pattern:?}")),
                (None, Some(url)) => Cow::Borrowed(url.as_str()),
                (None, None) => Cow::Borrowed(""),
            },
            Some(WebSearchAction::Other) | None => Cow::Borrowed(""),
        };
        if detail.is_empty() {
            Cow::Borrowed(self.query)
        } else {
            detail
        }
    }

    pub fn running(self) -> bool {
        self.running
    }

    pub fn elapsed_ms(self) -> Option<i64> {
        self.elapsed_ms
    }

    pub(crate) fn kind(self) -> WebSearchKind {
        match self.action {
            Some(WebSearchAction::OpenPage { .. } | WebSearchAction::FindInPage { .. }) => {
                WebSearchKind::Fetch
            }
            Some(WebSearchAction::Search { .. } | WebSearchAction::Other) | None => {
                WebSearchKind::Search
            }
        }
    }

    pub(crate) fn query_count(self) -> usize {
        match self.action {
            Some(WebSearchAction::Search { query, queries }) => {
                if query.as_deref().is_some_and(|query| !query.is_empty()) {
                    1
                } else {
                    queries
                        .as_ref()
                        .map(|queries| queries.iter().filter(|query| !query.is_empty()).count())
                        .filter(|count| *count > 0)
                        .unwrap_or(1)
                }
            }
            Some(WebSearchAction::OpenPage { .. })
            | Some(WebSearchAction::FindInPage { .. })
            | Some(WebSearchAction::Other)
            | None => 1,
        }
    }
}

fn search_detail<'a>(query: &'a Option<String>, queries: &'a Option<Vec<String>>) -> Cow<'a, str> {
    if let Some(query) = query.as_deref().filter(|query| !query.is_empty()) {
        return Cow::Borrowed(query);
    }
    let Some(first) = queries.as_ref().and_then(|queries| queries.first()) else {
        return Cow::Borrowed("");
    };
    if queries.as_ref().is_some_and(|queries| queries.len() > 1) && !first.is_empty() {
        Cow::Owned(format!("{first} ..."))
    } else {
        Cow::Borrowed(first)
    }
}

fn option_text(value: &Option<String>) -> Cow<'_, str> {
    Cow::Borrowed(value.as_deref().unwrap_or(""))
}
