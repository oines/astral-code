//! Session-local Astral appearance state and timeline projection.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::ActiveOverlay;
use super::SurfaceState;
use crate::permission_picker::render_picker as render_permission_picker;
use crate::theme_picker::ThemePickerState;
use crate::theme_picker::render_picker as render_theme_picker;
use crate::thread_picker::render_picker as render_thread_picker;
use crate::timeline_rail::RailEligibility;
use crate::timeline_rail::RailViewport;
use crate::timeline_rail::TimelineHit;
use crate::timeline_rail::TimelineRail;
use crate::timeline_rail::compute_rail;
use crate::timeline_rail::rail_width;
use crate::timeline_rail::render_rail;
use crate::timeline_rail::render_tick_hover_popup;
use crate::view::AstralTheme;
use crate::view::AstralThemeId;
use crate::view::BlockViewerPane;
use crate::view::ColorLevel;
use crate::view::CommandPalette;
use crate::view::FileViewerPane;
use crate::view::InfoModal;
use crate::view::ShortcutHelp;

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
    pub(super) viewport: RailViewport,
}

pub(super) fn render_timeline(
    buffer: &mut Buffer,
    theme: AstralTheme,
    frame: TimelineFrame,
    hovered: Option<TimelineHit>,
    preview: Option<&str>,
) -> Option<TimelineRail> {
    let rail = compute_rail(
        frame.scrollback,
        frame.rail_x,
        frame.turn_count,
        frame.viewport,
    )?;
    render_rail(buffer, &rail, hovered, theme);
    if let Some(TimelineHit::Tick(turn_index)) = hovered
        && let Some(preview) = preview
    {
        render_tick_hover_popup(buffer, &rail, frame.scrollback, turn_index, preview, theme);
    }
    Some(rail)
}

pub(super) fn render_overlay(
    state: &mut SurfaceState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) -> bool {
    let Some(overlay) = state.active_overlay() else {
        return false;
    };
    match overlay {
        ActiveOverlay::Subagent => {
            if !state.render_subagent_overlay(area, buffer, theme) {
                return false;
            }
        }
        ActiveOverlay::FileViewer => {
            let Some(viewer) = state.file_viewer_mut() else {
                return false;
            };
            FileViewerPane { state: viewer }.render(area, buffer, theme);
        }
        ActiveOverlay::BlockViewer => {
            let Some((block, is_running)) = state.current_block_viewer_entry() else {
                state.close_block_viewer();
                return render_overlay(state, area, buffer, theme);
            };
            let text_mode = state.block_viewer_text_mode();
            let Some(viewer) = state.block_viewer_mut() else {
                return false;
            };
            BlockViewerPane {
                state: viewer,
                block: &block,
                text_mode,
                is_running,
            }
            .render(area, buffer, theme);
        }
        ActiveOverlay::ThemePicker => {
            let Some(picker) = &mut state.theme_picker else {
                return false;
            };
            render_theme_picker(picker, area, buffer, theme);
        }
        ActiveOverlay::PermissionPicker => {
            let Some(picker) = &mut state.permission_picker else {
                return false;
            };
            render_permission_picker(picker, area, buffer, theme);
        }
        ActiveOverlay::ThreadPicker => {
            let Some(picker) = &mut state.thread_picker else {
                return false;
            };
            render_thread_picker(picker, area, buffer, theme);
        }
        ActiveOverlay::CommandPalette => {
            let Some(palette) = &mut state.command_palette else {
                return false;
            };
            CommandPalette { state: palette }.render(area, buffer, theme);
        }
        ActiveOverlay::ShortcutHelp => {
            let Some(shortcuts) = &mut state.shortcut_help else {
                return false;
            };
            ShortcutHelp { state: shortcuts }.render(area, buffer, theme);
        }
        ActiveOverlay::InfoModal => {
            let Some(modal) = &mut state.modal else {
                return false;
            };
            InfoModal { state: modal }.render(area, buffer, theme);
        }
    }
    true
}
