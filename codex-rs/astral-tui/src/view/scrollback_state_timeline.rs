//! Prompt-backed turn navigation for the fullscreen transcript.
//!
//! The timeline uses measured prompt rows, not scroll percentages. This keeps
//! the highlighted tick, chevrons, keyboard navigation, and mouse jumps on the
//! same semantic turn boundaries.

use crate::PresentationBlock;
use crate::conversation::TranscriptTurn;
use crate::timeline_rail::RailViewport;

use super::ScrollbackNavigation;
use super::ScrollbackState;

const PREVIEW_MAX_CHARS: usize = 120;

#[derive(Debug, Default)]
pub(super) struct TimelineState {
    entries: Vec<TimelineEntry>,
}

#[derive(Debug)]
struct TimelineEntry {
    prompt_id: String,
    preview: String,
}

impl TimelineState {
    pub(super) fn observe(&mut self, turns: &[TranscriptTurn]) {
        self.entries = turns.iter().filter_map(timeline_entry).collect();
    }

    fn index_for_entry(&self, entry_id: &str) -> Option<usize> {
        let turn_id = turn_id_from_entry(entry_id)?;
        self.entries
            .iter()
            .position(|entry| turn_id_from_entry(&entry.prompt_id) == Some(turn_id))
    }

    fn prompts_above_top(&self, navigation: &ScrollbackNavigation, strict: bool) -> usize {
        let viewport_top = navigation.viewport().first_visible_line;
        self.entries.partition_point(|entry| {
            navigation
                .entry_top(&entry.prompt_id)
                .is_some_and(|prompt_top| {
                    if strict {
                        prompt_top < viewport_top
                    } else {
                        prompt_top <= viewport_top
                    }
                })
        })
    }
}

impl ScrollbackState {
    pub(crate) fn next_turn(&mut self) {
        let current = self
            .display
            .selected_id()
            .and_then(|selected| self.timeline.index_for_entry(selected));
        let target = current
            .map(|index| (index + 1).min(self.timeline.entries.len().saturating_sub(1)))
            .unwrap_or_default();
        let prompt = self
            .timeline
            .entries
            .get(target)
            .map(|entry| entry.prompt_id.clone());
        self.select_and_snap(prompt);
    }

    pub(crate) fn previous_turn(&mut self) {
        let selected = self.display.selected_id();
        let current = selected.and_then(|selected| self.timeline.index_for_entry(selected));
        let target = current.and_then(|index| {
            let prompt = self.timeline.entries.get(index)?;
            if selected == Some(prompt.prompt_id.as_str()) {
                index.checked_sub(1)
            } else {
                Some(index)
            }
        });
        let prompt = target
            .and_then(|index| self.timeline.entries.get(index))
            .map(|entry| entry.prompt_id.clone());
        self.select_and_snap(prompt);
    }

    pub(crate) fn timeline_viewport(&self) -> RailViewport {
        let at_or_above = self
            .timeline
            .prompts_above_top(&self.navigation, /* strict */ false);
        let strictly_above = self
            .timeline
            .prompts_above_top(&self.navigation, /* strict */ true);
        let turn_count = self.timeline.entries.len();
        RailViewport {
            active: (turn_count > 0).then_some(at_or_above.saturating_sub(1)),
            up_target: strictly_above.checked_sub(1),
            down_target: (at_or_above < turn_count).then_some(at_or_above),
            at_bottom: !self.navigation.viewport().has_content_below,
        }
    }

    pub(crate) fn timeline_preview(&self, turn_index: usize) -> Option<&str> {
        self.timeline
            .entries
            .get(turn_index)
            .map(|entry| entry.preview.as_str())
    }

    pub(crate) fn jump_to_turn(&mut self, turn_index: usize) -> bool {
        let Some(prompt) = self
            .timeline
            .entries
            .get(turn_index)
            .map(|entry| entry.prompt_id.clone())
        else {
            return false;
        };
        self.select_and_snap(Some(prompt));
        true
    }
}

fn timeline_entry(turn: &TranscriptTurn) -> Option<TimelineEntry> {
    let block = turn
        .blocks
        .iter()
        .find(|block| matches!(&block.block, PresentationBlock::User { .. }))?;
    let preview = match &block.block {
        PresentationBlock::User { text, .. } => prompt_preview(text),
        PresentationBlock::Assistant { .. }
        | PresentationBlock::Thinking { .. }
        | PresentationBlock::Plan { .. }
        | PresentationBlock::Todo(_)
        | PresentationBlock::Tool(_)
        | PresentationBlock::Subagent(_)
        | PresentationBlock::System { .. } => String::new(),
    };
    Some(TimelineEntry {
        prompt_id: super::super::entry_state::entry_id(&turn.id, &block.item_id),
        preview,
    })
}

fn prompt_preview(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let mut preview = line.chars().take(PREVIEW_MAX_CHARS).collect::<String>();
    if preview.chars().count() == PREVIEW_MAX_CHARS && line.chars().nth(PREVIEW_MAX_CHARS).is_some()
    {
        preview.pop();
        preview.push('…');
    }
    preview
}

fn turn_id_from_entry(entry_id: &str) -> Option<&str> {
    entry_id.split_once('\0').map(|(turn_id, _)| turn_id)
}
