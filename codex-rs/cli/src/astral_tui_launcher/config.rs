use std::io;
use std::sync::Arc;
use std::time::Duration;

use codex_app_server_client::AppServerClient;
use codex_app_server_client::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY;
use codex_app_server_client::EnvironmentManager;
use codex_app_server_client::ExecServerRuntimePaths;
use codex_app_server_client::InProcessAppServerClient;
use codex_app_server_client::InProcessClientStartArgs;
use codex_app_server_client::RemoteAppServerClient;
use codex_app_server_client::RemoteAppServerConnectArgs;
use codex_app_server_client::RemoteAppServerEndpoint;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigBatchWriteParams;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ConfigWarningNotification;
use codex_app_server_protocol::ConfigWriteResponse;
use codex_app_server_protocol::MergeStrategy;
use codex_app_server_protocol::RequestId;
use codex_arg0::Arg0DispatchPaths;
use codex_config::CloudConfigBundleLoader;
use codex_config::ConfigLoadOptions;
use codex_config::LoaderOverrides;
use codex_config::types::UiVariant;
use codex_core::config::Config;
use codex_core::config::ConfigBuilder;
use codex_core::config::ConfigOverrides;
use codex_core::config::find_codex_home;
use codex_core::config::load_config_as_toml_with_cli_and_load_options;
use codex_core::config::resolve_oss_provider;
use codex_core::config::resolve_profile_v2_config_path;
use codex_feedback::CodexFeedback;
use codex_protocol::config_types::AltScreenMode;
use codex_protocol::config_types::SandboxMode;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::SessionSource;
use codex_tui::Cli;
use codex_tui::LocalStateDbStartupError;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_oss::ensure_oss_provider_ready;
use codex_utils_oss::get_default_model_for_oss_provider;
use tracing::warn;

#[cfg(unix)]
const DAEMON_CONNECT_TIMEOUT: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThreadParamsMode {
    Local,
    Remote,
}

#[derive(Debug, Clone)]
enum AppServerTarget {
    Embedded,
    LocalDaemon(RemoteAppServerEndpoint),
    Remote(RemoteAppServerEndpoint),
}

impl AppServerTarget {
    fn params_mode(&self) -> ThreadParamsMode {
        match self {
            Self::Embedded | Self::LocalDaemon(_) => ThreadParamsMode::Local,
            Self::Remote(_) => ThreadParamsMode::Remote,
        }
    }
}

pub(super) struct LaunchContext {
    pub client: AppServerClient,
    pub config: Arc<Config>,
    pub target: ThreadParamsMode,
}

pub(super) struct PreparedLaunch {
    arg0_paths: Arg0DispatchPaths,
    cli_kv_overrides: Vec<(String, toml::Value)>,
    loader_overrides: LoaderOverrides,
    strict_config: bool,
    cloud_config_bundle: CloudConfigBundleLoader,
    config: Arc<Config>,
    target: AppServerTarget,
    oss_provider_to_persist: Option<String>,
}

impl PreparedLaunch {
    pub fn configured_ui(&self) -> UiVariant {
        self.config.tui_variant
    }

    pub fn configured_alt_screen(&self) -> AltScreenMode {
        self.config.tui_alternate_screen
    }

    pub fn requires_classic_preflight(&self) -> bool {
        matches!(
            self.target,
            AppServerTarget::Embedded | AppServerTarget::LocalDaemon(_)
        ) && self.config.active_project.trust_level.is_none()
    }

    pub async fn start(self) -> io::Result<LaunchContext> {
        let target_mode = self.target.params_mode();
        let client = start_client(
            self.target,
            self.arg0_paths,
            Arc::clone(&self.config),
            self.cli_kv_overrides,
            self.loader_overrides,
            self.strict_config,
            self.cloud_config_bundle,
        )
        .await?;
        if let Some(provider) = self.oss_provider_to_persist.as_deref()
            && let Err(error) = persist_oss_provider(&client, provider).await
        {
            warn!(%error, %provider, "failed to persist selected OSS provider");
        }
        Ok(LaunchContext {
            client,
            config: self.config,
            target: target_mode,
        })
    }
}

