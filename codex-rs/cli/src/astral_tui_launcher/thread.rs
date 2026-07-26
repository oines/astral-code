use std::collections::HashMap;
use std::io;

use astral_tui::ThreadLaunch;
use astral_tui::ThreadPickerAction;
use astral_tui::ThreadPickerOptions;
use codex_app_server_client::AppServerClient;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadListCwdFilter;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadSortKey;
use codex_app_server_protocol::ThreadSource;
use codex_app_server_protocol::ThreadSourceKind;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartSource;
use codex_core::config::Config;
use codex_protocol::ThreadId;
use codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE;
use codex_protocol::models::PermissionProfile;
use codex_tui::Cli;

use super::config::ThreadParamsMode;

pub(super) async fn resolve_launch(
    client: &AppServerClient,
    cli: &Cli,
    config: &Config,
    mode: ThreadParamsMode,
) -> io::Result<Option<ThreadLaunch>> {
    if cli.resume_session_id.is_some() || cli.resume_last || cli.resume_picker {
        if cli.resume_picker {
            let params = thread_list_params(
                None,
                cli.resume_show_all,
                cli.resume_include_non_interactive,
                config,
                mode,
                cli.cwd.as_deref(),
                None,
            );
            let selected = astral_tui::run_thread_picker(
                client,
                ThreadPickerOptions::new(ThreadPickerAction::Resume, params),
            )
            .await?;
            let Some(thread) = selected else {
                return Ok(None);
            };
            return Ok(Some(ThreadLaunch::Resume(resume_params(
                &thread, cli, config, mode,
            ))));
        }
        let thread = resolve_thread(
            client,
            cli.resume_session_id.as_deref(),
            cli.resume_show_all,
            cli.resume_include_non_interactive,
            config,
            mode,
            cli.cwd.as_deref(),
        )
        .await?;
        return Ok(Some(match thread {
            Some(thread) => ThreadLaunch::Resume(resume_params(&thread, cli, config, mode)),
            None => ThreadLaunch::Start(start_params(cli, config, mode)),
        }));
    }
    if cli.fork_session_id.is_some() || cli.fork_last || cli.fork_picker {
        if cli.fork_picker {
            let params = thread_list_params(
                None,
                cli.fork_show_all,
                /*include_non_interactive*/ false,
                config,
                mode,
                cli.cwd.as_deref(),
                None,
            );
            let selected = astral_tui::run_thread_picker(
                client,
                ThreadPickerOptions::new(ThreadPickerAction::Fork, params),
            )
            .await?;
            let Some(thread) = selected else {
                return Ok(None);
            };
            return Ok(Some(ThreadLaunch::Fork(fork_params(
                &thread, cli, config, mode,
            ))));
        }
        let thread = resolve_thread(
            client,
            cli.fork_session_id.as_deref(),
            cli.fork_show_all,
            false,
            config,
            mode,
            cli.cwd.as_deref(),
        )
        .await?;
        return Ok(Some(match thread {
            Some(thread) => ThreadLaunch::Fork(fork_params(&thread, cli, config, mode)),
            None => ThreadLaunch::Start(start_params(cli, config, mode)),
        }));
    }
    Ok(Some(ThreadLaunch::Start(start_params(cli, config, mode))))
}

async fn resolve_thread(
    client: &AppServerClient,
    id_or_name: Option<&str>,
    show_all: bool,
    include_non_interactive: bool,
    config: &Config,
    mode: ThreadParamsMode,
    remote_cwd: Option<&std::path::Path>,
) -> io::Result<Option<Thread>> {
    if let Some(id_or_name) = id_or_name {
        return lookup_thread(client, id_or_name, include_non_interactive, config, mode)
            .await?
            .map(Some)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "no matching Astral session found")
            });
    }
    let response = list_threads(
        client,
        thread_list_params(
            None,
            show_all,
            include_non_interactive,
            config,
            mode,
            remote_cwd,
            None,
        ),
    )
    .await?;
    Ok(response.data.into_iter().next())
}

