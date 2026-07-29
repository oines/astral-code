//! Searchable command palette backed by Astral's action and slash registries.

use crate::actions;
use crate::actions::ActionId;
use crate::actions::When;
use crate::modal::ModalPointerState;
use crate::slash::SlashCommandId;
use crate::slash::SlashPaletteEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandPaletteCommand {
    CycleMode,
    ToggleMultiline,
    ShellMode,
    OpenShortcuts,
    ToggleQueue,
    EditPrompt,
    CopyResponse,
    Slash {
        command: SlashCommandId,
        name: &'static str,
        insert_text: String,
        requires_input: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandPaletteEntry {
    Section(&'static str),
    Command {
        label: String,
        shortcut: String,
        command: CommandPaletteCommand,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct CommandPaletteState {
    entries: Vec<CommandPaletteEntry>,
    query: String,
    selected: usize,
    pub(crate) scroll_offset: usize,
    pub(crate) pointer: ModalPointerState,
}

impl CommandPaletteState {
    pub(crate) fn new(slash_entries: Vec<SlashPaletteEntry>) -> Self {
        let mut entries = vec![CommandPaletteEntry::Section("Actions")];
        entries.extend([
            action_entry(ActionId::CycleMode, CommandPaletteCommand::CycleMode),
            action_entry(
                ActionId::ToggleMultiline,
                CommandPaletteCommand::ToggleMultiline,
            ),
            action_entry(ActionId::ShellMode, CommandPaletteCommand::ShellMode),
            action_entry(
                ActionId::ShortcutsHelp,
                CommandPaletteCommand::OpenShortcuts,
            ),
            action_entry(ActionId::ToggleQueue, CommandPaletteCommand::ToggleQueue),
            action_entry(
                ActionId::OpenExternalEditor,
                CommandPaletteCommand::EditPrompt,
            ),
            action_entry(
                ActionId::CopyLastResponse,
                CommandPaletteCommand::CopyResponse,
            ),
        ]);
        entries.push(CommandPaletteEntry::Section("Slash commands"));
        entries.extend(
            slash_entries
                .into_iter()
                .map(|entry| CommandPaletteEntry::Command {
                    label: entry.description.to_string(),
                    shortcut: format!("/{}", entry.name),
                    command: CommandPaletteCommand::Slash {
                        command: entry.command,
                        name: entry.name,
                        insert_text: entry.insert_text,
                        requires_input: entry.requires_input,
                    },
                }),
        );
        let mut state = Self {
            entries,
            query: String::new(),
            selected: 0,
            scroll_offset: 0,
            pointer: ModalPointerState::default(),
        };
        state.select_first();
        state
    }

    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    pub(crate) fn selected(&self) -> usize {
        self.selected
    }

    pub(crate) fn visible_indices(&self) -> Vec<usize> {
        if self.query.is_empty() {
            return (0..self.entries.len()).collect();
        }
        let query = self.query.to_lowercase();
        let mut visible = Vec::new();
        let mut section = None;
        for (index, entry) in self.entries.iter().enumerate() {
            match entry {
                CommandPaletteEntry::Section(_) => section = Some(index),
                CommandPaletteEntry::Command {
                    label, shortcut, ..
                } if label.to_lowercase().contains(&query)
                    || shortcut.to_lowercase().contains(&query) =>
                {
                    if let Some(section) = section.take() {
                        visible.push(section);
                    }
                    visible.push(index);
                }
                CommandPaletteEntry::Command { .. } => {}
            }
        }
        visible
    }

    pub(crate) fn entry(&self, index: usize) -> Option<&CommandPaletteEntry> {
        self.entries.get(index)
    }

    pub(crate) fn selected_command(&self) -> Option<CommandPaletteCommand> {
        let index = *self.visible_indices().get(self.selected)?;
        match self.entries.get(index)? {
            CommandPaletteEntry::Command { command, .. } => Some(command.clone()),
            CommandPaletteEntry::Section(_) => None,
        }
    }

    pub(crate) fn insert_query(&mut self, character: char) {
        self.query.push(character);
        self.select_first();
    }

    pub(crate) fn paste_query(&mut self, text: &str) {
        self.query
            .extend(text.chars().filter(|character| !character.is_control()));
        self.select_first();
    }

    pub(crate) fn backspace_query(&mut self) {
        self.query.pop();
        self.select_first();
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let visible = self.visible_indices();
        let direction = delta.signum();
        if visible.is_empty() || direction == 0 {
            self.selected = 0;
            return;
        }
        let mut selected = self.selected.min(visible.len().saturating_sub(1));
        for _ in 0..delta.unsigned_abs() {
            for _ in 0..visible.len() {
                selected =
                    (selected as isize + direction).rem_euclid(visible.len() as isize) as usize;
                if self
                    .entries
                    .get(visible[selected])
                    .is_some_and(|entry| matches!(entry, CommandPaletteEntry::Command { .. }))
                {
                    break;
                }
            }
        }
        self.selected = selected;
    }

    pub(crate) fn select(&mut self, row: usize) {
        let visible = self.visible_indices();
        if visible.get(row).is_some_and(|index| {
            self.entries
                .get(*index)
                .is_some_and(|entry| matches!(entry, CommandPaletteEntry::Command { .. }))
        }) {
            self.selected = row;
        }
    }

    pub(crate) fn select_start(&mut self) {
        self.select_first();
    }

    pub(crate) fn select_end(&mut self) {
        self.selected = self
            .visible_indices()
            .iter()
            .rposition(|index| {
                self.entries
                    .get(*index)
                    .is_some_and(|entry| matches!(entry, CommandPaletteEntry::Command { .. }))
            })
            .unwrap_or_default();
    }

    pub(crate) fn ensure_selection_visible(&mut self, height: usize) {
        if height == 0 {
            self.scroll_offset = self.selected;
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset.saturating_add(height) {
            self.scroll_offset = self.selected.saturating_add(1).saturating_sub(height);
        }
        self.scroll_offset = self
            .scroll_offset
            .min(self.visible_indices().len().saturating_sub(height));
    }

    fn select_first(&mut self) {
        self.selected = self
            .visible_indices()
            .iter()
            .position(|index| {
                self.entries
                    .get(*index)
                    .is_some_and(|entry| matches!(entry, CommandPaletteEntry::Command { .. }))
            })
            .unwrap_or_default();
        self.scroll_offset = 0;
    }
}

fn action_entry(id: ActionId, command: CommandPaletteCommand) -> CommandPaletteEntry {
    let definition = actions::definition(id, When::PromptFocused);
    CommandPaletteEntry::Command {
        label: definition.description.to_string(),
        shortcut: definition.key_display(),
        command,
    }
}
