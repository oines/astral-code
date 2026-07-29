use astral_terminal_inline::LinkSpan;
use crossterm::event::MouseEvent;

use super::super::LinkMouseAction;
use super::super::LinkTarget;
use super::ScrollbackState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScrollbackMouseAction {
    Ignored,
    Copy(String),
    Open(LinkTarget),
}

impl ScrollbackState {
    pub(crate) fn cycle_link(&mut self, forward: bool) -> bool {
        self.links.cycle(forward)
    }

    pub(crate) fn highlighted_link_target(&self) -> Option<LinkTarget> {
        self.links.highlighted_target()
    }

    pub(crate) fn has_visible_links(&self) -> bool {
        !self.links.is_empty()
    }

    pub(crate) fn frame_link_spans(&self) -> Vec<LinkSpan> {
        self.links.frame_spans()
    }

    pub(super) fn handle_link_mouse(&mut self, mouse: MouseEvent) -> Option<ScrollbackMouseAction> {
        match self.links.handle_mouse(mouse) {
            LinkMouseAction::Ignored => None,
            LinkMouseAction::Consumed => Some(ScrollbackMouseAction::Ignored),
            LinkMouseAction::Open(target) => Some(ScrollbackMouseAction::Open(target)),
        }
    }
}
