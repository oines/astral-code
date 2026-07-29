//! Action registry for Astral's main prompt and transcript surfaces.
//!
//! Ported from Grok Build's `xai-grok-pager` action registry design. The
//! registered keys remain Astral presentation input only; executing an action
//! continues to use Astral's existing app-server and core semantics.

mod defaults;
mod key;

use std::sync::LazyLock;

use crossterm::event::KeyEvent;

use key::KeyShortcut;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ActionId {
    CycleMode,
    ShortcutsHelp,
    PageUp,
    PageDown,
    FocusScrollback,
    SendPrompt,
    PromptCancel,
    ExitEmptyPrompt,
    CopyLastResponse,
    OpenTranscriptSearch,
    FocusPrompt,
    PreviousTurn,
    NextTurn,
    NextResponse,
    PreviousResponse,
    GoToTop,
    GoToBottom,
    ScrollLineUp,
    ScrollLineDown,
    HalfPageUp,
    HalfPageDown,
    SelectNext,
    SelectPrevious,
    CollapseEntry,
    ExpandEntry,
    ToggleEntry,
    ToggleAllEntries,
    ToggleAllReasoning,
    ToggleRawMarkdown,
    CopyBlockContent,
    CopyBlockMetadata,
    NextLink,
    PreviousLink,
    OpenEntry,
    ScrollbackCancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum When {
    Always,
    PromptFocused,
    ScrollbackFocused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Category {
    GettingStarted,
    Input,
    ConversationNavigation,
    ConversationActions,
    Session,
}

impl Category {
    pub(crate) const ORDER: [Self; 5] = [
        Self::GettingStarted,
        Self::Input,
        Self::ConversationNavigation,
        Self::ConversationActions,
        Self::Session,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::GettingStarted => "Getting started",
            Self::Input => "Input",
            Self::ConversationNavigation => "Conversation navigation",
            Self::ConversationActions => "Conversation actions",
            Self::Session => "Session",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ActionDef {
    pub(crate) id: ActionId,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
    pub(crate) long_help: Option<&'static str>,
    default_key: KeyShortcut,
    alternate_keys: Vec<KeyShortcut>,
    pub(crate) category: Category,
    pub(crate) context: When,
    hint_key_display: Option<&'static str>,
}

impl ActionDef {
    fn new(
        id: ActionId,
        label: &'static str,
        description: &'static str,
        default_key: KeyShortcut,
        alternate_keys: Vec<KeyShortcut>,
        category: Category,
        context: When,
    ) -> Self {
        Self {
            id,
            label,
            description,
            long_help: None,
            default_key,
            alternate_keys,
            category,
            context,
            hint_key_display: None,
        }
    }

    fn with_key_display(mut self, key_display: &'static str) -> Self {
        self.hint_key_display = Some(key_display);
        self
    }

    fn with_help(mut self, long_help: &'static str) -> Self {
        self.long_help = Some(long_help);
        self
    }

    pub(crate) fn key_display(&self) -> String {
        let mut seen = std::collections::HashSet::new();
        std::iter::once(self.default_key)
            .chain(self.alternate_keys.iter().copied())
            .map(KeyShortcut::display_pretty)
            .filter(|display| seen.insert(display.clone()))
            .collect::<Vec<_>>()
            .join(" / ")
    }

    pub(crate) fn hint_key(&self) -> &'static str {
        let Some(display) = self.hint_key_display else {
            panic!("footer actions must define a stable key display");
        };
        display
    }

    fn matches(&self, key: &KeyEvent) -> bool {
        self.default_key.matches(key)
            || self
                .alternate_keys
                .iter()
                .any(|shortcut| shortcut.matches(key))
    }
}

#[derive(Debug)]
struct ActionRegistry {
    actions: Vec<ActionDef>,
}

impl ActionRegistry {
    fn defaults() -> Self {
        Self {
            actions: defaults::default_actions(),
        }
    }

    fn lookup(&self, key: &KeyEvent, context: When) -> Option<ActionId> {
        self.actions
            .iter()
            .find(|definition| definition.context == context && definition.matches(key))
            .map(|definition| definition.id)
            .or_else(|| {
                self.actions
                    .iter()
                    .find(|definition| {
                        definition.context == When::Always && definition.matches(key)
                    })
                    .map(|definition| definition.id)
            })
    }
}

static ACTIONS: LazyLock<ActionRegistry> = LazyLock::new(ActionRegistry::defaults);

pub(crate) fn lookup(key: &KeyEvent, context: When) -> Option<ActionId> {
    ACTIONS.lookup(key, context)
}

pub(crate) fn definitions() -> &'static [ActionDef] {
    &ACTIONS.actions
}

pub(crate) fn definition(id: ActionId, context: When) -> &'static ActionDef {
    ACTIONS
        .actions
        .iter()
        .find(|definition| {
            definition.id == id
                && (definition.context == context || definition.context == When::Always)
        })
        .unwrap_or_else(|| panic!("missing action definition for {id:?} in {context:?}"))
}
