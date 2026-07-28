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

const MAX_VISIBLE_DENSE_ENTRIES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryGroupKind {
    VerbRun,
    Truncation,
}

/// One view-time fold over a stable range of transcript blocks.
///
/// Grouping remains presentation-only: members keep their original item ids,
/// order, and block data. A collapsed group merely decides which members the
/// transcript renderer exposes in the current frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntryGroupSpan {
    pub(crate) id: String,
    pub(crate) kind: EntryGroupKind,
    pub(crate) range: std::ops::Range<usize>,
    pub(crate) claimed: Vec<usize>,
    hidden: Vec<usize>,
    pub(crate) expanded: bool,
    pub(crate) label: String,
    pub(crate) running: bool,
    pub(crate) failed: bool,
}

impl EntryGroupSpan {
    pub(crate) fn header_owns_selection(&self) -> bool {
        self.kind == EntryGroupKind::Truncation || !self.expanded
    }

    pub(crate) fn hides(&self, index: usize) -> bool {
        self.hidden.contains(&index)
    }

    pub(crate) fn contains_member(&self, index: usize) -> bool {
        self.claimed.contains(&index)
    }
}

pub(crate) fn scan_turn(turn: &TranscriptTurn, display: &EntryDisplayState) -> Vec<EntryGroupSpan> {
    let (mut spans, claimed) = scan_verb_runs(turn, display);
    spans.extend(scan_truncations(turn, display, &claimed));
    spans.sort_unstable_by_key(|span| span.range.start);
    spans
}

fn scan_verb_runs(
    turn: &TranscriptTurn,
    display: &EntryDisplayState,
) -> (Vec<EntryGroupSpan>, Vec<bool>) {
    let mut spans = Vec::new();
    let mut claimed = vec![false; turn.blocks.len()];
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
        for member in &scan.claimed {
            claimed[*member] = true;
        }
        let hidden = if expanded {
            Vec::new()
        } else {
            scan.claimed.clone()
        };
        spans.push(EntryGroupSpan {
            id,
            kind: EntryGroupKind::VerbRun,
            range: index..scan.end,
            claimed: scan.claimed,
            hidden,
            expanded,
            running: label.running,
            failed: label.failed_count > 0,
            label: label.text,
        });
        index = scan.end;
    }
    (spans, claimed)
}

fn scan_truncations(
    turn: &TranscriptTurn,
    display: &EntryDisplayState,
    verb_claimed: &[bool],
) -> Vec<EntryGroupSpan> {
    let mut spans = Vec::new();
    let mut index = 0;
    while index < turn.blocks.len() {
        if verb_claimed[index] || !participates_in_truncation(turn, display, index) {
            index += 1;
            continue;
        }

        let start = index;
        let mut participants = Vec::new();
        while index < turn.blocks.len()
            && !verb_claimed[index]
            && participates_in_truncation(turn, display, index)
        {
            participants.push(index);
            index += 1;
        }
        if participants.len() <= MAX_VISIBLE_DENSE_ENTRIES + 1 {
            continue;
        }

        let id = entry_id(&turn.id, &turn.blocks[start].item_id);
        let expanded = display.group_is_expanded(&id);
        let hidden_count = participants.len() - MAX_VISIBLE_DENSE_ENTRIES;
        let label_indices = if expanded {
            participants.as_slice()
        } else {
            &participants[..hidden_count]
        };
        let label = aggregate_strict_label(turn, label_indices.iter().copied());
        let fallback = if expanded {
            format!("{} tool calls & thoughts", participants.len() - 1)
        } else {
            format!("{} more", hidden_count - 1)
        };
        let hidden = if expanded {
            vec![start]
        } else {
            participants[..hidden_count].to_vec()
        };
        spans.push(EntryGroupSpan {
            id,
            kind: EntryGroupKind::Truncation,
            range: start..index,
            claimed: participants,
            hidden,
            expanded,
            label: label.as_ref().map_or(fallback, |label| label.text.clone()),
            running: label.as_ref().is_some_and(|label| label.running),
            failed: label.as_ref().is_some_and(|label| label.failed_count > 0),
        });
    }
    spans
}

fn participates_in_truncation(
    turn: &TranscriptTurn,
    display: &EntryDisplayState,
    index: usize,
) -> bool {
    let Some(block) = turn.blocks.get(index) else {
        return false;
    };
    display.mode_for(&turn.id, &block.item_id, &block.block) == DisplayMode::Collapsed
        && matches!(
            block.block,
            PresentationBlock::Thinking { .. }
                | PresentationBlock::Tool(_)
                | PresentationBlock::Subagent(_)
                | PresentationBlock::System { .. }
        )
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
        ToolKind::WebFetch => Some(GroupBucket::WebFetch),
        ToolKind::WebSearch => Some(GroupBucket::WebSearch),
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
    WebFetch,
    WebSearch,
    Subagent,
    Command,
    EditFile,
    McpCall,
    OtherTool,
}

