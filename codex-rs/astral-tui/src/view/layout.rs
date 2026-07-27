use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;

/// Terminals at or below this height suppress optional rows above the prompt.
pub(crate) const SHORT_TERMINAL_ROWS: u16 = 16;

/// Auto-compact threshold inherited from the Grok pager layout.
pub(crate) const AUTO_COMPACT_MAX_ROWS: u16 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LayoutConfig {
    pub(crate) outer_vpad: u16,
    pub(crate) outer_hpad_left: u16,
    pub(crate) outer_hpad_right: u16,
    pub(crate) block_pad_left: u16,
    pub(crate) block_pad_right: u16,
}

impl LayoutConfig {
    const MIN_HPAD: u16 = 1;

    fn effective_outer_vpad(self, compact: bool) -> u16 {
        if compact { 0 } else { self.outer_vpad }
    }

    fn effective_hpad_left(self, compact: bool) -> u16 {
        if compact {
            Self::MIN_HPAD
        } else {
            self.outer_hpad_left.max(Self::MIN_HPAD)
        }
    }

    fn effective_hpad_right(self, compact: bool) -> u16 {
        if compact {
            Self::MIN_HPAD
        } else {
            self.outer_hpad_right.max(Self::MIN_HPAD)
        }
    }
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            outer_vpad: 1,
            outer_hpad_left: 2,
            outer_hpad_right: 2,
            block_pad_left: 2,
            block_pad_right: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollbarConfig {
    pub(crate) enabled: bool,
    pub(crate) gap_left: u16,
    pub(crate) gap_right: u16,
}

impl Default for ScrollbarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            gap_left: 0,
            gap_right: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PaneHeights {
    pub(crate) prompt: u16,
    pub(crate) tasks: u16,
    pub(crate) catalog: u16,
    pub(crate) todo: u16,
    pub(crate) queue: u16,
    pub(crate) btw: u16,
    pub(crate) turn_status: u16,
    pub(crate) banner: u16,
    pub(crate) plugin_cta: u16,
    pub(crate) follow_ups: u16,
    pub(crate) startup_warnings: u16,
    pub(crate) prompt_gap: u16,
    pub(crate) voice_recording: u16,
    pub(crate) shortcuts: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AgentViewLayoutInput {
    pub(crate) area: Rect,
    pub(crate) layout: LayoutConfig,
    pub(crate) scrollbar: ScrollbarConfig,
    pub(crate) panes: PaneHeights,
    pub(crate) timeline_width: u16,
    pub(crate) compact: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AgentViewLayout {
    pub(crate) status_bar: Rect,
    pub(crate) startup_warnings: Rect,
    pub(crate) tasks: Rect,
    pub(crate) catalog: Rect,
    pub(crate) scrollback: Rect,
    pub(crate) todo: Rect,
    pub(crate) queue: Rect,
    pub(crate) btw: Rect,
    pub(crate) turn_status: Rect,
    pub(crate) banner: Rect,
    pub(crate) plugin_cta: Rect,
    pub(crate) follow_ups: Rect,
    pub(crate) voice_recording: Rect,
    pub(crate) prompt: Rect,
    pub(crate) shortcuts: Rect,
    pub(crate) scrollback_content: Rect,
    pub(crate) scrollbar_x: u16,
    pub(crate) timeline_x: u16,
    pub(crate) timeline_width: u16,
}

impl AgentViewLayout {
    pub(crate) fn compute(input: AgentViewLayoutInput) -> Self {
        let compact =
            input.compact || (input.area.height > 0 && input.area.height <= AUTO_COMPACT_MAX_ROWS);
        let outer_vpad = input.layout.effective_outer_vpad(compact);
        let bottom_vpad = if input.area.height <= SHORT_TERMINAL_ROWS {
            0
        } else {
            outer_vpad
        };
        let inner = inset(
            input.area,
            input.layout.effective_hpad_left(compact),
            input.layout.effective_hpad_right(compact),
            outer_vpad,
            bottom_vpad,
        );
        let mut panes = input.panes;
        if input.area.height <= SHORT_TERMINAL_ROWS {
            panes.plugin_cta = 0;
            panes.follow_ups = 0;
        }
        let pane_gap = u16::from(outer_vpad > 0);
        let mut constraints = vec![Constraint::Length(1)];
        push_pane(&mut constraints, panes.startup_warnings, 0);
        push_pane(&mut constraints, panes.tasks, pane_gap);
        push_pane(&mut constraints, panes.catalog, pane_gap);
        push_pane(&mut constraints, panes.todo, pane_gap);
        constraints.push(Constraint::Length(pane_gap));
        constraints.push(Constraint::Min(5));
        push_pane(&mut constraints, panes.btw, 1);
        push_pane(&mut constraints, panes.queue, 1);
        push_pane(&mut constraints, panes.turn_status, 1);
        push_pane(&mut constraints, panes.banner, 1);
        push_pane(&mut constraints, panes.plugin_cta, 1);
        push_pane(&mut constraints, panes.follow_ups, 1);
        if panes.prompt_gap > 0 {
            constraints.push(Constraint::Length(panes.prompt_gap));
        }
        if panes.voice_recording > 0 {
            constraints.push(Constraint::Length(panes.voice_recording));
        }
        constraints.push(Constraint::Length(panes.prompt));
        if bottom_vpad > 0 {
            constraints.push(Constraint::Length(1));
        }
        constraints.push(Constraint::Length(panes.shortcuts));

        let chunks = Layout::vertical(constraints).split(inner);
        let mut index = 0;
        let status_bar = take(chunks.as_ref(), &mut index);
        let startup_warnings =
            take_optional(chunks.as_ref(), &mut index, panes.startup_warnings, 0);
        let tasks = take_optional(chunks.as_ref(), &mut index, panes.tasks, pane_gap);
        let catalog = take_optional(chunks.as_ref(), &mut index, panes.catalog, pane_gap);
        let todo = take_optional(chunks.as_ref(), &mut index, panes.todo, pane_gap);
        index += 1;
        let scrollback = take(chunks.as_ref(), &mut index);
        let btw = take_optional(chunks.as_ref(), &mut index, panes.btw, 1);
        let queue = take_optional(chunks.as_ref(), &mut index, panes.queue, 1);
        let turn_status = take_optional(chunks.as_ref(), &mut index, panes.turn_status, 1);
        let banner = take_optional(chunks.as_ref(), &mut index, panes.banner, 1);
        let plugin_cta = take_optional(chunks.as_ref(), &mut index, panes.plugin_cta, 1);
        let follow_ups = take_optional(chunks.as_ref(), &mut index, panes.follow_ups, 1);
        if panes.prompt_gap > 0 {
            index += 1;
        }
        let voice_recording = if panes.voice_recording > 0 {
            take(chunks.as_ref(), &mut index)
        } else {
            Rect::default()
        };
        let prompt = take(chunks.as_ref(), &mut index);
        if bottom_vpad > 0 {
            index += 1;
        }
        let shortcuts = take(chunks.as_ref(), &mut index);

        let scrollbar_x = input
            .area
            .right()
            .saturating_sub(input.scrollbar.gap_right + 1);
        let timeline_width = if input.scrollbar.enabled {
            input.timeline_width
        } else {
            0
        };
        let timeline_x = (scrollbar_x + 1).saturating_sub(timeline_width);
        let content_end_x = if timeline_width > 0 {
            timeline_x.saturating_sub(input.scrollbar.gap_left)
        } else {
            scrollbar_x.saturating_sub(input.scrollbar.gap_left)
        };
        let scrollback_content = if !input.scrollbar.enabled || content_end_x >= scrollback.right()
        {
            scrollback
        } else {
            Rect {
                width: content_end_x.saturating_sub(scrollback.x),
                ..scrollback
            }
        };

        Self {
            status_bar,
            startup_warnings,
            tasks,
            catalog,
            scrollback,
            todo,
            queue,
            btw,
            turn_status,
            banner,
            plugin_cta,
            follow_ups,
            voice_recording,
            prompt,
            shortcuts,
            scrollback_content,
            scrollbar_x,
            timeline_x,
            timeline_width,
        }
    }
}

fn inset(area: Rect, left: u16, right: u16, top: u16, bottom: u16) -> Rect {
    let horizontal = left.saturating_add(right).min(area.width);
    let vertical = top.saturating_add(bottom).min(area.height);
    Rect {
        x: area.x.saturating_add(left.min(area.width)),
        y: area.y.saturating_add(top.min(area.height)),
        width: area.width.saturating_sub(horizontal),
        height: area.height.saturating_sub(vertical),
    }
}

fn push_pane(constraints: &mut Vec<Constraint>, height: u16, gap: u16) {
    if height > 0 {
        if gap > 0 {
            constraints.push(Constraint::Length(gap));
        }
        constraints.push(Constraint::Length(height));
    }
}

fn take(chunks: &[Rect], index: &mut usize) -> Rect {
    let rect = chunks.get(*index).copied().unwrap_or_default();
    *index += 1;
    rect
}

fn take_optional(chunks: &[Rect], index: &mut usize, height: u16, gap: u16) -> Rect {
    if height == 0 {
        return Rect::default();
    }
    if gap > 0 {
        *index += 1;
    }
    take(chunks, index)
}
