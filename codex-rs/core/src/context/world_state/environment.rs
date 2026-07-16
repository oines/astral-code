use super::PreviousSectionState;
use super::WorldStateSection;
use super::truncate_world_state_text;
use crate::context::ContextualUserFragment;
use crate::context::environment_context::FileSystemContext;
use crate::context::environment_context::NetworkContext;
use crate::context::environment_context::push_xml_escaped_text;
use crate::environment_selection::TurnEnvironmentSnapshot;
use crate::session::turn_context::TurnContext;
use codex_utils_path_uri::PathUri;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;

#[path = "environment_projection.rs"]
mod projection;

use projection::project_snapshot;

const MAX_ENVIRONMENTS_FRAGMENT_BYTES: usize = 8 * 1024;
const MAX_ENVIRONMENT_ID_BYTES: usize = 256;
const MAX_ENVIRONMENT_VALUE_BYTES: usize = 1024;
const MAX_TURN_CONTEXT_VALUE_BYTES: usize = 256;
const MAX_SUBAGENTS_BYTES: usize = 1024;
const TRUNCATED_CONTEXT_NOTICE: &str =
    "Additional environment context was omitted to fit the model context limit.";

/// Environment values visible to the model.
#[derive(Clone, Debug, Default)]
pub(crate) struct EnvironmentsState {
    environments: BTreeMap<String, EnvironmentState>,
    /// Model slug the session is invoking via the API. Astral keeps this in the
    /// environment projection so the model can answer session-fact questions.
    model: Option<String>,
    current_date: Option<String>,
    timezone: Option<String>,
    network: Option<NetworkContext>,
    filesystem: Option<FileSystemContext>,
    subagents: Option<String>,
}

impl EnvironmentsState {
    pub(crate) fn from_turn_context_with_environments(
        turn_context: &TurnContext,
        environments: &TurnEnvironmentSnapshot,
    ) -> Self {
        Self {
            environments: environment_states(environments),
            model: Some(turn_context.model_info.slug.clone()),
            current_date: turn_context.current_date.clone(),
            timezone: turn_context.timezone.clone(),
            network: network_from_turn_context(turn_context),
            filesystem: Some(FileSystemContext::from_permission_profile(
                &turn_context.permission_profile,
                &turn_context.config.effective_workspace_roots(),
            )),
            subagents: None,
        }
    }

    pub(crate) fn with_subagents(mut self, subagents: String) -> Self {
        if !subagents.is_empty() {
            self.subagents = Some(subagents);
        }
        self
    }

    fn rendered_full(&self) -> RenderedEnvironments {
        RenderedEnvironments::full(&self.snapshot())
    }
}

impl WorldStateSection for EnvironmentsState {
    const ID: &'static str = "environments";
    type Snapshot = EnvironmentsSnapshot;

    fn snapshot(&self) -> Self::Snapshot {
        project_snapshot(self)
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> Option<Box<dyn ContextualUserFragment>> {
        let current = self.snapshot();
        let empty = EnvironmentsSnapshot::default();
        let previous = match previous {
            PreviousSectionState::Known(previous) => previous,
            PreviousSectionState::Absent | PreviousSectionState::Unknown => &empty,
        };
        let turn_context_values_changed = current.model != previous.model
            || current.current_date != previous.current_date
            || current.timezone != previous.timezone
            || current.network != previous.network
            || current.filesystem != previous.filesystem
            || current.subagents != previous.subagents
            || current.truncated != previous.truncated;
        let mut updates = current
            .environments
            .iter()
            .filter(|(id, _)| {
                let environment = &current.environments[*id];
                previous
                    .environments
                    .get(*id)
                    .is_none_or(|previous| !environment.has_same_diff_value(previous))
            })
            .map(|(id, environment)| (id.clone(), EnvironmentUpdate::Current(environment.clone())))
            .collect::<BTreeMap<_, _>>();
        updates.extend(
            previous
                .environments
                .keys()
                .filter(|id| !current.environments.contains_key(*id))
                .map(|id| (id.clone(), EnvironmentUpdate::Unavailable)),
        );
        let legacy_single = is_legacy_single(&current.environments)
            && updates
                .values()
                .all(|update| matches!(update, EnvironmentUpdate::Current(_)));
        (!updates.is_empty() || turn_context_values_changed).then(|| {
            Box::new(RenderedEnvironments {
                updates,
                legacy_single,
                model: current.model.clone(),
                current_date: current.current_date.clone(),
                timezone: current.timezone.clone(),
                network: current.network.clone(),
                filesystem: current.filesystem.clone(),
                subagents: match (&current.subagents, &previous.subagents) {
                    (Some(subagents), _) => RenderedSubagents::Current(subagents.clone()),
                    (None, Some(_)) => RenderedSubagents::Cleared,
                    (None, None) => RenderedSubagents::Omitted,
                },
                truncation: match (current.truncated, previous.truncated) {
                    (true, _) => RenderedTruncation::Current,
                    (false, true) => RenderedTruncation::Cleared,
                    (false, false) => RenderedTruncation::Omitted,
                },
            }) as Box<dyn ContextualUserFragment>
        })
    }
}

impl ContextualUserFragment for EnvironmentsState {
    fn role(&self) -> &'static str {
        "user"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        environment_context_markers()
    }

