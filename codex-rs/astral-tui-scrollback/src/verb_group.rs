//! View-time verb grouping derived from Grok Build's `verb_group` model at
//! commit `47348d13ec4508dcfe440e34c6d511bb02998fb2` (Apache-2.0).

use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;

use codex_app_server_protocol::CoreToolCallStatus;
use codex_app_server_protocol::ThreadItem;

use crate::DisplayMode;
use crate::EntryBlock;
use crate::EntryDisplayState;
use crate::TranscriptEntry;
use crate::TranscriptEntryId;
use crate::TranscriptTurn;
use crate::read_tool::ReadCall;
use crate::search_tool::SearchCall;

/// One synthetic fold over consecutive, non-destructive transcript entries.
///
/// The source entries remain untouched and ordered. `claimed` contains only
/// entries replaced by the header while collapsed; transparent entries inside
/// `range` (for example an opened Thought) continue to render normally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerbGroupSpan {
    anchor: TranscriptEntryId,
    range: Range<usize>,
    claimed: Vec<usize>,
    members: usize,
    label: String,
    running: bool,
    failed: bool,
}

impl VerbGroupSpan {
    pub fn anchor(&self) -> TranscriptEntryId {
        self.anchor
    }

    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    pub fn claimed(&self) -> &[usize] {
        &self.claimed
    }

    pub fn members(&self) -> usize {
        self.members
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    pub fn contains_member(&self, index: usize) -> bool {
        self.claimed.contains(&index)
    }
}

/// Expansion state shared by keyboard and mouse handlers.
///
/// Group state is keyed by the stable local id of the run's first entry and
/// never mutates the source transcript or any member's own display state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerbGroupDisplayState {
    expanded: HashSet<TranscriptEntryId>,
}

impl VerbGroupDisplayState {
    pub fn mode(&self, group: &VerbGroupSpan) -> DisplayMode {
        if self.expanded.contains(&group.anchor) {
            DisplayMode::Expanded
        } else {
            DisplayMode::Collapsed
        }
    }

    pub fn toggle(&mut self, group: &VerbGroupSpan) -> DisplayMode {
        if !self.expanded.remove(&group.anchor) {
            self.expanded.insert(group.anchor);
        }
        self.mode(group)
    }

    pub fn expand(&mut self, group: &VerbGroupSpan) -> bool {
        self.expanded.insert(group.anchor)
    }

    pub fn collapse(&mut self, group: &VerbGroupSpan) -> bool {
        self.expanded.remove(&group.anchor)
    }

    pub fn hides(&self, group: &VerbGroupSpan, index: usize) -> bool {
        self.mode(group) == DisplayMode::Collapsed && group.contains_member(index)
    }

    pub fn retain(&mut self, groups: &[VerbGroupSpan]) {
        let anchors = groups
            .iter()
            .map(VerbGroupSpan::anchor)
            .collect::<HashSet<_>>();
        self.expanded.retain(|anchor| anchors.contains(anchor));
    }

    /// Preserve expansion when opening a member moves the run's anchor.
    ///
    /// Both slices must describe the same turn before and after one entry
    /// display-state change. The most-overlapping successor inherits the old
    /// expansion key, matching Grok's rekey invariant without index-keyed UI
    /// state leaking into the transcript.
    pub fn reconcile(&mut self, previous: &[VerbGroupSpan], current: &[VerbGroupSpan]) -> bool {
        let current_anchors = current
            .iter()
            .map(VerbGroupSpan::anchor)
            .collect::<HashSet<_>>();
        let migrations = previous
            .iter()
            .filter(|old| {
                self.expanded.contains(&old.anchor) && !current_anchors.contains(&old.anchor)
            })
            .filter_map(|old| {
                let next = current
                    .iter()
                    .filter(|next| ranges_overlap(&old.range, &next.range))
                    .max_by_key(|next| overlap_len(&old.range, &next.range))?;
                Some((old.anchor, next.anchor))
            })
            .collect::<Vec<_>>();
        let before = self.expanded.clone();
        for (old, next) in migrations {
            self.expanded.remove(&old);
            self.expanded.insert(next);
        }
        self.expanded
            .retain(|anchor| current_anchors.contains(anchor));
        self.expanded != before
    }
}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

fn overlap_len(left: &Range<usize>, right: &Range<usize>) -> usize {
    left.end
        .min(right.end)
        .saturating_sub(left.start.max(right.start))
}

/// Find maximal Grok-style verb runs in one authoritative app-server turn.
///
/// `display_state` is deliberately supplied by the caller: per-entry fold
/// choices belong to the owning TUI, while this function remains a pure view
/// projection over the preserved transcript order.
pub fn scan_verb_groups(
    turn: &TranscriptTurn,
    display_state: impl Fn(&TranscriptEntry) -> Option<EntryDisplayState>,
) -> Vec<VerbGroupSpan> {
    let entries = turn.entries();
    let mut spans = Vec::new();
    let mut index = 0;
    while index < entries.len() {
        let Some(scan) = scan_run(entries, index, &display_state) else {
            index += 1;
            continue;
        };
        if scan.members == 0 {
            index = scan.stop;
            continue;
        }
        let label = aggregate_label(entries, &scan.claimed);
        spans.push(VerbGroupSpan {
            anchor: entries[index].id(),
            range: index..scan.end,
            claimed: scan.claimed,
            members: scan.members,
            label: label.text,
            running: label.running,
            failed: label.failed_count > 0,
        });
        index = scan.end;
    }
    spans
}

