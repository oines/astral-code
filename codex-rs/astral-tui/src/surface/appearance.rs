//! Session-local Astral appearance state and timeline projection.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::SurfaceState;
use crate::permission_picker::render_picker as render_permission_picker;
use crate::theme_picker::ThemePickerState;
use crate::theme_picker::render_picker as render_theme_picker;
use crate::thread_picker::render_picker as render_thread_picker;
use crate::timeline_rail::RailEligibility;
use crate::timeline_rail::RailViewport;
use crate::timeline_rail::compute_rail;
use crate::timeline_rail::rail_width;
use crate::timeline_rail::render_rail;
use crate::view::AstralTheme;
use crate::view::AstralThemeId;
use crate::view::BlockViewerPane;
use crate::view::ColorLevel;
use crate::view::InfoModal;

impl SurfaceState {
    pub(crate) fn theme_picker(&self) -> Option<&ThemePickerState> {
        self.theme_picker.as_ref()
    }

    pub(crate) fn theme_picker_mut(&mut self) -> Option<&mut ThemePickerState> {
        self.theme_picker.as_mut()
    }

    pub(crate) fn open_theme_picker(&mut self) {
        self.theme_picker = Some(ThemePickerState::new(self.theme));
    }

    pub(crate) fn close_theme_picker(&mut self) {
        self.theme_picker = None;
    }

    pub(crate) fn theme_id(&self) -> AstralThemeId {
        self.theme
    }

    pub(crate) fn set_theme(&mut self, theme: AstralThemeId) {
        self.theme = theme;
    }

    pub(crate) fn theme(&self) -> AstralTheme {
        AstralTheme::for_color_level(self.theme, self.color_level)
    }

    pub(crate) fn color_level(&self) -> ColorLevel {
        self.color_level
    }

    pub(crate) fn set_color_level(&mut self, color_level: ColorLevel) {
        self.color_level = color_level;
    }

    pub(crate) fn timeline_visible(&self) -> bool {
        self.timeline_visible
    }

    pub(crate) fn set_timeline_visible(&mut self, visible: bool) {
        self.timeline_visible = visible;
    }

    pub(crate) fn toggle_timeline(&mut self) -> bool {
        self.timeline_visible = !self.timeline_visible;
        self.timeline_visible
    }
}

pub(super) fn timeline_width(state: &SurfaceState, area_width: u16, turn_count: usize) -> u16 {
    rail_width(RailEligibility {
        visible: state.timeline_visible,
        area_width,
        turn_count,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TimelineFrame {
    pub(super) scrollback: Rect,
    pub(super) rail_x: u16,
    pub(super) turn_count: usize,
    pub(super) scroll_offset: usize,
    pub(super) first_visible_line: usize,
    pub(super) total_lines: usize,
}

pub(super) fn render_timeline(buffer: &mut Buffer, theme: AstralTheme, frame: TimelineFrame) {
    if frame.turn_count == 0 {
        return;
    }
    let at_bottom = frame.scroll_offset == 0;
    let active = if at_bottom || frame.total_lines == 0 {
        Some(frame.turn_count - 1)
    } else {
        Some(
            (frame.first_visible_line * frame.turn_count / frame.total_lines)
                .min(frame.turn_count - 1),
        )
    };
    if let Some(rail) = compute_rail(
        frame.scrollback,
        frame.rail_x,
        frame.turn_count,
        RailViewport { active, at_bottom },
    ) {
        render_rail(buffer, &rail, frame.turn_count, theme);
    }
}

pub(super) fn render_overlay(
    state: &mut SurfaceState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) -> bool {
    if state.block_viewer().is_some() {
        let Some(block) = state.current_block_viewer_block() else {
            state.close_block_viewer();
            return false;
        };
        let text_mode = state.block_viewer_text_mode();
        let Some(viewer) = state.block_viewer_mut() else {
            return false;
        };
        BlockViewerPane {
            state: viewer,
            block: &block,
            text_mode,
        }
        .render(area, buffer, theme);
    } else if let Some(picker) = &mut state.theme_picker {
        render_theme_picker(picker, area, buffer, theme);
    } else if let Some(picker) = &mut state.permission_picker {
        render_permission_picker(picker, area, buffer, theme);
    } else if let Some(picker) = &mut state.thread_picker {
        render_thread_picker(picker, area, buffer, theme);
    } else if let Some(modal) = &mut state.modal {
        InfoModal { state: modal }.render(area, buffer, theme);
    } else {
        return false;
    }
    true
}