pub(super) async fn prepare_launch(
    cli: &Cli,
    arg0_paths: Arg0DispatchPaths,
    loader_overrides: LoaderOverrides,
    explicit_remote_endpoint: Option<RemoteAppServerEndpoint>,
) -> io::Result<PreparedLaunch> {
    let strict_config = cli.strict_config;
    let mut raw_overrides = cli.config_overrides.raw_overrides.clone();
    if cli.web_search {
        raw_overrides.push("web_search=\"live\"".to_string());
    }
    let cli_kv_overrides = codex_utils_cli::CliConfigOverrides { raw_overrides }
        .parse_overrides()
        .map_err(io::Error::other)?;
    let loader_overrides = loader_overrides_for_profile(cli, loader_overrides)?;
    let cloud_config_bundle = CloudConfigBundleLoader::default();
    let uses_remote_workspace = explicit_remote_endpoint.is_some();
    let model_provider = resolve_launch_model_provider(
        cli,
        &cli_kv_overrides,
        &loader_overrides,
        cloud_config_bundle.clone(),
        uses_remote_workspace,
    )
    .await?;
    let config = build_config(
        cli,
        &arg0_paths,
        &cli_kv_overrides,
        &loader_overrides,
        cloud_config_bundle.clone(),
        uses_remote_workspace,
        model_provider.id.clone(),
    )
    .await?;
    let target = target_for_launch(
        explicit_remote_endpoint,
        &config,
        &cli_kv_overrides,
        &loader_overrides,
        strict_config,
        cli.bypass_hook_trust,
    )
    .await?;
    let config = Arc::new(config);
    Ok(PreparedLaunch {
        arg0_paths,
        cli_kv_overrides,
        loader_overrides,
        strict_config,
        cloud_config_bundle,
        config,
        target,
        oss_provider_to_persist: model_provider.persist,
    })
}

