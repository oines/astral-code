//! Width- and state-keyed cache for the flattened transcript layout.
//!
//! Grok Build keeps rendered entry output and layout measurements across
//! viewport movement. Astral's projection is still a flattened layout today,
//! so retaining that layout is the smallest equivalent invariant: scrolling
//! must not parse and wrap every historical Markdown block again.

use std::sync::Arc;

use crate::view::AstralTheme;
use crate::view::TranscriptLayout;

use super::TranscriptView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TranscriptCacheKey {
    pub(super) view: TranscriptView,
    pub(super) content_generation: u64,
    pub(super) display_revision: u64,
    pub(super) width: u16,
    pub(super) theme: AstralTheme,
}

#[derive(Debug)]
struct CachedTranscript {
    key: TranscriptCacheKey,
    layout: Arc<TranscriptLayout>,
}

#[derive(Debug, Default)]
pub(super) struct TranscriptCache {
    cached: Option<CachedTranscript>,
}

impl TranscriptCache {
    pub(super) fn get(&self, key: TranscriptCacheKey) -> Option<Arc<TranscriptLayout>> {
        self.cached
            .as_ref()
            .filter(|cached| cached.key == key)
            .map(|cached| Arc::clone(&cached.layout))
    }

    pub(super) fn store(
        &mut self,
        key: TranscriptCacheKey,
        layout: TranscriptLayout,
    ) -> Arc<TranscriptLayout> {
        let layout = Arc::new(layout);
        self.cached = Some(CachedTranscript {
            key,
            layout: Arc::clone(&layout),
        });
        layout
    }
}
