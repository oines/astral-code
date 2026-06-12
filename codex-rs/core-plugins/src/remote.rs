use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::PluginAuthPolicy;
use codex_app_server_protocol::PluginAvailability;
use codex_app_server_protocol::PluginInstallPolicy;
use codex_app_server_protocol::PluginInterface;
use codex_app_server_protocol::SkillInterface;
use codex_login::CodexAuth;
use codex_plugin::PluginId;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

mod remote_installed_plugin_sync;
mod share;

pub use remote_installed_plugin_sync::RemoteInstalledPluginBundleSyncError;
pub use remote_installed_plugin_sync::RemoteInstalledPluginBundleSyncOutcome;
pub use remote_installed_plugin_sync::RemotePluginCacheMutationGuard;
pub use remote_installed_plugin_sync::mark_remote_plugin_cache_mutation_in_flight;
pub(crate) use remote_installed_plugin_sync::maybe_start_remote_installed_plugin_bundle_sync;
pub use remote_installed_plugin_sync::sync_remote_installed_plugin_bundles_once;
pub use share::RemotePluginShareAccessPolicy;
pub use share::RemotePluginShareDiscoverability;
pub use share::RemotePluginSharePrincipal;
pub use share::RemotePluginSharePrincipalRole;
pub use share::RemotePluginSharePrincipalType;
pub use share::RemotePluginShareSaveResult;
pub use share::RemotePluginShareTarget;
pub use share::RemotePluginShareTargetRole;
pub use share::RemotePluginShareUpdateDiscoverability;
pub use share::RemotePluginShareUpdateTargetsResult;
pub use share::checkout_remote_plugin_share;
pub use share::delete_remote_plugin_share;
pub use share::list_remote_plugin_shares;
pub use share::load_plugin_share_remote_ids_by_local_path;
pub use share::save_remote_plugin_share;
pub use share::update_remote_plugin_share_targets;

pub const REMOTE_GLOBAL_MARKETPLACE_NAME: &str = "astral-curated-remote";
pub const REMOTE_WORKSPACE_MARKETPLACE_NAME: &str = "workspace-directory";
pub const REMOTE_WORKSPACE_SHARED_WITH_ME_MARKETPLACE_NAME: &str = "workspace-shared-with-me";
pub const REMOTE_WORKSPACE_SHARED_WITH_ME_PRIVATE_MARKETPLACE_NAME: &str =
    "workspace-shared-with-me-private";
pub const REMOTE_WORKSPACE_SHARED_WITH_ME_UNLISTED_MARKETPLACE_NAME: &str =
    "workspace-shared-with-me-unlisted";
pub const REMOTE_GLOBAL_MARKETPLACE_DISPLAY_NAME: &str = "Astral Curated Remote";
pub const REMOTE_WORKSPACE_MARKETPLACE_DISPLAY_NAME: &str = "Workspace Directory";
pub const REMOTE_WORKSPACE_SHARED_WITH_ME_PRIVATE_MARKETPLACE_DISPLAY_NAME: &str = "Shared with me";
pub const REMOTE_WORKSPACE_SHARED_WITH_ME_UNLISTED_MARKETPLACE_DISPLAY_NAME: &str =
    "Shared with me (unlisted)";

const INVALID_REQUEST_ERROR_CODE: i64 = -32600;
const REMOTE_INSTALLED_MARKETPLACE_DISPLAY_ORDER: [(&str, &str); 5] = [
    (
        REMOTE_GLOBAL_MARKETPLACE_NAME,
        REMOTE_GLOBAL_MARKETPLACE_DISPLAY_NAME,
    ),
    (
        REMOTE_WORKSPACE_MARKETPLACE_NAME,
        REMOTE_WORKSPACE_MARKETPLACE_DISPLAY_NAME,
    ),
    (
        REMOTE_WORKSPACE_SHARED_WITH_ME_MARKETPLACE_NAME,
        REMOTE_WORKSPACE_SHARED_WITH_ME_PRIVATE_MARKETPLACE_DISPLAY_NAME,
    ),
    (
        REMOTE_WORKSPACE_SHARED_WITH_ME_PRIVATE_MARKETPLACE_NAME,
        REMOTE_WORKSPACE_SHARED_WITH_ME_PRIVATE_MARKETPLACE_DISPLAY_NAME,
    ),
    (
        REMOTE_WORKSPACE_SHARED_WITH_ME_UNLISTED_MARKETPLACE_NAME,
        REMOTE_WORKSPACE_SHARED_WITH_ME_UNLISTED_MARKETPLACE_DISPLAY_NAME,
    ),
];