async fn build_config(
    cli: &Cli,
    arg0_paths: &Arg0DispatchPaths,
    cli_kv_overrides: &[(String, toml::Value)],
    loader_overrides: &LoaderOverrides,
    cloud_config_bundle: CloudConfigBundleLoader,
    uses_remote_workspace: bool,
    model_provider: Option<String>,
) -> io::Result<Config> {
    let model = cli.model.clone().or_else(|| {
        model_provider
            .as_deref()
            .and_then(get_default_model_for_oss_provider)
            .map(str::to_string)
    });
    let (sandbox_mode, approval_policy) = if cli.dangerously_bypass_approvals_and_sandbox {
        (
            Some(SandboxMode::DangerFullAccess),
            Some(AskForApproval::Never),
        )
    } else {
        (
            cli.sandbox_mode.map(Into::<SandboxMode>::into),
            cli.approval_policy.map(Into::into),
        )
    };
    let overrides = ConfigOverrides {
        model,
        approval_policy,
        sandbox_mode,
        cwd: if uses_remote_workspace {
            None
        } else {
            cli.cwd.clone()
        },
        model_provider: model_provider.clone(),
        codex_self_exe: arg0_paths.codex_self_exe.clone(),
        codex_linux_sandbox_exe: arg0_paths.codex_linux_sandbox_exe.clone(),
        main_execve_wrapper_exe: arg0_paths.main_execve_wrapper_exe.clone(),
        show_raw_agent_reasoning: cli.oss.then_some(true),
        bypass_hook_trust: cli.bypass_hook_trust.then_some(true),
        additional_writable_roots: cli.add_dir.clone(),
        ..Default::default()
    };
    let config = ConfigBuilder::default()
        .cli_overrides(cli_kv_overrides.to_vec())
        .harness_overrides(overrides)
        .loader_overrides(loader_overrides.clone())
        .strict_config(cli.strict_config)
        .cloud_config_bundle(cloud_config_bundle)
        .build()
        .await?;
    if let Some(provider) = model_provider {
        ensure_oss_provider_ready(&provider, &config).await?;
    }
    Ok(config)
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ResolvedModelProvider {
    id: Option<String>,
    persist: Option<String>,
}

async fn resolve_launch_model_provider(
    cli: &Cli,
    cli_kv_overrides: &[(String, toml::Value)],
    loader_overrides: &LoaderOverrides,
    cloud_config_bundle: CloudConfigBundleLoader,
    uses_remote_workspace: bool,
) -> io::Result<ResolvedModelProvider> {
    if !cli.oss {
        return Ok(ResolvedModelProvider::default());
    }
    if let Some(provider) = cli.oss_provider.as_ref() {
        return Ok(ResolvedModelProvider {
            id: Some(provider.clone()),
            persist: None,
        });
    }
    let codex_home = find_codex_home()?;
    let cwd = config_lookup_cwd(cli.cwd.as_deref(), uses_remote_workspace)?;
    let config_toml = load_config_as_toml_with_cli_and_load_options(
        &codex_home,
        cwd.as_ref(),
        cli_kv_overrides.to_vec(),
        ConfigLoadOptions {
            loader_overrides: loader_overrides.clone(),
            strict_config: cli.strict_config,
            cloud_config_bundle,
        },
    )
    .await?;
    if let Some(provider) = resolve_oss_provider(/*explicit_provider*/ None, &config_toml) {
        return Ok(ResolvedModelProvider {
            id: Some(provider),
            persist: None,
        });
    }
    let (provider, manually_selected) = codex_tui::select_oss_provider_for_launch().await?;
    if provider == "__CANCELLED__" {
        return Err(io::Error::other(
            "OSS provider selection was cancelled by user",
        ));
    }
    Ok(ResolvedModelProvider {
        persist: manually_selected.then(|| provider.clone()),
        id: Some(provider),
    })
}

async fn persist_oss_provider(client: &AppServerClient, provider: &str) -> io::Result<()> {
    let _: ConfigWriteResponse = client
        .request_typed(ClientRequest::ConfigBatchWrite {
            request_id: RequestId::String("astral-oss-provider".to_string()),
            params: ConfigBatchWriteParams {
                edits: vec![ConfigEdit {
                    key_path: "oss_provider".to_string(),
                    value: provider.into(),
                    merge_strategy: MergeStrategy::Replace,
                }],
                file_path: None,
                expected_version: None,
                reload_user_config: false,
            },
        })
        .await
        .map_err(io::Error::other)?;
    Ok(())
}

fn config_lookup_cwd(
    cwd: Option<&std::path::Path>,
    uses_remote_workspace: bool,
) -> io::Result<Option<AbsolutePathBuf>> {
    if uses_remote_workspace {
        return Ok(None);
    }
    let cwd = match cwd {
        Some(cwd) if cwd.is_absolute() => cwd.to_path_buf(),
        Some(cwd) => std::env::current_dir()?.join(cwd),
        None => std::env::current_dir()?,
    };
    AbsolutePathBuf::try_from(cwd)
        .map(Some)
        .map_err(io::Error::other)
}

fn loader_overrides_for_profile(
    cli: &Cli,
    mut loader_overrides: LoaderOverrides,
) -> io::Result<LoaderOverrides> {
    if let Some(profile) = cli.config_profile_v2.as_ref() {
        let codex_home = codex_core::config::find_codex_home()?;
        loader_overrides.user_config_path =
            Some(resolve_profile_v2_config_path(&codex_home, profile));
        loader_overrides.user_config_profile = Some(profile.clone());
    }
    Ok(loader_overrides)
}

async fn target_for_launch(
    explicit_remote_endpoint: Option<RemoteAppServerEndpoint>,
    config: &Config,
    cli_kv_overrides: &[(String, toml::Value)],
    loader_overrides: &LoaderOverrides,
    strict_config: bool,
    bypass_hook_trust: bool,
) -> io::Result<AppServerTarget> {
    if let Some(endpoint) = explicit_remote_endpoint {
        return Ok(AppServerTarget::Remote(endpoint));
    }
    if can_reuse_daemon(
        cli_kv_overrides,
        loader_overrides,
        strict_config,
        bypass_hook_trust,
    ) && let Some(socket_path) = probe_default_daemon(config.codex_home.as_path()).await
    {
        return Ok(AppServerTarget::LocalDaemon(
            RemoteAppServerEndpoint::UnixSocket { socket_path },
        ));
    }
    Ok(AppServerTarget::Embedded)
}

fn can_reuse_daemon(
    cli_kv_overrides: &[(String, toml::Value)],
    loader_overrides: &LoaderOverrides,
    strict_config: bool,
    bypass_hook_trust: bool,
) -> bool {
    cli_kv_overrides.is_empty()
        && loader_overrides_are_default(loader_overrides)
        && !strict_config
        && !bypass_hook_trust
}

fn loader_overrides_are_default(overrides: &LoaderOverrides) -> bool {
    let is_default = overrides.user_config_path.is_none()
        && overrides.user_config_profile.is_none()
        && overrides.managed_config_path.is_none()
        && overrides.system_config_path.is_none()
        && overrides.system_requirements_path.is_none()
        && !overrides.ignore_managed_requirements
        && !overrides.ignore_user_config
        && !overrides.ignore_user_and_project_exec_policy_rules
        && overrides.macos_managed_config_requirements_base64.is_none();
    #[cfg(target_os = "macos")]
    let is_default = is_default && overrides.managed_preferences_base64.is_none();
    is_default
}

#[cfg(unix)]
async fn probe_default_daemon(codex_home: &std::path::Path) -> Option<AbsolutePathBuf> {
    let socket_path = codex_app_server_client::app_server_control_socket_path(codex_home).ok()?;
    if !socket_path.as_path().try_exists().unwrap_or(false) {
        return None;
    }
    tokio::time::timeout(
        DAEMON_CONNECT_TIMEOUT,
        tokio::net::UnixStream::connect(socket_path.as_path()),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .map(|_| socket_path)
}

#[cfg(not(unix))]
async fn probe_default_daemon(_codex_home: &std::path::Path) -> Option<AbsolutePathBuf> {
    None
}

async fn start_client(
    target: AppServerTarget,
    arg0_paths: Arg0DispatchPaths,
    config: Arc<Config>,
    cli_overrides: Vec<(String, toml::Value)>,
    loader_overrides: LoaderOverrides,
    strict_config: bool,
    cloud_config_bundle: CloudConfigBundleLoader,
) -> io::Result<AppServerClient> {
    match target {
        AppServerTarget::LocalDaemon(endpoint) | AppServerTarget::Remote(endpoint) => {
            RemoteAppServerClient::connect(RemoteAppServerConnectArgs {
                endpoint,
                client_name: "astral-tui".to_string(),
                client_version: env!("CARGO_PKG_VERSION").to_string(),
                experimental_api: true,
                opt_out_notification_methods: Vec::new(),
                channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
            })
            .await
            .map(AppServerClient::Remote)
        }
        AppServerTarget::Embedded => {
            let state_db = codex_rollout::state_db::try_init(config.as_ref())
                .await
                .map(Some)
                .map_err(|error| {
                    io::Error::other(LocalStateDbStartupError::new(
                        codex_state::state_db_path(config.sqlite_home.as_path()),
                        error.to_string(),
                    ))
                })?;
            let runtime_paths = ExecServerRuntimePaths::from_optional_paths(
                arg0_paths.codex_self_exe.clone(),
                arg0_paths.codex_linux_sandbox_exe.clone(),
            )?;
            let environment_manager = if loader_overrides.ignore_user_config {
                EnvironmentManager::from_env(Some(runtime_paths)).await
            } else {
                EnvironmentManager::from_codex_home(config.codex_home.clone(), Some(runtime_paths))
                    .await
            }
            .map(Arc::new)
            .map_err(io::Error::other)?;
            let config_warnings = config
                .startup_warnings
                .iter()
                .map(|summary| ConfigWarningNotification {
                    summary: summary.clone(),
                    details: None,
                    path: None,
                    range: None,
                })
                .collect();
            InProcessAppServerClient::start(InProcessClientStartArgs {
                arg0_paths,
                config,
                cli_overrides,
                loader_overrides,
                strict_config,
                cloud_config_bundle,
                feedback: CodexFeedback::new(),
                log_db: None,
                state_db,
                environment_manager,
                config_warnings,
                session_source: SessionSource::Cli,
                enable_astral_api_key_env: false,
                client_name: "astral-tui".to_string(),
                client_version: env!("CARGO_PKG_VERSION").to_string(),
                experimental_api: true,
                opt_out_notification_methods: Vec::new(),
                channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
            })
            .await
            .map(AppServerClient::InProcess)
        }
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
