// Derived from Grok Build's scrollback view-time fold model at commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Adapted to Astral's provider-neutral PresentationBlock stream.

use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::PresentationBlock;
use astral_tui_scrollback::ToolKind;
use astral_tui_scrollback::ToolStatus;

use crate::conversation::TranscriptTurn;

use super::EntryDisplayState;
use super::entry_state::entry_id;

/// One view-time fold over a stable range of transcript blocks.
///
/// Grouping remains presentation-only: members keep their original item ids,
/// order, and block data. A collapsed group merely decides which members the
/// transcript renderer exposes in the current frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntryGroupSpan {
    pub(crate) id: String,
    pub(crate) range: std::ops::Range<usize>,
    pub(crate) claimed: Vec<usize>,
    pub(crate) expanded: bool,
    pub(crate) label: String,
    pub(crate) running: bool,
    pub(crate) failed: bool,
}

impl EntryGroupSpan {
    pub(crate) fn header_owns_selection(&self) -> bool {
        !self.expanded
    }

    pub(crate) fn hides(&self, index: usize) -> bool {
        !self.expanded && self.claimed.contains(&index)
    }

    pub(crate) fn contains_member(&self, index: usize) -> bool {
        self.claimed.contains(&index)
    }
}

pub(crate) fn scan_turn(turn: &TranscriptTurn, display: &EntryDisplayState) -> Vec<EntryGroupSpan> {
    scan_verb_runs(turn, display)
}

fn scan_verb_runs(turn: &TranscriptTurn, display: &EntryDisplayState) -> Vec<EntryGroupSpan> {
    let mut spans = Vec::new();
    let mut index = 0;
    while index < turn.blocks.len() {
        let Some(scan) = scan_verb_run(turn, display, index) else {
            index += 1;
            continue;
        };
        if scan.tool_members == 0 {
            index = scan.stop;
            continue;
        }
        let id = entry_id(&turn.id, &turn.blocks[index].item_id);
        let expanded = display.group_is_expanded(&id);
        let label = aggregate_label(
            turn,
            scan.claimed.iter().copied(),
            GroupLabelFallback::Members(scan.tool_members),
        );
        spans.push(EntryGroupSpan {
            id,
            range: index..scan.end,
            claimed: scan.claimed,
            expanded,
            running: label.running,
            failed: label.failed_count > 0,
            label: label.text,
        });
        index = scan.end;
    }
    spans
}

struct VerbRunScan {
    tool_members: usize,
    end: usize,
    stop: usize,
    claimed: Vec<usize>,
}