    fn body(&self) -> String {
        self.rendered_full().body()
    }
}

struct RenderedEnvironments {
    updates: BTreeMap<String, EnvironmentUpdate>,
    legacy_single: bool,
    model: Option<String>,
    current_date: Option<String>,
    timezone: Option<String>,
    network: Option<String>,
    filesystem: Option<String>,
    subagents: RenderedSubagents,
    truncation: RenderedTruncation,
}

enum RenderedSubagents {
    Omitted,
    Current(String),
    Cleared,
}

enum RenderedTruncation {
    Omitted,
    Current,
    Cleared,
}

enum EnvironmentUpdate {
    Current(EnvironmentSnapshot),
    Unavailable,
}

impl RenderedEnvironments {
    fn full(snapshot: &EnvironmentsSnapshot) -> Self {
        Self {
            updates: snapshot
                .environments
                .iter()
                .map(|(id, environment)| {
                    (id.clone(), EnvironmentUpdate::Current(environment.clone()))
                })
                .collect(),
            legacy_single: is_legacy_single(&snapshot.environments),
            model: snapshot.model.clone(),
            current_date: snapshot.current_date.clone(),
            timezone: snapshot.timezone.clone(),
            network: snapshot.network.clone(),
            filesystem: snapshot.filesystem.clone(),
            subagents: snapshot
                .subagents
                .clone()
                .map_or(RenderedSubagents::Omitted, RenderedSubagents::Current),
            truncation: if snapshot.truncated {
                RenderedTruncation::Current
            } else {
                RenderedTruncation::Omitted
            },
        }
    }
}

impl ContextualUserFragment for RenderedEnvironments {
    fn role(&self) -> &'static str {
        "user"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        environment_context_markers()
    }

    fn body(&self) -> String {
        let mut rendered = "\n".to_string();
        if self.legacy_single {
            if let Some(EnvironmentUpdate::Current(environment)) = self.updates.values().next() {
                push_environment_values(&mut rendered, environment, "  ");
            }
        } else if !self.updates.is_empty() {
            rendered.push_str("  <environments>\n");
            for (id, update) in &self.updates {
                match update {
                    EnvironmentUpdate::Current(environment) => {
                        rendered.push_str("    <environment id=\"");
                        push_xml_escaped_text(&mut rendered, id);
                        rendered.push('"');
                        rendered.push_str(">\n");
                        push_environment_values(&mut rendered, environment, "      ");
                        rendered.push_str("    </environment>\n");
                    }
                    EnvironmentUpdate::Unavailable => {
                        rendered.push_str("    <environment id=\"");
                        push_xml_escaped_text(&mut rendered, id);
                        rendered.push_str("\" status=\"unavailable\" />\n");
                    }
                }
            }
            rendered.push_str("  </environments>\n");
        }
        push_optional_element(&mut rendered, "model", self.model.as_deref());
        push_optional_element(&mut rendered, "current_date", self.current_date.as_deref());
        push_optional_element(&mut rendered, "timezone", self.timezone.as_deref());
        if let Some(network) = &self.network {
            rendered.push_str("  ");
            rendered.push_str(network);
            rendered.push('\n');
        }
        if let Some(filesystem) = &self.filesystem {
            rendered.push_str("  ");
            rendered.push_str(filesystem);
            rendered.push('\n');
        }
        match &self.subagents {
            RenderedSubagents::Omitted => {}
            RenderedSubagents::Current(subagents) => {
                rendered.push_str("  <subagents>\n");
                for line in subagents.lines() {
                    rendered.push_str("    ");
                    rendered.push_str(line);
                    rendered.push('\n');
                }
                rendered.push_str("  </subagents>\n");
            }
            RenderedSubagents::Cleared => rendered.push_str("  <subagents />\n"),
        }
        match self.truncation {
            RenderedTruncation::Omitted => {}
            RenderedTruncation::Current => {
                push_optional_element(&mut rendered, "truncated", Some(TRUNCATED_CONTEXT_NOTICE));
            }
            RenderedTruncation::Cleared => {
                push_optional_element(&mut rendered, "truncated", Some("false"));
            }
        }
        rendered
    }
}