async fn lookup_thread(
    client: &AppServerClient,
    id_or_name: &str,
    include_non_interactive: bool,
    config: &Config,
    mode: ThreadParamsMode,
) -> io::Result<Option<Thread>> {
    if ThreadId::from_string(id_or_name).is_ok() {
        let response: ThreadReadResponse = client
            .request_typed(ClientRequest::ThreadRead {
                request_id: RequestId::Integer(0),
                params: ThreadReadParams {
                    thread_id: id_or_name.to_string(),
                    include_turns: false,
                },
            })
            .await
            .map_err(io::Error::other)?;
        return Ok(Some(response.thread));
    }
    let mut cursor = None;
    loop {
        let params = thread_list_params(
            Some(id_or_name),
            /*show_all*/ true,
            include_non_interactive,
            config,
            mode,
            /*remote_cwd*/ None,
            cursor,
        );
        let response = list_threads(client, params).await?;
        if let Some(thread) = response
            .data
            .into_iter()
            .find(|thread| thread.name.as_deref() == Some(id_or_name))
        {
            return Ok(Some(thread));
        }
        let Some(next_cursor) = response.next_cursor else {
            return Ok(None);
        };
        cursor = Some(next_cursor);
    }
}

async fn list_threads(
    client: &AppServerClient,
    params: ThreadListParams,
) -> io::Result<ThreadListResponse> {
    client
        .request_typed(ClientRequest::ThreadList {
            request_id: RequestId::Integer(0),
            params,
        })
        .await
        .map_err(io::Error::other)
}

fn thread_list_params(
    id_or_name: Option<&str>,
    show_all: bool,
    include_non_interactive: bool,
    config: &Config,
    mode: ThreadParamsMode,
    remote_cwd: Option<&std::path::Path>,
    cursor: Option<String>,
) -> ThreadListParams {
    let mut source_kinds = vec![ThreadSourceKind::Cli, ThreadSourceKind::VsCode];
    if include_non_interactive {
        source_kinds.extend([ThreadSourceKind::Exec, ThreadSourceKind::AppServer]);
    }
    ThreadListParams {
        cursor,
        limit: Some(if id_or_name.is_some() { 100 } else { 1 }),
        sort_key: Some(ThreadSortKey::UpdatedAt),
        sort_direction: None,
        model_providers: (id_or_name.is_none() && matches!(mode, ThreadParamsMode::Local))
            .then(|| vec![config.model_provider_id.clone()]),
        source_kinds: Some(source_kinds),
        archived: Some(false),
        cwd: if show_all {
            None
        } else {
            match mode {
                ThreadParamsMode::Local => Some(ThreadListCwdFilter::One(
                    config.cwd.to_string_lossy().to_string(),
                )),
                ThreadParamsMode::Remote => remote_cwd
                    .map(|cwd| ThreadListCwdFilter::One(cwd.to_string_lossy().to_string())),
            }
        },
        use_state_db_only: false,
        search_term: id_or_name.map(str::to_string),
    }
}

fn start_params(cli: &Cli, config: &Config, mode: ThreadParamsMode) -> ThreadStartParams {
    let mut params = common_params(cli, config, mode, /*existing_thread*/ None);
    ThreadStartParams {
        model: params.model.take(),
        model_provider: params.model_provider.take(),
        service_tier: params.service_tier.take(),
        cwd: params.cwd.take(),
        runtime_workspace_roots: params.runtime_workspace_roots.take(),
        approval_policy: params.approval_policy.take(),
        approvals_reviewer: params.approvals_reviewer.take(),
        sandbox: params.sandbox.take(),
        permissions: params.permissions.take(),
        config: params.config.take(),
        base_instructions: config.base_instructions.clone(),
        developer_instructions: config.developer_instructions.clone(),
        personality: config.personality,
        ephemeral: Some(config.ephemeral),
        session_start_source: Some(ThreadStartSource::Startup),
        thread_source: Some(ThreadSource::User),
        ..ThreadStartParams::default()
    }
}