struct RunScan {
    members: usize,
    end: usize,
    stop: usize,
    claimed: Vec<usize>,
}

fn scan_run(
    entries: &[TranscriptEntry],
    start: usize,
    display_state: &impl Fn(&TranscriptEntry) -> Option<EntryDisplayState>,
) -> Option<RunScan> {
    match run_step(entries.get(start)?, display_state) {
        RunStep::Member(_) | RunStep::ThoughtMember => {}
        RunStep::Transparent | RunStep::Break => return None,
    }
    let mut members = 0;
    let mut end = start;
    let mut claimed = Vec::new();
    let mut index = start;
    while let Some(entry) = entries.get(index) {
        match run_step(entry, display_state) {
            RunStep::Member(_) => {
                members += 1;
                claimed.push(index);
                end = index + 1;
            }
            RunStep::ThoughtMember => {
                claimed.push(index);
                end = index + 1;
            }
            RunStep::Transparent => {}
            RunStep::Break => break,
        }
        index += 1;
    }
    Some(RunScan {
        members,
        end,
        stop: index,
        claimed,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunStep {
    Member(GroupBucket),
    ThoughtMember,
    Transparent,
    Break,
}

fn run_step(
    entry: &TranscriptEntry,
    display_state: &impl Fn(&TranscriptEntry) -> Option<EntryDisplayState>,
) -> RunStep {
    let block = EntryBlock::from_entry(entry);
    let Some(state) = display_state(entry) else {
        return RunStep::Break;
    };
    match &block {
        EntryBlock::Reasoning(reasoning) => {
            if reasoning.running() || state.mode() != DisplayMode::Collapsed {
                RunStep::Transparent
            } else {
                RunStep::ThoughtMember
            }
        }
        EntryBlock::ProtocolItem { item, .. } => {
            let Some((kind, _)) = member_kind_and_status(item) else {
                return RunStep::Break;
            };
            if state.mode() == DisplayMode::Collapsed {
                RunStep::Member(kind)
            } else {
                RunStep::Transparent
            }
        }
        EntryBlock::User { .. }
        | EntryBlock::Assistant { .. }
        | EntryBlock::ProposedPlan { .. } => RunStep::Break,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupBucket {
    File,
    Skill,
    Search,
}

impl GroupBucket {
    fn verb(self, running: bool) -> &'static str {
        match (self, running) {
            (Self::File | Self::Skill, false) => "Read",
            (Self::File | Self::Skill, true) => "Reading",
            (Self::Search, false) => "Searched",
            (Self::Search, true) => "Searching",
        }
    }

    fn noun(self, count: usize) -> &'static str {
        match (self, count) {
            (Self::File, 1) => "file",
            (Self::File, _) => "files",
            (Self::Skill, 1) => "skill",
            (Self::Skill, _) => "skills",
            (Self::Search, 1) => "pattern",
            (Self::Search, _) => "patterns",
        }
    }
}

fn member_kind_and_status(item: &ThreadItem) -> Option<(GroupBucket, CoreToolCallStatus)> {
    if let Some(read) = ReadCall::from_item(item) {
        let kind = if skill_name_from_path(read.path()).is_some() {
            GroupBucket::Skill
        } else {
            GroupBucket::File
        };
        return Some((kind, read.status()));
    }
    SearchCall::from_item(item).map(|search| (GroupBucket::Search, search.status()))
}

fn skill_name_from_path(path: &str) -> Option<&str> {
    let path = Path::new(path);
    if path.file_name()?.to_str()? != "SKILL.md" {
        return None;
    }
    path.parent()?.file_name()?.to_str()
}

struct GroupLabel {
    text: String,
    running: bool,
    failed_count: usize,
}

fn aggregate_label(entries: &[TranscriptEntry], indices: &[usize]) -> GroupLabel {
    let mut buckets: Vec<(GroupBucket, usize)> = Vec::new();
    let mut running = false;
    let mut failed_count = 0;
    for index in indices {
        let Some((kind, status)) = entries
            .get(*index)
            .map(TranscriptEntry::item)
            .and_then(member_kind_and_status)
        else {
            continue;
        };
        if let Some((_, count)) = buckets.iter_mut().find(|(bucket, _)| *bucket == kind) {
            *count += 1;
        } else {
            buckets.push((kind, 1));
        }
        running |= status == CoreToolCallStatus::InProgress;
        failed_count += usize::from(matches!(
            status,
            CoreToolCallStatus::Failed | CoreToolCallStatus::Interrupted
        ));
    }
    let mut text = buckets
        .into_iter()
        .map(|(kind, count)| format!("{} {count} {}", kind.verb(running), kind.noun(count)))
        .collect::<Vec<_>>()
        .join(", ");
    if failed_count > 0 {
        text.push_str(&format!(" · {failed_count} failed"));
    }
    GroupLabel {
        text,
        running,
        failed_count,
    }
}

#[cfg(test)]
#[path = "verb_group_tests.rs"]
mod tests;