impl GroupBucket {
    fn verb(self, running: bool) -> &'static str {
        let (past, present) = match self {
            Self::File | Self::Skill => ("Read", "Reading"),
            Self::Search | Self::WebSearch => ("Searched", "Searching"),
            Self::Dir => ("Listed", "Listing"),
            Self::WebFetch => ("Fetched", "Fetching"),
            Self::Subagent | Self::Command | Self::OtherTool => ("Ran", "Running"),
            Self::EditFile => ("Edited", "Editing"),
            Self::McpCall => ("Called", "Calling"),
        };
        if running { present } else { past }
    }

    fn noun(self, count: usize) -> &'static str {
        let (singular, plural) = match self {
            Self::File | Self::EditFile => ("file", "files"),
            Self::Skill => ("skill", "skills"),
            Self::Search => ("pattern", "patterns"),
            Self::Dir => ("dir", "dirs"),
            Self::WebFetch | Self::WebSearch => ("website", "websites"),
            Self::Subagent => ("subagent", "subagents"),
            Self::Command => ("command", "commands"),
            Self::McpCall => ("MCP tool", "MCP tools"),
            Self::OtherTool => ("tool", "tools"),
        };
        if count == 1 { singular } else { plural }
    }
}

fn label_bucket(block: &PresentationBlock) -> Option<(GroupBucket, ToolStatus)> {
    match block {
        PresentationBlock::Tool(tool) => Some((label_tool_bucket(tool.kind), tool.status)),
        PresentationBlock::Subagent(subagent) => Some((GroupBucket::Subagent, subagent.status)),
        PresentationBlock::User { .. }
        | PresentationBlock::Assistant { .. }
        | PresentationBlock::Thinking { .. }
        | PresentationBlock::Plan { .. }
        | PresentationBlock::Todo(_)
        | PresentationBlock::System { .. } => None,
    }
}

fn label_tool_bucket(kind: ToolKind) -> GroupBucket {
    match kind {
        ToolKind::Read => GroupBucket::File,
        ToolKind::Skill => GroupBucket::Skill,
        ToolKind::Search => GroupBucket::Search,
        ToolKind::List => GroupBucket::Dir,
        ToolKind::WebFetch => GroupBucket::WebFetch,
        ToolKind::WebSearch => GroupBucket::WebSearch,
        ToolKind::Collab => GroupBucket::Subagent,
        ToolKind::Execute
        | ToolKind::Background
        | ToolKind::BackgroundPoll
        | ToolKind::BackgroundInput
        | ToolKind::BackgroundList
        | ToolKind::BackgroundStop => GroupBucket::Command,
        ToolKind::Edit => GroupBucket::EditFile,
        ToolKind::Mcp => GroupBucket::McpCall,
        ToolKind::ImageView | ToolKind::ImageGeneration | ToolKind::Todo | ToolKind::Other => {
            GroupBucket::OtherTool
        }
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
    aggregate_known_labels(turn, indices).unwrap_or_else(|| GroupLabel {
        text: match fallback {
            GroupLabelFallback::Members(count) => format!("{count} tool calls"),
        },
        running: false,
        failed_count: 0,
    })
}

fn aggregate_strict_label(
    turn: &TranscriptTurn,
    indices: impl Iterator<Item = usize>,
) -> Option<GroupLabel> {
    let indices = indices.collect::<Vec<_>>();
    if indices.iter().any(|index| {
        turn.blocks.get(*index).is_some_and(|block| {
            !matches!(block.block, PresentationBlock::Thinking { .. })
                && label_bucket(&block.block).is_none()
        })
    }) {
        return None;
    }
    aggregate_known_labels(turn, indices.into_iter())
}

fn aggregate_known_labels(
    turn: &TranscriptTurn,
    indices: impl Iterator<Item = usize>,
) -> Option<GroupLabel> {
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
    if buckets.is_empty() {
        return None;
    }
    let mut text = buckets
        .into_iter()
        .map(|(kind, count)| format!("{} {count} {}", kind.verb(running), kind.noun(count)))
        .collect::<Vec<_>>()
        .join(", ");
    if failed_count > 0 {
        text.push_str(&format!(" · {failed_count} failed"));
    }
    Some(GroupLabel {
        text,
        running,
        failed_count,
    })
}

#[cfg(test)]
#[path = "entry_group_tests.rs"]
mod tests;
