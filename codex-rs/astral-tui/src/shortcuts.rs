//! Interactive shortcut help derived from Astral's action registry.

use std::collections::HashSet;

use crate::actions;
use crate::actions::ActionDef;
use crate::actions::ActionId;
use crate::actions::Category;
use crate::actions::When;
use crate::modal::ModalPointerState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShortcutRow {
    Category(Category),
    Action(usize),
}

#[derive(Debug, Clone)]
pub(crate) struct ShortcutHelpState {
    context: When,
    query: String,
    search_active: bool,
    hide_inactive: bool,
    collapsed: HashSet<Category>,
    expanded: HashSet<ActionId>,
    detail: Option<ActionId>,
    selected: usize,
    pub(crate) scroll_offset: usize,
    pub(crate) detail_scroll: usize,
    pub(crate) pointer: ModalPointerState,
}

impl ShortcutHelpState {
    pub(crate) fn new(context: When) -> Self {
        Self {
            context,
            query: String::new(),
            search_active: false,
            hide_inactive: false,
            collapsed: Category::ORDER.into_iter().skip(1).collect(),
            expanded: HashSet::new(),
            detail: None,
            selected: 1,
            scroll_offset: 0,
            detail_scroll: 0,
            pointer: ModalPointerState::default(),
        }
    }

    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    pub(crate) fn search_active(&self) -> bool {
        self.search_active
    }

    pub(crate) fn hide_inactive(&self) -> bool {
        self.hide_inactive
    }

    pub(crate) fn detail(&self) -> Option<&'static ActionDef> {
        let id = self.detail?;
        actions::definitions()
            .iter()
            .find(|definition| definition.id == id)
    }

    pub(crate) fn selected(&self) -> usize {
        self.selected
    }

    pub(crate) fn is_expanded(&self, id: ActionId) -> bool {
        self.expanded.contains(&id)
    }

    pub(crate) fn is_collapsed(&self, category: Category) -> bool {
        self.collapsed.contains(&category)
    }

    pub(crate) fn is_active(&self, definition: &ActionDef) -> bool {
        definition.context == When::Always || definition.context == self.context
    }

    pub(crate) fn visible_rows(&self) -> Vec<ShortcutRow> {
        let query = self.query.to_lowercase();
        let searching = !query.is_empty();
        let mut rows = Vec::new();
        for category in Category::ORDER {
            let matching = actions::definitions()
                .iter()
                .enumerate()
                .filter(|(_, definition)| definition.category == category)
                .filter(|(_, definition)| !self.hide_inactive || self.is_active(definition))
                .filter(|(_, definition)| {
                    !searching
                        || definition.label.to_lowercase().contains(&query)
                        || definition.description.to_lowercase().contains(&query)
                        || definition.key_display().to_lowercase().contains(&query)
                        || definition
                            .long_help
                            .is_some_and(|help| help.to_lowercase().contains(&query))
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if matching.is_empty() {
                continue;
            }
            rows.push(ShortcutRow::Category(category));
            if searching || !self.is_collapsed(category) {
                rows.extend(matching.into_iter().map(ShortcutRow::Action));
            }
        }
        rows
    }

    pub(crate) fn selected_row(&self) -> Option<ShortcutRow> {
        self.visible_rows().get(self.selected).copied()
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let len = self.visible_rows().len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));
    }

    pub(crate) fn select(&mut self, selected: usize) {
        self.selected = selected.min(self.visible_rows().len().saturating_sub(1));
    }

    pub(crate) fn select_start(&mut self) {
        self.selected = 0;
    }

    pub(crate) fn select_end(&mut self) {
        self.selected = self.visible_rows().len().saturating_sub(1);
    }

    pub(crate) fn toggle_selected(&mut self) -> bool {
        match self.selected_row() {
            Some(ShortcutRow::Category(category)) => {
                if !self.collapsed.insert(category) {
                    self.collapsed.remove(&category);
                }
                self.clamp_selection();
                true
            }
            Some(ShortcutRow::Action(index)) => {
                let Some(definition) = actions::definitions().get(index) else {
                    return false;
                };
                if !self.expanded.insert(definition.id) {
                    self.expanded.remove(&definition.id);
                }
                true
            }
            None => false,
        }
    }

    pub(crate) fn collapse_selected(&mut self) -> bool {
        match self.selected_row() {
            Some(ShortcutRow::Category(category)) => self.collapsed.insert(category),
            Some(ShortcutRow::Action(index)) => actions::definitions()
                .get(index)
                .is_some_and(|definition| self.expanded.remove(&definition.id)),
            None => false,
        }
    }

    pub(crate) fn expand_selected(&mut self) -> bool {
        match self.selected_row() {
            Some(ShortcutRow::Category(category)) => self.collapsed.remove(&category),
            Some(ShortcutRow::Action(index)) => actions::definitions()
                .get(index)
                .is_some_and(|definition| self.expanded.insert(definition.id)),
            None => false,
        }
    }

    pub(crate) fn open_selected_detail(&mut self) -> bool {
        let Some(ShortcutRow::Action(index)) = self.selected_row() else {
            return self.toggle_selected();
        };
        let Some(definition) = actions::definitions().get(index) else {
            return false;
        };
        self.detail = Some(definition.id);
        self.detail_scroll = 0;
        self.search_active = false;
        self.query.clear();
        true
    }

    pub(crate) fn close_detail(&mut self) -> bool {
        self.detail.take().is_some()
    }

    pub(crate) fn begin_search(&mut self) {
        self.search_active = true;
    }

    pub(crate) fn insert_query(&mut self, character: char) {
        self.query.push(character);
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub(crate) fn backspace_query(&mut self) {
        self.query.pop();
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub(crate) fn clear_search(&mut self) -> bool {
        if !self.search_active && self.query.is_empty() {
            return false;
        }
        self.search_active = false;
        self.query.clear();
        self.selected = 1.min(self.visible_rows().len().saturating_sub(1));
        self.scroll_offset = 0;
        true
    }

    pub(crate) fn toggle_filter(&mut self) {
        self.hide_inactive = !self.hide_inactive;
        self.selected = 0;
        self.scroll_offset = 0;
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        self.selected = self
            .selected
            .min(self.visible_rows().len().saturating_sub(1));
    }
}
