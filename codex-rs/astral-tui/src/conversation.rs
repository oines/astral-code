use std::collections::HashMap;
use std::collections::HashSet;

use astral_tui_scrollback::ApplyOutcome;
use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::EntryBlock;
use astral_tui_scrollback::EntryDisplayState;
use astral_tui_scrollback::Transcript;
use astral_tui_scrollback::TranscriptEntryId;
use astral_tui_scrollback::VerbGroupDisplayState;
use astral_tui_scrollback::VerbGroupSpan;
use astral_tui_scrollback::project_verb_groups;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Thread;

/// Presentation-only action for one canonical transcript entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryDisplayAction {
    ToggleFold,
    Collapse,
    Expand,
    ToggleRaw,
    Reset,
}

/// Presentation-only action for one Grok-style verb group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbGroupDisplayAction {
    Toggle,
    Collapse,
    Expand,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TurnPresentationState {
    groups: Vec<VerbGroupSpan>,
    display: VerbGroupDisplayState,
    unstable_suffix_start: Option<usize>,
}

/// TUI-owned interaction state over the canonical app-server transcript.
///
/// Protocol items remain in [`Transcript`]. This layer owns only fold/raw
/// choices and derived verb groups, keyed by stable local entry identities.
/// Server requests such as approvals and Ask User deliberately do not enter
/// this state; the runtime routes them to the modal interaction layer.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationState {
    transcript: Transcript,
    entry_display: HashMap<TranscriptEntryId, EntryDisplayState>,
    turn_presentation: HashMap<String, TurnPresentationState>,
}

impl ConversationState {
    pub fn from_thread(thread: &Thread) -> Self {
        let mut state = Self {
            transcript: Transcript::from_thread(thread),
            entry_display: HashMap::new(),
            turn_presentation: HashMap::new(),
        };
        state.reconcile_presentation();
        state
    }

    pub fn transcript(&self) -> &Transcript {
        &self.transcript
    }

    /// Rehydrate from an authoritative snapshot without discarding interaction
    /// state for entries that still have the same thread/turn/item identity.
    pub fn reset_from_thread(&mut self, thread: &Thread) {
        if self.transcript.thread_id() != thread.id {
            self.entry_display.clear();
            self.turn_presentation.clear();
        }
        self.transcript.reset_from_thread(thread);
        self.reconcile_presentation();
    }

    /// Apply one notification to the transcript and reconcile derived display
    /// state only when canonical transcript state changed.
    pub fn apply(&mut self, notification: &ServerNotification) -> ApplyOutcome {
        let outcome = self.transcript.apply(notification);
        if outcome == ApplyOutcome::Applied {
            self.reconcile_presentation();
        }
        outcome
    }

    pub fn entry_display_state(&self, entry_id: TranscriptEntryId) -> Option<EntryDisplayState> {
        self.entry_display.get(&entry_id).copied()
    }

    pub fn apply_entry_display_action(
        &mut self,
        entry_id: TranscriptEntryId,
        action: EntryDisplayAction,
    ) -> bool {
        let changed = {
            let Some(entry) = self
                .transcript
                .turns()
                .iter()
                .flat_map(astral_tui_scrollback::TranscriptTurn::entries)
                .find(|entry| entry.id() == entry_id)
            else {
                return false;
            };
            let block = EntryBlock::from_entry(entry);
            let Some(state) = self.entry_display.get_mut(&entry_id) else {
                return false;
            };
            match action {
                EntryDisplayAction::ToggleFold => state.toggle_fold(&block),
                EntryDisplayAction::Collapse => state.collapse(&block),
                EntryDisplayAction::Expand => state.expand(&block),
                EntryDisplayAction::ToggleRaw => state.toggle_raw(&block),
                EntryDisplayAction::Reset => state.reset(&block),
            }
        };
        if changed {
            self.reconcile_verb_groups();
        }
        changed
    }

    pub fn verb_groups(&self, turn_id: &str) -> &[VerbGroupSpan] {
        self.turn_presentation
            .get(turn_id)
            .map_or(&[], |state| state.groups.as_slice())
    }

    pub fn verb_group_mode(&self, turn_id: &str, group: &VerbGroupSpan) -> Option<DisplayMode> {
        self.turn_presentation
            .get(turn_id)
            .map(|state| state.display.mode(group))
    }

    /// First source entry whose view-time grouping may still change when the
    /// running turn appends more items.
    pub fn unstable_group_suffix_start(&self, turn_id: &str) -> Option<usize> {
        self.turn_presentation
            .get(turn_id)
            .and_then(|state| state.unstable_suffix_start)
    }

    pub fn apply_verb_group_display_action(
        &mut self,
        turn_id: &str,
        anchor: TranscriptEntryId,
        action: VerbGroupDisplayAction,
    ) -> Option<DisplayMode> {
        let state = self.turn_presentation.get_mut(turn_id)?;
        let group = state
            .groups
            .iter()
            .find(|group| group.anchor() == anchor)?
            .clone();
        match action {
            VerbGroupDisplayAction::Toggle => {
                state.display.toggle(&group);
            }
            VerbGroupDisplayAction::Collapse => {
                state.display.collapse(&group);
            }
            VerbGroupDisplayAction::Expand => {
                state.display.expand(&group);
            }
        }
        Some(state.display.mode(&group))
    }

    fn reconcile_presentation(&mut self) {
        let mut active_entries = HashSet::new();
        for entry in self
            .transcript
            .turns()
            .iter()
            .flat_map(astral_tui_scrollback::TranscriptTurn::entries)
        {
            let block = EntryBlock::from_entry(entry);
            let Some(default_state) = EntryDisplayState::for_block(&block) else {
                continue;
            };
            active_entries.insert(entry.id());
            if let Some(state) = self.entry_display.get_mut(&entry.id()) {
                state.reconcile(&block);
            } else {
                self.entry_display.insert(entry.id(), default_state);
            }
        }
        self.entry_display
            .retain(|entry_id, _| active_entries.contains(entry_id));
        self.reconcile_verb_groups();
    }

    fn reconcile_verb_groups(&mut self) {
        let mut active_turns = HashSet::new();
        for turn in self.transcript.turns() {
            active_turns.insert(turn.id().to_string());
            let (groups, unstable_suffix_start) =
                project_verb_groups(turn, |entry| self.entry_display.get(&entry.id()).copied())
                    .into_parts();
            let state = self
                .turn_presentation
                .entry(turn.id().to_string())
                .or_default();
            state.display.reconcile(&state.groups, &groups);
            state.groups = groups;
            state.unstable_suffix_start = unstable_suffix_start;
        }
        self.turn_presentation
            .retain(|turn_id, _| active_turns.contains(turn_id));
    }
}

#[cfg(test)]
#[path = "conversation_tests.rs"]
mod tests;