pub fn remote_plugin_background_sync_available() -> bool {
    false
}

fn remote_plugin_control_plane_disabled_error() -> RemotePluginCatalogError {
    RemotePluginCatalogError::ControlPlaneDisabled
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePluginServiceConfig {
    pub hosted_base_url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteMarketplace {
    pub name: String,
    pub display_name: String,
    pub plugins: Vec<RemotePluginSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteMarketplaceSource {
    Global,
    WorkspaceDirectory,
    SharedWithMe,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteInstalledPlugin {
    pub marketplace_name: String,
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub install_policy: PluginInstallPolicy,
    pub auth_policy: PluginAuthPolicy,
    pub availability: PluginAvailability,
    pub interface: Option<PluginInterface>,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemotePluginSummary {
    pub id: String,
    pub remote_plugin_id: String,
    pub name: String,
    pub share_context: Option<RemotePluginShareContext>,
    pub installed: bool,
    pub enabled: bool,
    pub install_policy: PluginInstallPolicy,
    pub auth_policy: PluginAuthPolicy,
    pub availability: PluginAvailability,
    pub interface: Option<PluginInterface>,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePluginShareContext {
    pub remote_plugin_id: String,
    pub remote_version: Option<String>,
    pub discoverability: RemotePluginShareDiscoverability,
    pub share_url: Option<String>,
    pub creator_account_user_id: Option<String>,
    pub creator_name: Option<String>,
    pub share_principals: Option<Vec<RemotePluginSharePrincipal>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemotePluginShareSummary {
    pub summary: RemotePluginSummary,
    pub local_plugin_path: Option<AbsolutePathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemotePluginDetail {
    pub marketplace_name: String,
    pub marketplace_display_name: String,
    pub summary: RemotePluginSummary,
    pub description: Option<String>,
    pub release_version: Option<String>,
    pub bundle_download_url: Option<String>,
    pub app_manifest: Option<JsonValue>,
    pub skills: Vec<RemotePluginSkill>,
    pub app_ids: Vec<String>,
    pub app_templates: Vec<RemoteAppTemplate>,
    pub mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAppTemplate {
    pub template_id: String,
    pub name: String,
    pub description: Option<String>,
    pub canonical_connector_id: Option<String>,
    pub logo_url: Option<String>,
    pub logo_url_dark: Option<String>,
    pub materialized_app_ids: Vec<String>,
    pub reason: Option<RemoteAppTemplateUnavailableReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RemoteAppTemplateUnavailableReason {
    NotConfiguredForWorkspace,
    NoActiveWorkspace,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemotePluginSkill {
    pub name: String,
    pub description: String,
    pub short_description: Option<String>,
    pub interface: Option<SkillInterface>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemotePluginSkillDetail {
    pub contents: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteDiscoverablePlugin {
    pub config_id: String,
    pub remote_plugin_id: String,
    pub name: String,
    pub description: Option<String>,
    pub has_skills: bool,
    pub app_ids: Vec<String>,
    pub install_policy: PluginInstallPolicy,
    pub availability: PluginAvailability,
}

pub fn is_valid_remote_plugin_id(plugin_id: &str) -> bool {
    !plugin_id.is_empty()
        && plugin_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '~')
}

pub fn validate_remote_plugin_id(plugin_id: &str) -> Result<(), JSONRPCErrorError> {
    if !is_valid_remote_plugin_id(plugin_id) {
        return Err(JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message:
                "invalid remote plugin id: only ASCII letters, digits, `_`, `-`, and `~` are allowed"
                    .to_string(),
            data: None,
        });
    }

    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum RemotePluginCatalogError {
    #[error("legacy hosted remote plugin control-plane is disabled in Astral")]
    ControlPlaneDisabled,

    #[error("hosted authentication required for legacy remote plugin catalog")]
    AuthRequired,

    #[error(
        "hosted authentication required for legacy remote plugin catalog; api key auth is not supported"
    )]
    UnsupportedAuthMode,

    #[error("failed to read auth token for remote plugin catalog: {0}")]
    AuthToken(#[source] std::io::Error),

    #[error("failed to send remote plugin catalog request to {url}: {source}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("remote plugin catalog request to {url} failed with status {status}: {body}")]
    UnexpectedStatus {
        url: String,
        status: reqwest::StatusCode,
        body: String,
    },

    #[error("failed to parse remote plugin catalog response from {url}: {source}")]
    Decode {
        url: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("invalid remote plugin catalog base URL: {0}")]
    InvalidBaseUrl(#[source] url::ParseError),

    #[error("invalid remote plugin catalog base URL path")]
    InvalidBaseUrlPath,

    #[error("remote marketplace `{marketplace_name}` is not supported")]
    UnknownMarketplace { marketplace_name: String },

    #[error(
        "remote plugin mutation returned unexpected plugin id: expected `{expected}`, got `{actual}`"
    )]
    UnexpectedPluginId { expected: String, actual: String },

    #[error(
        "remote plugin skill response returned unexpected skill name: expected `{expected}`, got `{actual}`"
    )]
    UnexpectedSkillName { expected: String, actual: String },

    #[error(
        "remote plugin mutation returned unexpected enabled state for `{plugin_id}`: expected {expected_enabled}, got {actual_enabled}"
    )]
    UnexpectedEnabledState {
        plugin_id: String,
        expected_enabled: bool,
        actual_enabled: bool,
    },

    #[error("invalid plugin path `{path}`: {reason}")]
    InvalidPluginPath { path: PathBuf, reason: String },

    #[error("remote plugin `{remote_plugin_id}` is not available for plugin/share/checkout")]
    PluginShareCheckoutNotAvailable { remote_plugin_id: String },

    #[error("failed to archive plugin at `{path}`: {source}")]
    Archive {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to join plugin archive task: {0}")]
    ArchiveJoin(#[source] tokio::task::JoinError),

    #[error(
        "plugin archive would be {bytes} bytes, exceeding the maximum upload size of {max_bytes} bytes"
    )]
    ArchiveTooLarge { bytes: usize, max_bytes: usize },

    #[error("workspace plugin upload response did not include an etag")]
    MissingUploadEtag,

    #[error("{0}")]
    UnexpectedResponse(String),

    #[error("{0}")]
    CacheRemove(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub enum RemotePluginScope {
    #[serde(rename = "GLOBAL")]
    Global,
    #[serde(rename = "WORKSPACE")]
    Workspace,
}

impl RemotePluginScope {
    fn from_marketplace_name(name: &str) -> Option<Self> {
        match name {
            REMOTE_GLOBAL_MARKETPLACE_NAME => Some(Self::Global),
            REMOTE_WORKSPACE_MARKETPLACE_NAME
            | REMOTE_WORKSPACE_SHARED_WITH_ME_MARKETPLACE_NAME
            | REMOTE_WORKSPACE_SHARED_WITH_ME_PRIVATE_MARKETPLACE_NAME
            | REMOTE_WORKSPACE_SHARED_WITH_ME_UNLISTED_MARKETPLACE_NAME => Some(Self::Workspace),
            _ => None,
        }
    }
}

pub async fn fetch_remote_marketplaces(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _sources: &[RemoteMarketplaceSource],
    _global_catalog_cache_path: Option<&Path>,
) -> Result<Vec<RemoteMarketplace>, RemotePluginCatalogError> {
    Err(remote_plugin_control_plane_disabled_error())
}

pub async fn fetch_and_cache_global_remote_plugin_catalog(
    _codex_home: &Path,
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
) -> Result<(), RemotePluginCatalogError> {
    Err(remote_plugin_control_plane_disabled_error())
}

pub fn has_cached_global_remote_plugin_catalog(
    _codex_home: &Path,
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
) -> bool {
    false
}

pub fn cached_global_remote_discoverable_plugins(
    _codex_home: &Path,
    _config: &RemotePluginServiceConfig,
    _auth: &CodexAuth,
) -> Vec<RemoteDiscoverablePlugin> {
    Vec::new()
}

pub async fn fetch_astral_curated_remote_collection_marketplace(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
) -> Result<Option<RemoteMarketplace>, RemotePluginCatalogError> {
    Err(remote_plugin_control_plane_disabled_error())
}

pub(crate) async fn fetch_remote_installed_plugins(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
) -> Result<Vec<RemoteInstalledPlugin>, RemotePluginCatalogError> {
    Err(remote_plugin_control_plane_disabled_error())
}

pub fn group_remote_installed_plugins_by_marketplaces(
    plugins: &[RemoteInstalledPlugin],
    visible_scopes: &[RemotePluginScope],
) -> Vec<RemoteMarketplace> {
    let mut plugins_by_marketplace = BTreeMap::<String, Vec<RemotePluginSummary>>::new();

    for plugin in plugins {
        if !RemotePluginScope::from_marketplace_name(&plugin.marketplace_name)
            .is_some_and(|scope| visible_scopes.contains(&scope))
        {
            continue;
        }
        let Ok(plugin_id) = PluginId::new(plugin.name.clone(), plugin.marketplace_name.clone())
        else {
            continue;
        };
        let plugin_summary = RemotePluginSummary {
            id: plugin_id.as_key(),
            remote_plugin_id: plugin.id.clone(),
            name: plugin.name.clone(),
            share_context: None,
            installed: true,
            enabled: plugin.enabled,
            install_policy: plugin.install_policy,
            auth_policy: plugin.auth_policy,
            availability: plugin.availability,
            interface: plugin.interface.clone(),
            keywords: plugin.keywords.clone(),
        };
        plugins_by_marketplace
            .entry(plugin.marketplace_name.clone())
            .or_default()
            .push(plugin_summary);
    }

    REMOTE_INSTALLED_MARKETPLACE_DISPLAY_ORDER
        .into_iter()
        .filter_map(|(marketplace_name, display_name)| {
            let mut marketplace_plugins = plugins_by_marketplace.remove(marketplace_name)?;
            sort_remote_plugin_summaries_by_display_name(&mut marketplace_plugins);
            Some(RemoteMarketplace {
                name: marketplace_name.to_string(),
                display_name: display_name.to_string(),
                plugins: marketplace_plugins,
            })
        })
        .collect()
}

pub async fn fetch_remote_plugin_detail(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _marketplace_name: &str,
    _plugin_id: &str,
) -> Result<RemotePluginDetail, RemotePluginCatalogError> {
    Err(remote_plugin_control_plane_disabled_error())
}

pub async fn fetch_remote_plugin_share_context(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _plugin_id: &str,
) -> Result<Option<RemotePluginShareContext>, RemotePluginCatalogError> {
    Err(remote_plugin_control_plane_disabled_error())
}

pub async fn fetch_remote_plugin_detail_with_download_urls(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _marketplace_name: &str,
    _plugin_id: &str,
) -> Result<RemotePluginDetail, RemotePluginCatalogError> {
    Err(remote_plugin_control_plane_disabled_error())
}

pub async fn fetch_remote_plugin_skill_detail(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _marketplace_name: &str,
    _plugin_id: &str,
    _skill_name: &str,
) -> Result<RemotePluginSkillDetail, RemotePluginCatalogError> {
    Err(remote_plugin_control_plane_disabled_error())
}

pub async fn install_remote_plugin(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _marketplace_name: &str,
    _plugin_id: &str,
) -> Result<(), RemotePluginCatalogError> {
    Err(remote_plugin_control_plane_disabled_error())
}

pub async fn uninstall_remote_plugin(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _codex_home: PathBuf,
    _plugin_id: &str,
) -> Result<(), RemotePluginCatalogError> {
    Err(remote_plugin_control_plane_disabled_error())
}

fn remote_plugin_display_name(plugin: &RemotePluginSummary) -> &str {
    plugin
        .interface
        .as_ref()
        .and_then(|interface| interface.display_name.as_deref())
        .unwrap_or(&plugin.name)
}

fn sort_remote_plugin_summaries_by_display_name(plugins: &mut [RemotePluginSummary]) {
    plugins.sort_by(|left, right| {
        let left_display_name = remote_plugin_display_name(left);
        let right_display_name = remote_plugin_display_name(right);
        left_display_name
            .to_ascii_lowercase()
            .cmp(&right_display_name.to_ascii_lowercase())
            .then_with(|| left_display_name.cmp(right_display_name))
            .then_with(|| left.id.cmp(&right.id))
    });
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