fn resume_params(
    thread: &Thread,
    cli: &Cli,
    config: &Config,
    mode: ThreadParamsMode,
) -> ThreadResumeParams {
    let params = common_params(cli, config, mode, Some(thread));
    let preserve_thread_context = preserves_thread_context(cli, mode);
    ThreadResumeParams {
        thread_id: thread.id.clone(),
        model: params.model,
        model_provider: params.model_provider,
        service_tier: params.service_tier,
        cwd: params.cwd,
        runtime_workspace_roots: params.runtime_workspace_roots,
        approval_policy: params.approval_policy,
        approvals_reviewer: params.approvals_reviewer,
        sandbox: params.sandbox,
        permissions: params.permissions,
        config: params.config,
        base_instructions: (!preserve_thread_context)
            .then(|| config.base_instructions.clone())
            .flatten(),
        developer_instructions: (!preserve_thread_context)
            .then(|| config.developer_instructions.clone())
            .flatten(),
        personality: (!preserve_thread_context)
            .then_some(config.personality)
            .flatten(),
        ..ThreadResumeParams::default()
    }
}

fn fork_params(
    thread: &Thread,
    cli: &Cli,
    config: &Config,
    mode: ThreadParamsMode,
) -> ThreadForkParams {
    let params = common_params(cli, config, mode, Some(thread));
    let preserve_thread_context = preserves_thread_context(cli, mode);
    ThreadForkParams {
        thread_id: thread.id.clone(),
        model: params.model,
        model_provider: params.model_provider,
        service_tier: params.service_tier,
        cwd: params.cwd,
        runtime_workspace_roots: params.runtime_workspace_roots,
        approval_policy: params.approval_policy,
        approvals_reviewer: params.approvals_reviewer,
        sandbox: params.sandbox,
        permissions: params.permissions,
        config: params.config,
        base_instructions: (!preserve_thread_context)
            .then(|| config.base_instructions.clone())
            .flatten(),
        developer_instructions: (!preserve_thread_context)
            .then(|| config.developer_instructions.clone())
            .flatten(),
        ephemeral: config.ephemeral,
        thread_source: Some(ThreadSource::User),
        ..ThreadForkParams::default()
    }
}

struct CommonParams {
    model: Option<String>,
    model_provider: Option<String>,
    service_tier: Option<Option<String>>,
    cwd: Option<String>,
    runtime_workspace_roots: Option<Vec<codex_utils_absolute_path::AbsolutePathBuf>>,
    approval_policy: Option<codex_app_server_protocol::AskForApproval>,
    approvals_reviewer: Option<codex_app_server_protocol::ApprovalsReviewer>,
    sandbox: Option<codex_app_server_protocol::SandboxMode>,
    permissions: Option<String>,
    config: Option<HashMap<String, serde_json::Value>>,
}