fn push_environment_values(rendered: &mut String, environment: &EnvironmentSnapshot, indent: &str) {
    rendered.push_str(indent);
    rendered.push_str("<cwd>");
    push_xml_escaped_text(rendered, &environment.cwd);
    rendered.push_str("</cwd>\n");
    if environment.status == EnvironmentStatus::Starting {
        rendered.push_str(indent);
        rendered.push_str("<status>starting</status>\n");
    }
    if let Some(shell) = &environment.shell {
        rendered.push_str(indent);
        rendered.push_str("<shell>");
        push_xml_escaped_text(rendered, shell);
        rendered.push_str("</shell>\n");
    }
}

fn push_optional_element(rendered: &mut String, name: &str, value: Option<&str>) {
    let Some(value) = value else {
        return;
    };
    rendered.push_str("  <");
    rendered.push_str(name);
    rendered.push('>');
    push_xml_escaped_text(rendered, value);
    rendered.push_str("</");
    rendered.push_str(name);
    rendered.push_str(">\n");
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EnvironmentState {
    cwd: PathUri,
    status: EnvironmentStatus,
    shell: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct EnvironmentsSnapshot {
    environments: BTreeMap<String, EnvironmentSnapshot>,
    model: Option<String>,
    current_date: Option<String>,
    timezone: Option<String>,
    network: Option<String>,
    filesystem: Option<String>,
    subagents: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    truncated: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct EnvironmentSnapshot {
    cwd: String,
    status: EnvironmentStatus,
    shell: Option<String>,
}

impl EnvironmentSnapshot {
    fn has_same_diff_value(&self, other: &Self) -> bool {
        self.cwd == other.cwd
            && self.status == other.status
            && self
                .shell
                .as_ref()
                .zip(other.shell.as_ref())
                .is_none_or(|(current, previous)| current == previous)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EnvironmentStatus {
    Starting,
    Available,
}

fn environment_states(snapshot: &TurnEnvironmentSnapshot) -> BTreeMap<String, EnvironmentState> {
    let mut environments = snapshot
        .turn_environments
        .iter()
        .map(|environment| {
            (
                environment.environment_id.clone(),
                EnvironmentState {
                    cwd: environment.cwd().clone(),
                    status: EnvironmentStatus::Available,
                    shell: environment
                        .shell
                        .as_ref()
                        .map(|shell| shell.name().to_string()),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for environment in &snapshot.starting {
        environments
            .entry(environment.selection.environment_id.clone())
            .or_insert_with(|| EnvironmentState {
                cwd: environment.selection.cwd.clone(),
                status: EnvironmentStatus::Starting,
                shell: None,
            });
    }
    environments
}

fn is_legacy_single(environments: &BTreeMap<String, EnvironmentSnapshot>) -> bool {
    environments.len() == 1
        && environments
            .values()
            .all(|environment| environment.status == EnvironmentStatus::Available)
}

fn environment_context_markers() -> (&'static str, &'static str) {
    (
        codex_protocol::protocol::ENVIRONMENT_CONTEXT_OPEN_TAG,
        codex_protocol::protocol::ENVIRONMENT_CONTEXT_CLOSE_TAG,
    )
}

fn network_from_turn_context(turn_context: &TurnContext) -> Option<NetworkContext> {
    let network = turn_context
        .config
        .config_layer_stack
        .requirements()
        .network
        .as_ref()?;

    Some(NetworkContext::new(
        network
            .domains
            .as_ref()
            .and_then(codex_config::NetworkDomainPermissionsToml::allowed_domains)
            .unwrap_or_default(),
        network
            .domains
            .as_ref()
            .and_then(codex_config::NetworkDomainPermissionsToml::denied_domains)
            .unwrap_or_default(),
    ))
}

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "environment_render_tests.rs"]
mod render_tests;
