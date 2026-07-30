use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;

use super::ActionDef;
use super::ActionId;
use super::Category;
use super::When;
use super::key::KeyShortcut;
use super::key::shift_tab_keys;

pub(super) fn default_actions() -> Vec<ActionDef> {
    let shift_tab = shift_tab_keys();
    vec![
        ActionDef::new(
            ActionId::CommandPalette,
            "commands",
            "Search commands and actions",
            KeyShortcut::control('p'),
            Vec::new(),
            Category::GettingStarted,
            When::Always,
        )
        .with_key_display("Ctrl+P")
        .with_help(
            "Opens Astral's searchable command palette without changing the current draft. \
             Ctrl+P works from the prompt or transcript; ? also opens it while the transcript \
             is focused.",
        ),
        ActionDef::new(
            ActionId::CycleMode,
            "mode",
            "Cycle collaboration mode",
            shift_tab[0],
            shift_tab[1..].to_vec(),
            Category::Session,
            When::Always,
        )
        .with_key_display("Shift+Tab")
        .with_help(
            "Cycles through Astral's collaboration modes for the current thread. \
             The selected mode is sent through Astral's existing thread settings.",
        ),
        ActionDef::new(
            ActionId::ToggleMultiline,
            "multiline",
            "Toggle multiline prompt mode",
            KeyShortcut::control('m'),
            Vec::new(),
            Category::Input,
            When::PromptFocused,
        )
        .with_key_display("Ctrl+M")
        .with_help(
            "Swaps the prompt's Enter behavior for this session. While multiline mode is on, \
             Enter inserts a newline and Shift+Enter or Alt+Enter sends the prompt.",
        ),
        ActionDef::new(
            ActionId::ModelPicker,
            "model",
            "Pick model",
            KeyShortcut::control('m'),
            Vec::new(),
            Category::Session,
            When::ScrollbackFocused,
        )
        .with_key_display("Ctrl+M")
        .with_help(
            "Opens the model picker without changing the current draft. \
             Ctrl+M toggles multiline while the prompt is focused and opens \
             this picker while the transcript is focused.",
        ),
        ActionDef::new(
            ActionId::OpenSessions,
            "sessions",
            "Open saved sessions",
            KeyShortcut::control('s'),
            Vec::new(),
            Category::Session,
            When::Always,
        )
        .with_key_display("Ctrl+S")
        .with_help(
            "Opens Astral's existing resume picker without changing the current draft. \
             Select a saved conversation to switch to its full history.",
        ),
        ActionDef::new(
            ActionId::ShellMode,
            "shell",
            "Shell mode",
            KeyShortcut::character('!'),
            Vec::new(),
            Category::Input,
            When::PromptFocused,
        )
        .with_key_display("!")
        .with_help(
            "Type ! on an empty prompt to run a local shell command without leaving Astral. \
             Enter runs the command through Astral's existing thread shell request; \
             Backspace or Esc leaves an empty shell prompt.",
        ),
        ActionDef::new(
            ActionId::ShortcutsHelp,
            "shortcuts",
            "Keyboard shortcuts",
            KeyShortcut::with_required_modifiers(KeyCode::Char('.'), KeyModifiers::CONTROL),
            vec![KeyShortcut::with_required_modifiers(
                KeyCode::Char('x'),
                KeyModifiers::CONTROL,
            )],
            Category::GettingStarted,
            When::Always,
        )
        .with_key_display("Ctrl+.")
        .with_help(
            "Opens the keyboard shortcuts window. The definitions shown there \
             are the same definitions used to dispatch keys.",
        ),
        ActionDef::new(
            ActionId::ToggleQueue,
            "queue",
            "Toggle prompt queue",
            KeyShortcut::control(';'),
            vec![KeyShortcut::control('\''), KeyShortcut::control('4')],
            Category::ConversationActions,
            When::Always,
        )
        .with_key_display("Ctrl+;")
        .with_help(
            "Shows or hides the queued follow-up pane. Enter queues while a turn \
             is running; queued prompts run in order after the active turn.",
        ),
        ActionDef::new(
            ActionId::PageUp,
            "page up",
            "Scroll transcript up one page",
            KeyShortcut::with_any_modifiers(KeyCode::PageUp),
            Vec::new(),
            Category::ConversationNavigation,
            When::Always,
        )
        .with_key_display("Page Up"),
        ActionDef::new(
            ActionId::PageDown,
            "page down",
            "Scroll transcript down one page",
            KeyShortcut::with_any_modifiers(KeyCode::PageDown),
            Vec::new(),
            Category::ConversationNavigation,
            When::Always,
        )
        .with_key_display("Page Down"),
        ActionDef::new(
            ActionId::FocusScrollback,
            "scrollback",
            "Focus transcript",
            KeyShortcut::plain(KeyCode::Tab),
            Vec::new(),
            Category::GettingStarted,
            When::PromptFocused,
        )
        .with_key_display("Tab"),
        ActionDef::new(
            ActionId::SendPrompt,
            "send",
            "Send prompt",
            KeyShortcut::plain(KeyCode::Enter),
            vec![KeyShortcut::new(KeyCode::Enter, KeyModifiers::SUPER)],
            Category::Input,
            When::PromptFocused,
        )
        .with_key_display("Enter"),
        ActionDef::new(
            ActionId::InterjectPrompt,
            "send now",
            "Send now to the active turn",
            KeyShortcut::new(KeyCode::Enter, KeyModifiers::CONTROL),
            vec![KeyShortcut::control('i')],
            Category::Input,
            When::PromptFocused,
        )
        .with_key_display("Ctrl+Enter")
        .with_help(
            "Uses Astral's existing turn/steer request to add the draft to the \
             active turn. With an empty draft, sends the next queued follow-up; \
             from the queue pane it sends the selected row.",
        ),
        ActionDef::new(
            ActionId::OpenExternalEditor,
            "edit prompt",
            "Edit the current draft in an external editor",
            KeyShortcut::control('g'),
            Vec::new(),
            Category::Input,
            When::PromptFocused,
        )
        .with_key_display("Ctrl+G")
        .with_help(
            "Opens the current plain-text draft in $VISUAL or $EDITOR, falling back to vi. \
             Saving and closing the editor returns the updated text to the prompt without \
             sending it.",
        ),
        ActionDef::new(
            ActionId::PromptCancel,
            "interrupt",
            "Interrupt, clear draft, or exit while idle",
            KeyShortcut::control('c'),
            Vec::new(),
            Category::Session,
            When::PromptFocused,
        )
        .with_key_display("Ctrl+C"),
        ActionDef::new(
            ActionId::ExitEmptyPrompt,
            "exit",
            "Exit when the prompt is empty",
            KeyShortcut::control('d'),
            Vec::new(),
            Category::Session,
            When::PromptFocused,
        )
        .with_key_display("Ctrl+D"),
        ActionDef::new(
            ActionId::CopyLastResponse,
            "copy response",
            "Copy the last agent response",
            KeyShortcut::control('o'),
            Vec::new(),
            Category::ConversationActions,
            When::PromptFocused,
        )
        .with_key_display("Ctrl+O"),
        ActionDef::new(
            ActionId::OpenTranscriptSearch,
            "search",
            "Search transcript",
            KeyShortcut::character('/'),
            Vec::new(),
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("/"),
        ActionDef::new(
            ActionId::FocusPrompt,
            "prompt",
            "Focus prompt",
            KeyShortcut::plain(KeyCode::Tab),
            vec![KeyShortcut::character('i'), KeyShortcut::character(' ')],
            Category::GettingStarted,
            When::ScrollbackFocused,
        )
        .with_key_display("Tab"),
        ActionDef::new(
            ActionId::PreviousTurn,
            "previous turn",
            "Select previous turn",
            KeyShortcut::character('H'),
            vec![KeyShortcut::shift(KeyCode::Left)],
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("Shift+H / Shift+←"),
        ActionDef::new(
            ActionId::NextTurn,
            "next turn",
            "Select next turn",
            KeyShortcut::character('L'),
            vec![KeyShortcut::shift(KeyCode::Right)],
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("Shift+L / Shift+→"),
        ActionDef::new(
            ActionId::NextResponse,
            "next response",
            "Select next final response",
            KeyShortcut::character('J'),
            Vec::new(),
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("Shift+J"),
        ActionDef::new(
            ActionId::PreviousResponse,
            "previous response",
            "Select previous final response",
            KeyShortcut::character('K'),
            Vec::new(),
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("Shift+K"),
        ActionDef::new(
            ActionId::GoToTop,
            "top",
            "Go to transcript top",
            KeyShortcut::character('g'),
            Vec::new(),
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("g"),
        ActionDef::new(
            ActionId::GoToBottom,
            "bottom",
            "Go to transcript bottom",
            KeyShortcut::character('G'),
            Vec::new(),
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("Shift+G"),
        ActionDef::new(
            ActionId::ScrollLineUp,
            "line up",
            "Scroll transcript up one line",
            KeyShortcut::control('k'),
            Vec::new(),
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("Ctrl+K"),
        ActionDef::new(
            ActionId::ScrollLineDown,
            "line down",
            "Scroll transcript down one line",
            KeyShortcut::control('j'),
            Vec::new(),
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("Ctrl+J"),
        ActionDef::new(
            ActionId::HalfPageUp,
            "half page up",
            "Scroll transcript up half a page",
            KeyShortcut::control('u'),
            Vec::new(),
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("Ctrl+U"),
        ActionDef::new(
            ActionId::HalfPageDown,
            "half page down",
            "Scroll transcript down half a page",
            KeyShortcut::control('d'),
            Vec::new(),
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("Ctrl+D"),
        ActionDef::new(
            ActionId::SelectNext,
            "next entry",
            "Select next transcript entry",
            KeyShortcut::character('j'),
            vec![KeyShortcut::with_any_modifiers(KeyCode::Down)],
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("j / ↓"),
        ActionDef::new(
            ActionId::SelectPrevious,
            "previous entry",
            "Select previous transcript entry",
            KeyShortcut::character('k'),
            vec![KeyShortcut::with_any_modifiers(KeyCode::Up)],
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("k / ↑"),
        ActionDef::new(
            ActionId::CollapseEntry,
            "collapse",
            "Collapse selected entry",
            KeyShortcut::character('h'),
            vec![KeyShortcut::with_any_modifiers(KeyCode::Left)],
            Category::ConversationActions,
            When::ScrollbackFocused,
        )
        .with_key_display("h / ←"),
        ActionDef::new(
            ActionId::ExpandEntry,
            "expand",
            "Expand selected entry",
            KeyShortcut::character('l'),
            vec![KeyShortcut::with_any_modifiers(KeyCode::Right)],
            Category::ConversationActions,
            When::ScrollbackFocused,
        )
        .with_key_display("l / →"),
        ActionDef::new(
            ActionId::ToggleEntry,
            "fold",
            "Expand or collapse selected entry",
            KeyShortcut::character('e'),
            Vec::new(),
            Category::ConversationActions,
            When::ScrollbackFocused,
        )
        .with_key_display("e")
        .with_help(
            "Folds or unfolds the selected transcript entry. Enter is separate: \
             it opens the focused entry, link, or subagent view.",
        ),
        ActionDef::new(
            ActionId::ToggleAllEntries,
            "all entries",
            "Expand or collapse all entries",
            KeyShortcut::character('E'),
            Vec::new(),
            Category::ConversationActions,
            When::ScrollbackFocused,
        )
        .with_key_display("Shift+E"),
        ActionDef::new(
            ActionId::ToggleAllReasoning,
            "reasoning",
            "Expand or collapse all reasoning entries",
            KeyShortcut::control('e'),
            Vec::new(),
            Category::ConversationActions,
            When::ScrollbackFocused,
        )
        .with_key_display("Ctrl+e")
        .with_help(
            "Changes only the presentation state of Astral Reasoning entries. \
             It does not alter the thread or its protocol data.",
        ),
        ActionDef::new(
            ActionId::ToggleRawMarkdown,
            "raw markdown",
            "Toggle raw Markdown for the selected message",
            KeyShortcut::character('r'),
            Vec::new(),
            Category::ConversationActions,
            When::ScrollbackFocused,
        )
        .with_key_display("r"),
        ActionDef::new(
            ActionId::CopyBlockContent,
            "copy",
            "Copy selected block content",
            KeyShortcut::character('y'),
            Vec::new(),
            Category::ConversationActions,
            When::ScrollbackFocused,
        )
        .with_key_display("y"),
        ActionDef::new(
            ActionId::CopyBlockMetadata,
            "copy metadata",
            "Copy selected command, path, or query",
            KeyShortcut::character('Y'),
            Vec::new(),
            Category::ConversationActions,
            When::ScrollbackFocused,
        )
        .with_key_display("Y"),
        ActionDef::new(
            ActionId::NextLink,
            "next link",
            "Select next visible link",
            KeyShortcut::character('o'),
            Vec::new(),
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("o/O"),
        ActionDef::new(
            ActionId::PreviousLink,
            "previous link",
            "Select previous visible link",
            KeyShortcut::character('O'),
            Vec::new(),
            Category::ConversationNavigation,
            When::ScrollbackFocused,
        )
        .with_key_display("Shift+O"),
        ActionDef::new(
            ActionId::OpenEntry,
            "open",
            "Open selected entry, link, or subagent",
            KeyShortcut::plain(KeyCode::Enter),
            vec![KeyShortcut::with_required_modifiers(
                KeyCode::Char('f'),
                KeyModifiers::CONTROL,
            )],
            Category::ConversationActions,
            When::ScrollbackFocused,
        )
        .with_key_display("Enter"),
        ActionDef::new(
            ActionId::ScrollbackCancel,
            "interrupt",
            "Interrupt the running turn or exit while idle",
            KeyShortcut::with_required_modifiers(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Vec::new(),
            Category::Session,
            When::ScrollbackFocused,
        )
        .with_key_display("Ctrl+C"),
    ]
}