fn scan_verb_run(
    turn: &TranscriptTurn,
    display: &EntryDisplayState,
    start: usize,
) -> Option<VerbRunScan> {
    match run_step(turn, display, start)? {
        RunStep::Member(_) | RunStep::ThoughtMember => {}
        RunStep::Transparent | RunStep::Break => return None,
    }
    let mut tool_members = 0;
    let mut end = start;
    let mut claimed = Vec::new();
    let mut index = start;
    while index < turn.blocks.len() {
        let Some(step) = run_step(turn, display, index) else {
            break;
        };
        match step {
            RunStep::Member(_) => {
                tool_members += 1;
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
    Some(VerbRunScan {
        tool_members,
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

fn run_step(turn: &TranscriptTurn, display: &EntryDisplayState, index: usize) -> Option<RunStep> {
    let block = turn.blocks.get(index)?;
    let mode = display.mode_for(&turn.id, &block.item_id, &block.block);
    match &block.block {
        PresentationBlock::Thinking { running, .. } => {
            if *running || mode != DisplayMode::Collapsed {
                Some(RunStep::Transparent)
            } else {
                Some(RunStep::ThoughtMember)
            }
        }
        PresentationBlock::Tool(tool) => {
            eager_bucket(tool.kind).map_or(Some(RunStep::Break), |bucket| {
                Some(if mode == DisplayMode::Collapsed {
                    RunStep::Member(bucket)
                } else {
                    RunStep::Transparent
                })
            })
        }
        PresentationBlock::Subagent(_) => Some(if mode == DisplayMode::Collapsed {
            RunStep::Member(GroupBucket::Subagent)
        } else {
            RunStep::Transparent
        }),
        PresentationBlock::User { .. }
        | PresentationBlock::Assistant { .. }
        | PresentationBlock::Plan { .. }
        | PresentationBlock::Todo(_)
        | PresentationBlock::System { .. } => Some(RunStep::Break),
    }
}

fn eager_bucket(kind: ToolKind) -> Option<GroupBucket> {
    match kind {
        ToolKind::Read => Some(GroupBucket::File),
        ToolKind::Skill => Some(GroupBucket::Skill),
        ToolKind::Search => Some(GroupBucket::Search),
        ToolKind::List => Some(GroupBucket::Dir),
        ToolKind::WebFetch | ToolKind::WebSearch => Some(GroupBucket::Website),
        ToolKind::Collab => Some(GroupBucket::Subagent),
        ToolKind::Execute
        | ToolKind::Background
        | ToolKind::BackgroundPoll
        | ToolKind::BackgroundInput
        | ToolKind::BackgroundList
        | ToolKind::BackgroundStop
        | ToolKind::Edit
        | ToolKind::Mcp
        | ToolKind::ImageView
        | ToolKind::ImageGeneration
        | ToolKind::Todo
        | ToolKind::Other => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupBucket {
    File,
    Skill,
    Search,
    Dir,
    Website,
    Subagent,
}

impl GroupBucket {
    fn verb(self, running: bool) -> &'static str {
        let (past, present) = match self {
            Self::File | Self::Skill => ("Read", "Reading"),
            Self::Search | Self::Website => ("Searched", "Searching"),
            Self::Dir => ("Listed", "Listing"),
            Self::Subagent => ("Ran", "Running"),
        };
        if running { present } else { past }
    }

    fn noun(self, count: usize) -> &'static str {
        let (singular, plural) = match self {
            Self::File => ("file", "files"),
            Self::Skill => ("skill", "skills"),
            Self::Search => ("pattern", "patterns"),
            Self::Dir => ("dir", "dirs"),
            Self::Website => ("website", "websites"),
            Self::Subagent => ("subagent", "subagents"),
        };
        if count == 1 { singular } else { plural }
    }
}

fn label_bucket(block: &PresentationBlock) -> Option<(GroupBucket, ToolStatus)> {
    match block {
        PresentationBlock::Tool(tool) => {
            eager_bucket(tool.kind).map(|bucket| (bucket, tool.status))
        }
        PresentationBlock::Subagent(subagent) => Some((GroupBucket::Subagent, subagent.status)),
        PresentationBlock::User { .. }
        | PresentationBlock::Assistant { .. }
        | PresentationBlock::Thinking { .. }
        | PresentationBlock::Plan { .. }
        | PresentationBlock::Todo(_)
        | PresentationBlock::System { .. } => None,
    }
}

enum GroupLabelFallback {
    Members(usize),
}

struct GroupLabel {
    text: String,
    running: bool,
    failed_count: usize,
}

fn aggregate_label(
    turn: &TranscriptTurn,
    indices: impl Iterator<Item = usize>,
    fallback: GroupLabelFallback,
) -> GroupLabel {
    let mut buckets: Vec<(GroupBucket, usize)> = Vec::new();
    let mut running = false;
    let mut failed_count = 0;
    for index in indices {
        let Some((kind, status)) = turn
            .blocks
            .get(index)
            .and_then(|block| label_bucket(&block.block))
        else {
            continue;
        };
        if let Some((_, count)) = buckets.iter_mut().find(|(bucket, _)| *bucket == kind) {
            *count += 1;
        } else {
            buckets.push((kind, 1));
        }
        running |= status == ToolStatus::Running;
        failed_count += usize::from(status == ToolStatus::Failed);
    }
    let mut text = buckets
        .into_iter()
        .map(|(kind, count)| format!("{} {count} {}", kind.verb(running), kind.noun(count)))
        .collect::<Vec<_>>()
        .join(", ");
    if text.is_empty() {
        text = match fallback {
            GroupLabelFallback::Members(count) => format!("{count} tool calls"),
        };
    }
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
#[path = "entry_group_tests.rs"]
mod tests;
