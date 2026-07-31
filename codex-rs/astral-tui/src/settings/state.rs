use std::collections::BTreeSet;

use ratatui::layout::Rect;
use serde_json::Value;

use crate::composer::ComposerState;
use crate::modal::ModalPointerState;
use crate::view::AstralThemeId;

use super::Category;
use super::SettingDefinition;
use super::SettingsData;
use super::SettingsFocus;
use super::SettingsInput;
use super::SettingsStore;
use super::SettingsWrite;
use super::definitions;
use super::pages::SearchPageState;
use super::pages::SessionMemoryPageState;
use super::pages::models::ModelsManagerState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsPage {
    Root,
    Category(Category),
    Models,
    Search,
    SessionMemoryTemplates,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsRow {
    Category(Category),
    Definition(&'static SettingDefinition),
    Feature(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PickerOption {
    pub(crate) label: String,
    pub(crate) value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SettingsEditor {
    Text {
        definition: &'static SettingDefinition,
        input: ComposerState,
    },
    Picker {
        definition: Option<&'static SettingDefinition>,
        feature_index: Option<usize>,
        options: Vec<PickerOption>,
        selected: usize,
        original_theme: Option<AstralThemeId>,
    },
    Confirm {
        title: String,
        message: String,
        confirm_label: String,
        action: SettingsConfirmAction,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SettingsConfirmAction {
    Write {
        write: SettingsWrite,
        selected_theme: Option<AstralThemeId>,
    },
    DiscardAndBack {
        destination: SettingsPage,
    },
    DiscardModelsPanel,
    DiscardAndClose,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SettingsState {
    pub(super) generation: u64,
    pub(super) store: SettingsStore,
    pub(super) page: SettingsPage,
    pub(super) selected: usize,
    pub(super) scroll_offset: usize,
    pub(super) query: ComposerState,
    pub(super) search_focused: bool,
    pub(super) expanded: BTreeSet<String>,
    pub(super) editor: Option<SettingsEditor>,
    pub(super) models: ModelsManagerState,
    pub(super) search: SearchPageState,
    pub(super) session_memory: SessionMemoryPageState,
    pub(super) pointer: ModalPointerState,
    pub(super) row_expand_hits: Vec<Option<Rect>>,
    pub(super) row_value_hits: Vec<Option<Rect>>,
    pub(super) notice: Option<String>,
    pub(super) notice_is_error: bool,
    pub(super) current_theme: AstralThemeId,
}

impl SettingsState {
    pub(crate) fn new(
        generation: u64,
        data: SettingsData,
        current_provider: String,
        current_model: String,
        current_theme: AstralThemeId,
    ) -> Self {
        let models = ModelsManagerState::new(
            generation,
            data.config.clone(),
            data.models.clone(),
            current_provider,
            current_model,
        );
        let store = SettingsStore::new(data);
        let search = SearchPageState::new(&store);
        let session_memory = SessionMemoryPageState::new(&store);
        Self {
            generation,
            store,
            page: SettingsPage::Root,
            selected: 0,
            scroll_offset: 0,
            query: ComposerState::default(),
            search_focused: false,
            expanded: BTreeSet::new(),
            editor: None,
            models,
            search,
            session_memory,
            pointer: ModalPointerState::default(),
            row_expand_hits: Vec::new(),
            row_value_hits: Vec::new(),
            notice: None,
            notice_is_error: false,
            current_theme,
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn apply_focus(&mut self, focus: SettingsFocus) {
        match focus {
            SettingsFocus::Root => self.enter_page(SettingsPage::Root),
            SettingsFocus::Category(category) => {
                self.enter_page(SettingsPage::Category(category));
            }
            SettingsFocus::Key(key) => {
                let category = definitions()
                    .iter()
                    .find(|definition| definition.key == key)
                    .map(|definition| definition.category)
                    .unwrap_or(Category::Advanced);
                self.enter_page(SettingsPage::Category(category));
                if let Some(index) = self
                    .rows()
                    .iter()
                    .position(|row| matches!(row, SettingsRow::Definition(definition) if definition.key == key))
                {
                    self.selected = index;
                }
            }
            SettingsFocus::Models => self.enter_page(SettingsPage::Models),
            SettingsFocus::ModelsProvider(provider_id) => {
                self.enter_page(SettingsPage::Models);
                self.models.focus_provider(&provider_id);
            }
            SettingsFocus::Search => self.enter_page(SettingsPage::Search),
            SettingsFocus::SessionMemoryTemplates => {
                self.enter_page(SettingsPage::SessionMemoryTemplates);
            }
        }
    }

    pub(crate) fn models_mut(&mut self) -> &mut ModelsManagerState {
        &mut self.models
    }

    pub(crate) fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub(crate) fn set_notice(&mut self, notice: impl Into<String>) {
        self.notice = Some(notice.into());
        self.notice_is_error = false;
    }

    pub(crate) fn set_error(&mut self, error: impl Into<String>) {
        self.notice = Some(error.into());
        self.notice_is_error = true;
    }

    pub(crate) fn notice_is_error(&self) -> bool {
        self.notice_is_error
    }

    pub(crate) fn clear_notice(&mut self) {
        self.notice = None;
        self.notice_is_error = false;
    }

    pub(crate) fn focus_search(&mut self) {
        self.search_focused = true;
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub(crate) fn focus_list(&mut self) {
        self.search_focused = false;
    }

    pub(crate) fn edit_query(&mut self, key: crossterm::event::KeyEvent) -> bool {
        self.query.edit_key(key)
    }

    pub(crate) fn paste_query(&mut self, text: &str) {
        self.query.insert_text(text);
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub(crate) fn clear_query(&mut self) -> bool {
        if self.query.text().is_empty() {
            return false;
        }
        self.query.clear();
        self.selected = 0;
        self.scroll_offset = 0;
        true
    }

    pub(crate) fn go_back(&mut self) -> bool {
        if self.editor.take().is_some() {
            return true;
        }
        match self.page {
            SettingsPage::Root => false,
            SettingsPage::Category(_) => {
                self.enter_page(SettingsPage::Root);
                true
            }
            SettingsPage::Models => {
                self.enter_page(SettingsPage::Category(Category::Models));
                true
            }
            SettingsPage::Search => {
                self.enter_page(SettingsPage::Category(Category::Tools));
                true
            }
            SettingsPage::SessionMemoryTemplates => {
                self.enter_page(SettingsPage::Category(Category::Advanced));
                true
            }
        }
    }

    pub(crate) fn request_back(&mut self) -> SettingsInput {
        let destination = match self.page {
            SettingsPage::Root => return self.request_close(),
            SettingsPage::Category(_) => SettingsPage::Root,
            SettingsPage::Models => SettingsPage::Category(Category::Models),
            SettingsPage::Search => SettingsPage::Category(Category::Tools),
            SettingsPage::SessionMemoryTemplates => SettingsPage::Category(Category::Advanced),
        };
        let has_page_draft = match self.page {
            SettingsPage::Search => self.search.is_dirty(),
            SettingsPage::SessionMemoryTemplates => self.session_memory.is_dirty(),
            SettingsPage::Root | SettingsPage::Category(_) | SettingsPage::Models => false,
        };
        if !has_page_draft {
            self.enter_page(destination);
            return SettingsInput::Redraw;
        }
        self.editor = Some(SettingsEditor::Confirm {
            title: "Discard unsaved changes?".to_string(),
            message: "This page contains changes that have not been written to your user config."
                .to_string(),
            confirm_label: "Discard and go back".to_string(),
            action: SettingsConfirmAction::DiscardAndBack { destination },
        });
        SettingsInput::Redraw
    }

    pub(super) fn discard_page_draft(&mut self, page: SettingsPage) {
        match page {
            SettingsPage::Search => self.search = SearchPageState::new(&self.store),
            SettingsPage::SessionMemoryTemplates => {
                self.session_memory = SessionMemoryPageState::new(&self.store);
            }
            SettingsPage::Root | SettingsPage::Category(_) | SettingsPage::Models => {}
        }
    }

    pub(super) fn enter_page(&mut self, page: SettingsPage) {
        self.page = page;
        self.selected = 0;
        self.scroll_offset = 0;
        self.query.clear();
        self.search_focused = false;
        self.pointer.clear_hover();
        self.notice = None;
        self.notice_is_error = false;
    }
}
