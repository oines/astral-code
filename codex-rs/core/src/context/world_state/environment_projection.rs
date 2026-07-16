use super::EnvironmentSnapshot;
use super::EnvironmentsSnapshot;
use super::EnvironmentsState;
use super::MAX_ENVIRONMENT_ID_BYTES;
use super::MAX_ENVIRONMENT_VALUE_BYTES;
use super::MAX_ENVIRONMENTS_FRAGMENT_BYTES;
use super::MAX_SUBAGENTS_BYTES;
use super::MAX_TURN_CONTEXT_VALUE_BYTES;
use super::RenderedEnvironments;
use super::truncate_world_state_text;
use crate::context::ContextualUserFragment;
use crate::context::environment_context::FileSystemContext;
use crate::context::environment_context::NetworkContext;

pub(super) fn project_snapshot(state: &EnvironmentsState) -> EnvironmentsSnapshot {
    let (model, model_truncated) =
        project_optional_text(state.model.as_deref(), MAX_TURN_CONTEXT_VALUE_BYTES);
    let (current_date, current_date_truncated) =
        project_optional_text(state.current_date.as_deref(), MAX_TURN_CONTEXT_VALUE_BYTES);
    let (timezone, timezone_truncated) =
        project_optional_text(state.timezone.as_deref(), MAX_TURN_CONTEXT_VALUE_BYTES);
    let mut projected = EnvironmentsSnapshot {
        model,
        current_date,
        timezone,
        ..Default::default()
    };
    let mut truncated = model_truncated || current_date_truncated || timezone_truncated;

    if let Some(filesystem) = state.filesystem.as_ref().map(FileSystemContext::render) {
        let mut candidate = projected.clone();
        candidate.filesystem = Some(filesystem);
        if projected_snapshot_fits(&candidate) {
            projected = candidate;
        } else {
            truncated = true;
        }
    }

    for (id, environment) in &state.environments {
        if id.len() > MAX_ENVIRONMENT_ID_BYTES {
            truncated = true;
            continue;
        }
        let cwd = environment.cwd.inferred_native_path_string();
        let projected_cwd = truncate_world_state_text(&cwd, MAX_ENVIRONMENT_VALUE_BYTES);
        let projected_shell = environment
            .shell
            .as_deref()
            .map(|shell| truncate_world_state_text(shell, MAX_ENVIRONMENT_VALUE_BYTES));
        truncated |=
            projected_cwd != cwd || projected_shell.as_deref() != environment.shell.as_deref();

        let mut candidate = projected.clone();
        candidate.environments.insert(
            id.clone(),
            EnvironmentSnapshot {
                cwd: projected_cwd,
                status: environment.status,
                shell: projected_shell,
            },
        );
        if projected_snapshot_fits(&candidate) {
            projected = candidate;
        } else {
            truncated = true;
        }
    }

    if let Some(network) = state.network.as_ref().map(NetworkContext::render) {
        let mut candidate = projected.clone();
        candidate.network = Some(network);
        if projected_snapshot_fits(&candidate) {
            projected = candidate;
        } else {
            truncated = true;
        }
    }

    if let Some(subagents) = &state.subagents {
        let projected_subagents = truncate_world_state_text(subagents, MAX_SUBAGENTS_BYTES);
        truncated |= projected_subagents != *subagents;
        let mut candidate = projected.clone();
        candidate.subagents = Some(projected_subagents);
        if projected_snapshot_fits(&candidate) {
            projected = candidate;
        } else {
            truncated = true;
        }
    }

    projected.truncated = truncated;
    debug_assert!(
        RenderedEnvironments::replacement(&projected).render().len()
            <= MAX_ENVIRONMENTS_FRAGMENT_BYTES
    );
    projected
}

fn project_optional_text(value: Option<&str>, max_bytes: usize) -> (Option<String>, bool) {
    let Some(value) = value else {
        return (None, false);
    };
    let projected = truncate_world_state_text(value, max_bytes);
    let truncated = projected != value;
    (Some(projected), truncated)
}

fn projected_snapshot_fits(snapshot: &EnvironmentsSnapshot) -> bool {
    let mut guarded = snapshot.clone();
    guarded.truncated = true;
    RenderedEnvironments::replacement(&guarded).render().len() <= MAX_ENVIRONMENTS_FRAGMENT_BYTES
}
