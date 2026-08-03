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
use crate::web_search::WebSearchKind;

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
    claimed_entry_ids: Vec<TranscriptEntryId>,
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

    /// Preserve expansion when opening or reordering members moves the run's
    /// anchor.
    ///
    /// The most-overlapping successor by stable entry identity inherits the
    /// old expansion key. Index ranges are intentionally ignored because an
    /// authoritative snapshot can put unrelated entries at the same offsets.
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
                    .map(|next| (next, shared_entry_count(old, next)))
                    .filter(|(_, shared)| *shared > 0)
                    .max_by_key(|(_, shared)| *shared)?
                    .0;
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

fn shared_entry_count(left: &VerbGroupSpan, right: &VerbGroupSpan) -> usize {
    left.claimed_entry_ids
        .iter()
        .filter(|entry_id| right.claimed_entry_ids.contains(entry_id))
        .count()
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
            claimed_entry_ids: scan
                .claimed
                .iter()
                .map(|index| entries[*index].id())
                .collect(),
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
        EntryBlock::ProtocolItem { .. }
        | EntryBlock::DynamicToolCall(_)
        | EntryBlock::WebSearch(_) => {
            let Some(meta) = member_meta(&block) else {
                return RunStep::Break;
            };
            if state.mode() == DisplayMode::Collapsed {
                RunStep::Member(meta.kind)
            } else {
                RunStep::Transparent
            }
        }
        EntryBlock::User { .. }
        | EntryBlock::Assistant { .. }
        | EntryBlock::ProposedPlan { .. }
        | EntryBlock::CollabAgentToolCall(_)
        | EntryBlock::McpToolCall(_)
        | EntryBlock::ContextCompaction(_) => RunStep::Break,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupBucket {
    File,
    Skill,
    Search,
    WebFetch,
    WebSearch,
}

impl GroupBucket {
    fn verb(self, running: bool) -> &'static str {
        match (self, running) {
            (Self::File | Self::Skill, false) => "Read",
            (Self::File | Self::Skill, true) => "Reading",
            (Self::Search, false) => "Searched",
            (Self::Search, true) => "Searching",
            (Self::WebFetch, false) => "Fetched",
            (Self::WebFetch, true) => "Fetching",
            (Self::WebSearch, false) => "Searched",
            (Self::WebSearch, true) => "Searching",
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
            (Self::WebFetch | Self::WebSearch, 1) => "website",
            (Self::WebFetch | Self::WebSearch, _) => "websites",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MemberMeta {
    kind: GroupBucket,
    running: bool,
    failed: bool,
}

fn member_meta(block: &EntryBlock<'_>) -> Option<MemberMeta> {
    match block {
        EntryBlock::ProtocolItem { item, .. } => protocol_member_meta(item),
        EntryBlock::WebSearch(search) => Some(MemberMeta {
            kind: match search.kind() {
                WebSearchKind::Search => GroupBucket::WebSearch,
                WebSearchKind::Fetch => GroupBucket::WebFetch,
            },
            running: search.running(),
            failed: false,
        }),
        EntryBlock::DynamicToolCall(call) if call.is_web_fetch() => Some(MemberMeta {
            kind: GroupBucket::WebFetch,
            running: call.running(),
            failed: call.failed(),
        }),
        EntryBlock::User { .. }
        | EntryBlock::Assistant { .. }
        | EntryBlock::ProposedPlan { .. }
        | EntryBlock::Reasoning(_)
        | EntryBlock::CollabAgentToolCall(_)
        | EntryBlock::DynamicToolCall(_)
        | EntryBlock::McpToolCall(_)
        | EntryBlock::ContextCompaction(_) => None,
    }
}

fn protocol_member_meta(item: &ThreadItem) -> Option<MemberMeta> {
    let (kind, status) = if let Some(read) = ReadCall::from_item(item) {
        let kind = if skill_name_from_path(read.path()).is_some() {
            GroupBucket::Skill
        } else {
            GroupBucket::File
        };
        (kind, read.status())
    } else {
        let search = SearchCall::from_item(item)?;
        (GroupBucket::Search, search.status())
    };
    Some(MemberMeta {
        kind,
        running: status == CoreToolCallStatus::InProgress,
        failed: matches!(
            status,
            CoreToolCallStatus::Failed | CoreToolCallStatus::Interrupted
        ),
    })
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
        let Some(entry) = entries.get(*index) else {
            continue;
        };
        let block = EntryBlock::from_entry(entry);
        let Some(meta) = member_meta(&block) else {
            continue;
        };
        if let Some((_, count)) = buckets.iter_mut().find(|(bucket, _)| *bucket == meta.kind) {
            *count += 1;
        } else {
            buckets.push((meta.kind, 1));
        }
        running |= meta.running;
        failed_count += usize::from(meta.failed);
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
