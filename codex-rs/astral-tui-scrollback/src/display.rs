use codex_app_server_protocol::UserInput;
use unicode_width::UnicodeWidthStr;

use crate::EntryBlock;

const USER_COLLAPSED_MAX_LINES: usize = 3;
const USER_FOLD_ESTIMATE_WIDTH: usize = 60;

/// Presentation-only visibility for one transcript entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    Collapsed,
    Truncated,
    Expanded,
}

/// Mutable interaction state keyed by a stable transcript entry id in the TUI.
///
/// It contains no protocol data. Replacing or replaying a `ThreadItem` cannot
/// rewrite this state, while an unpinned entry may adopt the block's new default
/// when its lifecycle changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryDisplayState {
    mode: DisplayMode,
    mode_pinned: bool,
    raw: bool,
}

impl EntryDisplayState {
    pub fn for_block(block: &EntryBlock<'_>) -> Option<Self> {
        let policy = block.display_policy()?;
        Some(Self {
            mode: policy.default_mode,
            mode_pinned: false,
            raw: false,
        })
    }

    pub fn mode(self) -> DisplayMode {
        self.mode
    }

    pub fn mode_pinned(self) -> bool {
        self.mode_pinned
    }

    pub fn raw(self) -> bool {
        self.raw
    }

    /// Adopt lifecycle-driven defaults unless the user explicitly chose a
    /// fold. This is what collapses a finished Thought without overriding an
    /// expanded Thought the user pinned while it was running.
    pub fn reconcile(&mut self, block: &EntryBlock<'_>) -> bool {
        let Some(policy) = block.display_policy() else {
            return false;
        };
        let before = *self;
        if !self.mode_pinned {
            self.mode = policy.default_mode;
        }
        if !policy.has_raw_mode {
            self.raw = false;
        }
        *self != before
    }

    pub fn toggle_fold(&mut self, block: &EntryBlock<'_>) -> bool {
        let Some(policy) = block.display_policy().filter(|policy| policy.foldable) else {
            return false;
        };
        self.mode = policy.next_mode(self.mode);
        self.mode_pinned = true;
        true
    }

    pub fn collapse(&mut self, block: &EntryBlock<'_>) -> bool {
        let Some(policy) = block.display_policy().filter(|policy| policy.foldable) else {
            return false;
        };
        let mode = policy.collapse_mode();
        let changed = self.mode != mode || !self.mode_pinned;
        self.mode = mode;
        self.mode_pinned = true;
        changed
    }

    pub fn expand(&mut self, block: &EntryBlock<'_>) -> bool {
        if !block.display_policy().is_some_and(|policy| policy.foldable) {
            return false;
        }
        let changed = self.mode != DisplayMode::Expanded || !self.mode_pinned;
        self.mode = DisplayMode::Expanded;
        self.mode_pinned = true;
        changed
    }

    pub fn toggle_raw(&mut self, block: &EntryBlock<'_>) -> bool {
        if !block
            .display_policy()
            .is_some_and(|policy| policy.has_raw_mode)
        {
            return false;
        }
        self.raw = !self.raw;
        true
    }

    pub fn reset(&mut self, block: &EntryBlock<'_>) -> bool {
        let Some(policy) = block.display_policy() else {
            return false;
        };
        let before = *self;
        self.mode = policy.default_mode;
        self.mode_pinned = false;
        self.raw = false;
        *self != before
    }
}

impl EntryBlock<'_> {
    fn display_policy(&self) -> Option<DisplayPolicy> {
        match self {
            Self::User { content } => {
                let foldable = user_is_foldable(content);
                Some(DisplayPolicy {
                    default_mode: if foldable {
                        DisplayMode::Collapsed
                    } else {
                        DisplayMode::Expanded
                    },
                    foldable,
                    has_raw_mode: false,
                    fold_cycle: FoldCycle::TwoState,
                })
            }
            Self::Assistant { .. } => Some(DisplayPolicy {
                default_mode: DisplayMode::Expanded,
                foldable: false,
                has_raw_mode: true,
                fold_cycle: FoldCycle::TwoState,
            }),
            Self::ProposedPlan { .. } => Some(DisplayPolicy {
                default_mode: DisplayMode::Expanded,
                foldable: false,
                has_raw_mode: true,
                fold_cycle: FoldCycle::TwoState,
            }),
            Self::Reasoning(reasoning) => Some(DisplayPolicy {
                default_mode: if reasoning.running() {
                    DisplayMode::Truncated
                } else {
                    DisplayMode::Collapsed
                },
                foldable: true,
                has_raw_mode: true,
                fold_cycle: if reasoning.running() {
                    FoldCycle::RunningReasoning
                } else {
                    FoldCycle::TwoState
                },
            }),
            Self::ProtocolItem { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DisplayPolicy {
    default_mode: DisplayMode,
    foldable: bool,
    has_raw_mode: bool,
    fold_cycle: FoldCycle,
}

impl DisplayPolicy {
    fn next_mode(self, current: DisplayMode) -> DisplayMode {
        match self.fold_cycle {
            FoldCycle::TwoState => match current {
                DisplayMode::Collapsed => DisplayMode::Expanded,
                DisplayMode::Truncated | DisplayMode::Expanded => DisplayMode::Collapsed,
            },
            FoldCycle::RunningReasoning => match current {
                DisplayMode::Collapsed | DisplayMode::Truncated => DisplayMode::Expanded,
                DisplayMode::Expanded => DisplayMode::Truncated,
            },
        }
    }

    fn collapse_mode(self) -> DisplayMode {
        match self.fold_cycle {
            FoldCycle::RunningReasoning => DisplayMode::Truncated,
            FoldCycle::TwoState => DisplayMode::Collapsed,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum FoldCycle {
    TwoState,
    RunningReasoning,
}

fn user_is_foldable(content: &[UserInput]) -> bool {
    let mut visual_lines = 0;
    for input in content {
        match input {
            UserInput::Text { text, .. } => {
                for line in text.lines() {
                    let width = UnicodeWidthStr::width(line);
                    visual_lines += width.max(1).div_ceil(USER_FOLD_ESTIMATE_WIDTH);
                    if visual_lines > USER_COLLAPSED_MAX_LINES {
                        return true;
                    }
                }
            }
            UserInput::Image { .. }
            | UserInput::LocalImage { .. }
            | UserInput::Skill { .. }
            | UserInput::Mention { .. } => visual_lines += 1,
        }
    }
    visual_lines > USER_COLLAPSED_MAX_LINES
}

#[cfg(test)]
#[path = "display_tests.rs"]
mod tests;