fn common_params(
    cli: &Cli,
    config: &Config,
    mode: ThreadParamsMode,
    existing_thread: Option<&Thread>,
) -> CommonParams {
    let preserve_thread_context = existing_thread.is_some() && preserves_thread_context(cli, mode);
    let effective_cwd = existing_thread
        .filter(|_| preserve_thread_context)
        .map_or(config.cwd.as_path(), |thread| thread.cwd.as_path());
    let permissions = (matches!(mode, ThreadParamsMode::Local) && !preserve_thread_context)
        .then(|| config.permissions.active_permission_profile())
        .flatten()
        .map(|profile| profile.id);
    let send_sandbox_override = !preserve_thread_context
        || cli.sandbox_mode.is_some()
        || cli.dangerously_bypass_approvals_and_sandbox;
    let sandbox = (permissions.is_none() && send_sandbox_override)
        .then(|| {
            sandbox_mode_from_permission_profile(
                &config.permissions.effective_permission_profile(),
                effective_cwd,
            )
        })
        .flatten();
    let send_model_override = !preserve_thread_context || cli.model.is_some() || cli.oss;
    let send_approval_override = !preserve_thread_context
        || cli.approval_policy.is_some()
        || cli.dangerously_bypass_approvals_and_sandbox;
    CommonParams {
        model: send_model_override.then(|| config.model.clone()).flatten(),
        model_provider: (matches!(mode, ThreadParamsMode::Local) && send_model_override)
            .then(|| config.model_provider_id.clone()),
        service_tier: (!preserve_thread_context)
            .then(|| {
                config.service_tier.clone().map(Some).or_else(|| {
                    (config.notices.fast_default_opt_out == Some(true))
                        .then(|| Some(SERVICE_TIER_DEFAULT_REQUEST_VALUE.to_string()))
                })
            })
            .flatten(),
        cwd: match mode {
            ThreadParamsMode::Local => Some(effective_cwd.to_string_lossy().to_string()),
            ThreadParamsMode::Remote => cli
                .cwd
                .as_ref()
                .map(|cwd| cwd.to_string_lossy().to_string()),
        },
        runtime_workspace_roots: (matches!(mode, ThreadParamsMode::Local)
            && !preserve_thread_context)
            .then(|| config.workspace_roots.clone()),
        approval_policy: send_approval_override
            .then(|| config.permissions.approval_policy.value().into()),
        approvals_reviewer: (!preserve_thread_context)
            .then(|| config.approvals_reviewer.into()),
        sandbox,
        permissions,
        config: (!preserve_thread_context).then(|| config_request_overrides(config)),
    }
}

fn preserves_thread_context(cli: &Cli, mode: ThreadParamsMode) -> bool {
    matches!(mode, ThreadParamsMode::Local) && cli.cwd.is_none()
}

fn sandbox_mode_from_permission_profile(
    profile: &PermissionProfile,
    cwd: &std::path::Path,
) -> Option<codex_app_server_protocol::SandboxMode> {
    match profile {
        PermissionProfile::Disabled => {
            Some(codex_app_server_protocol::SandboxMode::DangerFullAccess)
        }
        PermissionProfile::External { .. } => None,
        PermissionProfile::Managed { .. } => {
            let filesystem = profile.file_system_sandbox_policy();
            if filesystem.has_full_disk_write_access() {
                profile
                    .network_sandbox_policy()
                    .is_enabled()
                    .then_some(codex_app_server_protocol::SandboxMode::DangerFullAccess)
            } else if filesystem.can_write_path_with_cwd(cwd, cwd) {
                Some(codex_app_server_protocol::SandboxMode::WorkspaceWrite)
            } else {
                Some(codex_app_server_protocol::SandboxMode::ReadOnly)
            }
        }
    }
}

fn config_request_overrides(config: &Config) -> HashMap<String, serde_json::Value> {
    let mut overrides = HashMap::new();
    let mut insert = |key: &str, value: Option<String>| {
        if let Some(value) = value {
            overrides.insert(key.to_string(), value.into());
        }
    };
    insert(
        "model_reasoning_effort",
        config
            .model_reasoning_effort
            .as_ref()
            .map(ToString::to_string),
    );
    insert(
        "model_reasoning_summary",
        config
            .model_reasoning_summary
            .map(|value| value.to_string()),
    );
    insert(
        "model_verbosity",
        config.model_verbosity.map(|value| value.to_string()),
    );
    insert(
        "personality",
        config.personality.map(|value| value.to_string()),
    );
    insert(
        "web_search",
        Some(config.web_search_mode.value().to_string()),
    );
    if config.bypass_hook_trust {
        overrides.insert("bypass_hook_trust".to_string(), true.into());
    }
    overrides
}

#[cfg(test)]
#[path = "thread_tests.rs"]
mod tests;
