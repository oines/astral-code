//! Astral slash-command discovery and invocation parsing.
//!
//! The interaction model is derived from Grok Build's `SlashController` at
//! commit `47348d13ec4508dcfe440e34c6d511bb02998fb2` (Apache-2.0). Command
//! semantics remain Astral-owned and are dispatched through app-server v2.

use std::collections::HashMap;

use codex_app_server_protocol::Model;

use crate::model_command::ModelCatalog;
use crate::model_command::ModelResolveError;
use crate::model_command::ModelSelection;

const MAX_MATCHES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlashCommandId {
    Model,
    Compact,
    New,
    Resume,
    Fork,
    Rename,
    Status,
    Copy,
    Exit,
    Quit,
    Permissions,
    Plan,
    Theme,
    Timeline,
    Mcp,
    Skills,
    Hooks,
    Apps,
    Plugins,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Args {
    None,
    Optional(&'static str),
    Required(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlashCommandState {
    Idle,
    Working,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandAvailability {
    Always,
    Connected,
    Idle,
}

impl CommandAvailability {
    fn allows(self, state: SlashCommandState) -> bool {
        match (self, state) {
            (
                Self::Always,
                SlashCommandState::Idle
                | SlashCommandState::Working
                | SlashCommandState::Disconnected,
            )
            | (Self::Connected, SlashCommandState::Idle | SlashCommandState::Working)
            | (Self::Idle, SlashCommandState::Idle) => true,
            (Self::Connected | Self::Idle, SlashCommandState::Disconnected)
            | (Self::Idle, SlashCommandState::Working) => false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CommandSpec {
    id: SlashCommandId,
    name: &'static str,
    description: &'static str,
    args: Args,
    availability: CommandAvailability,
}

macro_rules! command {
    ($id:ident, $name:literal, $description:literal, $args:expr, $availability:ident) => {
        CommandSpec {
            id: SlashCommandId::$id,
            name: $name,
            description: $description,
            args: $args,
            availability: CommandAvailability::$availability,
        }
    };
}

const COMMANDS: &[CommandSpec] = &[
    command!(
        Model,
        "model",
        "Choose model and reasoning effort",
        Args::Required("model"),
        Idle
    ),
    command!(
        Permissions,
        "permissions",
        "Choose what Astral can do",
        Args::None,
        Idle
    ),
    command!(
        Compact,
        "compact",
        "Compact the current conversation",
        Args::None,
        Idle
    ),
    command!(
        Plan,
        "plan",
        "Switch mode and optionally start a planning task",
        Args::Optional("prompt"),
        Idle
    ),
    command!(New, "new", "Start a new conversation", Args::None, Idle),
    command!(
        Resume,
        "resume",
        "Resume a saved conversation",
        Args::None,
        Idle
    ),
    command!(
        Fork,
        "fork",
        "Fork the current conversation",
        Args::None,
        Idle
    ),
    command!(
        Rename,
        "rename",
        "Rename this conversation",
        Args::Required("name"),
        Connected
    ),
    command!(
        Status,
        "status",
        "Show session and context status",
        Args::None,
        Always
    ),
    command!(Copy, "copy", "Copy the last response", Args::None, Always),
    command!(
        Theme,
        "theme",
        "Choose the Astral theme",
        Args::Optional("theme"),
        Always
    ),
    command!(
        Timeline,
        "timeline",
        "Toggle the timeline rail",
        Args::None,
        Always
    ),
    command!(
        Mcp,
        "mcp",
        "Show configured MCP servers",
        Args::Optional("verbose"),
        Connected
    ),
    command!(
        Skills,
        "skills",
        "Browse available skills",
        Args::None,
        Connected
    ),
    command!(
        Hooks,
        "hooks",
        "View lifecycle hooks",
        Args::None,
        Connected
    ),
    command!(Apps, "apps", "Manage connected apps", Args::None, Connected),
    command!(Plugins, "plugins", "Browse plugins", Args::None, Connected),
    command!(Exit, "exit", "Exit Astral", Args::None, Always),
    command!(Quit, "quit", "Exit Astral", Args::None, Always),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashSuggestion {
    pub command: SlashCommandId,
    pub display: String,
    pub description: String,
    pub insert_text: String,
    pub indices: Vec<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlashSnapshot {
    pub active: bool,
    pub open: bool,
    pub title: &'static str,
    pub query: String,
    pub matches: Vec<SlashSuggestion>,
    pub selected: usize,
    pub ghost: Option<String>,
    pub recognized: bool,
}

impl SlashSnapshot {
    pub fn selection(&self) -> Option<&SlashSuggestion> {
        self.matches
            .get(self.selected.min(self.matches.len().saturating_sub(1)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashInvocation {
    pub command: SlashCommandId,
    pub name: &'static str,
    pub args: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashError {
    Unknown(String),
    UnavailableWhileWorking(String),
    RequiresConnection(String),
    MissingArgument {
        command: String,
        placeholder: &'static str,
    },
}

impl std::fmt::Display for SlashError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(command) => write!(formatter, "Unknown command: /{command}"),
            Self::UnavailableWhileWorking(command) => {
                write!(
                    formatter,
                    "/{command} is unavailable while Astral is working"
                )
            }
            Self::RequiresConnection(command) => {
                write!(formatter, "/{command} requires an app-server connection")
            }
            Self::MissingArgument {
                command,
                placeholder,
            } => write!(formatter, "Usage: /{command} <{placeholder}>"),
        }
    }
}

#[derive(Debug, Default)]
pub struct SlashController {
    snapshot: SlashSnapshot,
    mru: HashMap<SlashCommandId, u64>,
    clock: u64,
    models: ModelCatalog,
}

impl SlashController {
    pub fn snapshot(&self) -> &SlashSnapshot {
        &self.snapshot
    }

    pub fn refresh(&mut self, text: &str, state: SlashCommandState) {
        let previous = self.snapshot.selection().map(|row| row.insert_text.clone());
        let Some((query, has_args)) = leading_query(text) else {
            self.snapshot = SlashSnapshot::default();
            return;
        };
        let exact = find_spec(query);
        if has_args
            && exact.is_some_and(|spec| {
                spec.id == SlashCommandId::Model && spec.availability.allows(state)
            })
        {
            let args = parse_invocation(text)
                .map(|(_, args)| args)
                .unwrap_or_default();
            let matches = self
                .models
                .suggestions(args)
                .into_iter()
                .take(MAX_MATCHES)
                .map(|suggestion| SlashSuggestion {
                    command: SlashCommandId::Model,
                    display: suggestion.display,
                    description: suggestion.description,
                    insert_text: suggestion.insert_text,
                    indices: Vec::new(),
                })
                .collect::<Vec<_>>();
            let selected = previous
                .and_then(|insert_text| {
                    matches
                        .iter()
                        .position(|row| row.insert_text == insert_text)
                })
                .unwrap_or_default();
            self.snapshot = SlashSnapshot {
                active: true,
                open: !matches.is_empty(),
                title: "models",
                query: args.to_string(),
                matches,
                selected,
                ghost: None,
                recognized: self.models.resolve(args).is_ok(),
            };
            return;
        }
        let mut ranked = COMMANDS
            .iter()
            .enumerate()
            .filter(|(_, spec)| spec.availability.allows(state))
            .filter_map(|(order, spec)| {
                fuzzy_match(spec.name, query).map(|(score, indices)| {
                    let recency = self.mru.get(&spec.id).copied().unwrap_or_default();
                    (spec, score, recency, order, indices)
                })
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| b.2.cmp(&a.2))
                .then_with(|| a.3.cmp(&b.3))
        });
        let matches = ranked
            .into_iter()
            .take(MAX_MATCHES)
            .map(|(spec, _, _, _, indices)| SlashSuggestion {
                command: spec.id,
                display: format!("/{}", spec.name),
                description: spec.description.to_string(),
                insert_text: match spec.args {
                    Args::None => format!("/{}", spec.name),
                    Args::Optional(_) | Args::Required(_) => format!("/{} ", spec.name),
                },
                indices,
            })
            .collect::<Vec<_>>();
        let selected = previous
            .and_then(|insert_text| {
                matches
                    .iter()
                    .position(|row| row.insert_text == insert_text)
            })
            .unwrap_or_default();
        let ghost = (!has_args)
            .then(|| matches.get(selected))
            .flatten()
            .and_then(|row| prefix_ghost(query, &row.display[1..]));
        self.snapshot = SlashSnapshot {
            active: true,
            open: !has_args && !matches.is_empty(),
            title: "commands",
            query: query.to_string(),
            matches,
            selected,
            ghost,
            recognized: exact.is_some_and(|spec| {
                spec.availability.allows(state)
                    && (!matches!(spec.args, Args::Required(_)) || has_args)
            }),
        };
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = self.snapshot.matches.len();
        if len == 0 {
            return;
        }
        self.snapshot.selected =
            (self.snapshot.selected as isize + delta).rem_euclid(len as isize) as usize;
        self.refresh_ghost();
    }

    pub(crate) fn select(&mut self, index: usize) {
        if self.snapshot.matches.is_empty() {
            return;
        }
        self.snapshot.selected = index.min(self.snapshot.matches.len().saturating_sub(1));
        self.refresh_ghost();
    }

    fn refresh_ghost(&mut self) {
        self.snapshot.ghost = self
            .snapshot
            .selection()
            .and_then(|row| prefix_ghost(&self.snapshot.query, &row.display[1..]));
    }

    pub fn close(&mut self) {
        self.snapshot.open = false;
    }

    pub fn accept_selection(&mut self, state: SlashCommandState) -> Option<String> {
        let insert_text = self
            .snapshot
            .selection()
            .map(|selection| selection.insert_text.clone())?;
        self.refresh(&insert_text, state);
        Some(insert_text)
    }

    pub fn invocation(
        &self,
        text: &str,
        state: SlashCommandState,
    ) -> Result<Option<SlashInvocation>, SlashError> {
        let Some((name, args)) = parse_invocation(text) else {
            return Ok(None);
        };
        let Some(spec) = find_spec(name) else {
            return Err(SlashError::Unknown(name.to_string()));
        };
        if !spec.availability.allows(state) {
            return Err(match state {
                SlashCommandState::Working => SlashError::UnavailableWhileWorking(name.to_string()),
                SlashCommandState::Disconnected => SlashError::RequiresConnection(name.to_string()),
                SlashCommandState::Idle => unreachable!("idle commands are available while idle"),
            });
        }
        if let Args::Required(placeholder) = spec.args
            && args.is_empty()
        {
            return Err(SlashError::MissingArgument {
                command: name.to_string(),
                placeholder,
            });
        }
        Ok(Some(SlashInvocation {
            command: spec.id,
            name: spec.name,
            args: args.to_string(),
        }))
    }

    pub fn record(&mut self, command: SlashCommandId) {
        self.clock = self.clock.saturating_add(1);
        self.mru.insert(command, self.clock);
    }

    pub fn set_models(
        &mut self,
        models: Vec<Model>,
        current_model: impl Into<String>,
        current_provider: impl Into<String>,
    ) {
        self.models.replace(models, current_model, current_provider);
    }

    pub fn update_current_model(
        &mut self,
        model: impl Into<String>,
        model_provider: impl Into<String>,
    ) {
        self.models.update_current(model, model_provider);
    }

    pub fn resolve_model(&self, args: &str) -> Result<ModelSelection, ModelResolveError> {
        self.models.resolve(args)
    }
}

fn leading_query(text: &str) -> Option<(&str, bool)> {
    let rest = text.strip_prefix('/')?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    Some((&rest[..end], end < rest.len()))
}

fn parse_invocation(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix('/')?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let name = &rest[..end];
    (!name.is_empty()).then(|| (name, rest[end..].trim()))
}

fn find_spec(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS
        .iter()
        .find(|spec| spec.name.eq_ignore_ascii_case(name))
}

fn prefix_ghost(query: &str, candidate: &str) -> Option<String> {
    candidate
        .get(query.len()..)
        .filter(|suffix| !suffix.is_empty() && candidate[..query.len()].eq_ignore_ascii_case(query))
        .map(str::to_string)
}

pub(crate) fn fuzzy_match(candidate: &str, query: &str) -> Option<(u32, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let case_sensitive = query.chars().any(char::is_uppercase);
    let mut indices = Vec::new();
    let mut query_chars = query.chars();
    let mut expected = query_chars.next()?;
    for (index, character) in candidate.chars().enumerate() {
        let equal = if case_sensitive {
            character == expected
        } else {
            character.eq_ignore_ascii_case(&expected)
        };
        if equal {
            indices.push(index);
            if let Some(next) = query_chars.next() {
                expected = next;
            } else {
                let prefix = indices.iter().copied().eq(0..indices.len());
                let gaps = indices.last().copied().unwrap_or_default() + 1 - indices.len();
                return Some((u32::from(prefix) * 1_000 + 100 - gaps as u32, indices));
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "slash_tests.rs"]
mod tests;
