//! Browser navigation and search editing for the models manager.

use crossterm::event::KeyEvent;

use super::BrowserRow;
use super::ModelsManagerState;
use super::ProviderLoad;
use super::ProviderModelsRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrowserFocus {
    List,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrowserScroll {
    FollowSelection,
    Manual,
}

pub(super) const SEARCH_ROW_ID: usize = usize::MAX;

impl ModelsManagerState {
    pub(super) fn move_selection(&mut self, delta: isize) {
        let len = self.rows().len();
        if len > 0 {
            self.selected = self
                .selected
                .saturating_add_signed(delta)
                .min(len.saturating_sub(1));
        }
        self.browser_focus = BrowserFocus::List;
        self.browser_scroll = BrowserScroll::FollowSelection;
        self.pointer.clear_hover();
    }

    pub(super) fn set_selected(&mut self, selected: usize) {
        if selected < self.rows().len() {
            self.selected = selected;
        }
    }

    pub(super) fn clamp_selection(&mut self) {
        self.selected = self.selected.min(self.rows().len().saturating_sub(1));
    }

    pub(super) fn select_start(&mut self) {
        self.selected = 0;
        self.browser_focus = BrowserFocus::List;
        self.browser_scroll = BrowserScroll::FollowSelection;
        self.pointer.clear_hover();
    }

    pub(super) fn select_end(&mut self) {
        self.selected = self.rows().len().saturating_sub(1);
        self.browser_focus = BrowserFocus::List;
        self.browser_scroll = BrowserScroll::FollowSelection;
        self.pointer.clear_hover();
    }

    pub(super) fn scroll_browser(&mut self, delta: isize) {
        self.scroll_offset = self.scroll_offset.saturating_add_signed(delta);
        self.browser_focus = BrowserFocus::List;
        self.browser_scroll = BrowserScroll::Manual;
        self.pointer.clear_hover();
    }

    pub(super) fn focus_search(&mut self) {
        self.browser_focus = BrowserFocus::Search;
        self.pointer.clear_hover();
    }

    pub(super) fn focus_list(&mut self) {
        self.browser_focus = BrowserFocus::List;
        self.pointer.clear_hover();
    }

    pub(super) fn search_focused(&self) -> bool {
        self.browser_focus == BrowserFocus::Search
    }

    pub(super) fn query_is_empty(&self) -> bool {
        self.query.text().is_empty()
    }

    pub(super) fn edit_query(&mut self, key: KeyEvent) -> bool {
        let previous = self.query.text().to_string();
        let handled = self.query.edit_key(key);
        if self.query.text() != previous {
            self.reset_filtered_navigation();
        }
        handled
    }

    pub(super) fn paste_query(&mut self, text: &str) {
        let text = text
            .chars()
            .filter(|character| !character.is_control())
            .collect::<String>();
        if !text.is_empty() {
            self.query.insert_text(&text);
            self.reset_filtered_navigation();
        }
    }

    pub(super) fn clear_query(&mut self) -> bool {
        if self.query.text().is_empty() {
            return false;
        }
        self.query.clear();
        self.reset_filtered_navigation();
        true
    }

    pub(super) fn expand_selected(&mut self) {
        let Some(BrowserRow::Provider { provider_index }) = self.rows().get(self.selected).cloned()
        else {
            return;
        };
        self.set_provider_expanded(provider_index, true);
        self.pointer.clear_hover();
    }

    pub(super) fn collapse_selected(&mut self) {
        let rows = self.rows();
        let Some(row) = rows.get(self.selected).cloned() else {
            return;
        };
        let provider_index = match row {
            BrowserRow::Provider { provider_index } => {
                self.set_provider_expanded(provider_index, false);
                self.pointer.clear_hover();
                return;
            }
            BrowserRow::AddModel { provider_index }
            | BrowserRow::EditProvider { provider_index }
            | BrowserRow::Status { provider_index }
            | BrowserRow::Model { provider_index, .. } => provider_index,
            BrowserRow::AddProvider => return,
        };
        if let Some(parent) = rows.iter().position(|row| {
            matches!(
                row,
                BrowserRow::Provider {
                    provider_index: index
                } if *index == provider_index
            )
        }) {
            self.selected = parent;
            self.browser_scroll = BrowserScroll::FollowSelection;
            self.pointer.clear_hover();
        }
    }

    pub(super) fn set_provider_expanded(&mut self, provider_index: usize, expanded: bool) {
        let provider = &mut self.providers[provider_index];
        provider.expanded = expanded;
        if expanded
            && matches!(
                provider.load,
                ProviderLoad::NotLoaded | ProviderLoad::Failed(_)
            )
        {
            provider.load = ProviderLoad::Loading;
            self.pending_request = Some(ProviderModelsRequest {
                generation: self.generation,
                provider_id: provider.id.clone(),
            });
        }
    }

    fn reset_filtered_navigation(&mut self) {
        self.selected = 0;
        self.scroll_offset = 0;
        self.browser_scroll = BrowserScroll::FollowSelection;
        self.pointer.clear_hover();
    }
}
