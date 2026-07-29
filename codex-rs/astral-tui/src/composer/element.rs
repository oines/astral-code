//! Structured spans embedded in the editable prompt.
//!
//! Grok Build models paste, file, image, and mention chips as one family of
//! atomic textarea elements. Astral keeps the same view-state invariant while
//! retaining its own app-server `UserInput` semantics.

use std::ops::Range;

use crate::mention::MentionTarget;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComposerElementKind {
    Mention(MentionTarget),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComposerElement {
    pub(crate) range: Range<usize>,
    pub(crate) insert_text: String,
    pub(crate) kind: ComposerElementKind,
}

impl ComposerElement {
    pub(crate) fn mention(range: Range<usize>, insert_text: String, target: MentionTarget) -> Self {
        Self {
            range,
            insert_text,
            kind: ComposerElementKind::Mention(target),
        }
    }

    pub(crate) fn matches_text(&self, text: &str) -> bool {
        text.get(self.range.clone()) == Some(self.insert_text.as_str())
            && (self.range.start == 0
                || text[..self.range.start]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace))
            && (self.range.end == text.len()
                || text[self.range.end..]
                    .chars()
                    .next()
                    .is_some_and(char::is_whitespace))
    }

    pub(crate) fn mention_target(&self) -> &MentionTarget {
        let ComposerElementKind::Mention(target) = &self.kind;
        target
    }
}
