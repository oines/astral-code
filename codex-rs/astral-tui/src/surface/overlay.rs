//! Authoritative ownership for full-surface overlays.
//!
//! Rendering and every input path consult this same value. That keeps a
//! background request or a stale picker from receiving input while a different
//! overlay is visibly on top.

use super::SurfaceState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveOverlay {
    Subagent,
    FileViewer,
    BlockViewer,
    ThemePicker,
    PermissionPicker,
    ThreadPicker,
    ShortcutHelp,
    InfoModal,
}

impl SurfaceState {
    pub(crate) fn active_overlay(&self) -> Option<ActiveOverlay> {
        if self.subagent_view_open() {
            Some(ActiveOverlay::Subagent)
        } else if self.file_viewer().is_some() {
            Some(ActiveOverlay::FileViewer)
        } else if self.block_viewer().is_some() {
            Some(ActiveOverlay::BlockViewer)
        } else if self.theme_picker().is_some() {
            Some(ActiveOverlay::ThemePicker)
        } else if self.permission_picker().is_some() {
            Some(ActiveOverlay::PermissionPicker)
        } else if self.thread_picker().is_some() {
            Some(ActiveOverlay::ThreadPicker)
        } else if self.shortcut_help().is_some() {
            Some(ActiveOverlay::ShortcutHelp)
        } else if self.modal().is_some() {
            Some(ActiveOverlay::InfoModal)
        } else {
            None
        }
    }
}
