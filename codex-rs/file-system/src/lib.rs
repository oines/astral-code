mod find_up;

pub use find_up::FindUpErrorPolicy;
pub use find_up::find_nearest_ancestor_with_markers;
pub use find_up::find_nearest_native_ancestor_with_markers;

use async_trait::async_trait;
use codex_protocol::config_types::WindowsSandboxLevel;
use codex_protocol::models::PermissionProfile;
use codex_protocol::models::SandboxEnforcement;
use codex_protocol::permissions::FileSystemPath;
use codex_protocol::permissions::FileSystemSandboxKind;
use codex_protocol::permissions::FileSystemSandboxPolicy;
use codex_protocol::permissions::FileSystemSpecialPath;
use codex_protocol::permissions::NetworkSandboxPolicy;
use codex_protocol::protocol::SandboxPolicy;
use codex_utils_path_uri::PathUri;
use std::io;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CreateDirectoryOptions {
    pub recursive: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoveOptions {
    pub recursive: bool,
    pub force: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CopyOptions {
    pub recursive: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileMetadata {
    pub is_directory: bool,
    pub is_file: bool,
    pub is_symlink: bool,
    pub size: Option<u64>,
    pub created_at_ms: i64,
    pub modified_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadDirectoryEntry {
    pub file_name: String,
    pub is_directory: bool,
    pub is_file: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobSearchRequest {
    pub root: PathUri,
    pub pattern: String,
    pub max_results: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobSearchMatch {
    pub path: PathUri,
    pub modified_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobSearchResponse {
    pub matches: Vec<GlobSearchMatch>,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrepOutputMode {
    Content,
    FilesWithMatches,
    Count,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepSearchRequest {
    pub root: PathUri,
    pub pattern: String,
    pub glob: Option<String>,
    pub file_type: Option<String>,
    pub output_mode: GrepOutputMode,
    pub context_before: usize,
    pub context_after: usize,
    pub line_numbers: bool,
    pub ignore_case: bool,
    pub head_limit: usize,
    pub offset: usize,
    pub multiline: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepSearchResponse {
    pub lines: Vec<String>,
    pub num_files: usize,
    pub num_matches: Option<usize>,
    pub applied_limit: Option<usize>,
    pub applied_offset: Option<usize>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSystemSandboxContext {
    pub permissions: PermissionProfile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathUri>,
    pub windows_sandbox_level: WindowsSandboxLevel,
    #[serde(default)]
    pub windows_sandbox_private_desktop: bool,
    #[serde(default)]
    pub use_legacy_landlock: bool,
}

impl FileSystemSandboxContext {
    pub fn from_legacy_sandbox_policy(
        sandbox_policy: SandboxPolicy,
        cwd: PathUri,
    ) -> io::Result<Self> {
        // Legacy policy projection materializes native roots, so convert at the receiving-host
        // boundary while retaining the URI in the resulting sandbox context.
        let native_cwd = cwd.to_abs_path()?;
        let file_system_sandbox_policy =
            FileSystemSandboxPolicy::from_legacy_sandbox_policy_for_cwd(
                &sandbox_policy,
                &native_cwd,
            );
        let permissions = PermissionProfile::from_runtime_permissions_with_enforcement(
            SandboxEnforcement::from_legacy_sandbox_policy(&sandbox_policy),
            &file_system_sandbox_policy,
            NetworkSandboxPolicy::from(&sandbox_policy),
        );
        Ok(Self::from_permission_profile_with_cwd(permissions, cwd))
    }

    pub fn from_permission_profile(permissions: PermissionProfile) -> Self {
        Self::from_permissions_and_cwd(permissions, /*cwd*/ None)
    }

    pub fn from_permission_profile_with_cwd(permissions: PermissionProfile, cwd: PathUri) -> Self {
        Self::from_permissions_and_cwd(permissions, Some(cwd))
    }

    fn from_permissions_and_cwd(permissions: PermissionProfile, cwd: Option<PathUri>) -> Self {
        Self {
            permissions,
            cwd,
            windows_sandbox_level: WindowsSandboxLevel::Disabled,
            windows_sandbox_private_desktop: false,
            use_legacy_landlock: false,
        }
    }

    pub fn should_run_in_sandbox(&self) -> bool {
        let file_system_policy = self.permissions.file_system_sandbox_policy();
        matches!(file_system_policy.kind, FileSystemSandboxKind::Restricted)
            && !file_system_policy.has_full_disk_write_access()
    }

    pub fn has_cwd_dependent_permissions(&self) -> bool {
        let file_system_policy = self.permissions.file_system_sandbox_policy();
        file_system_policy_has_cwd_dependent_entries(&file_system_policy)
    }

    pub fn drop_cwd_if_unused(mut self) -> Self {
        if !self.has_cwd_dependent_permissions() {
            self.cwd = None;
        }
        self
    }
}

fn file_system_policy_has_cwd_dependent_entries(
    file_system_policy: &FileSystemSandboxPolicy,
) -> bool {
    file_system_policy
        .entries
        .iter()
        .any(|entry| match &entry.path {
            FileSystemPath::GlobPattern { pattern } => !Path::new(pattern).is_absolute(),
            FileSystemPath::Special {
                value: FileSystemSpecialPath::ProjectRoots { .. },
            } => true,
            FileSystemPath::Path { .. } | FileSystemPath::Special { .. } => false,
        })
}

pub type FileSystemResult<T> = io::Result<T>;

/// Abstract filesystem access used by components that may operate locally or via
/// a remote environment.
#[async_trait]
pub trait ExecutorFileSystem: Send + Sync {
    /// Resolves a path within this filesystem.
    async fn canonicalize(
        &self,
        path: &PathUri,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<PathUri>;

    async fn read_file(
        &self,
        path: &PathUri,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<Vec<u8>>;

    /// Reads a file and decodes it as UTF-8 text.
    async fn read_file_text(
        &self,
        path: &PathUri,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<String> {
        let bytes = self.read_file(path, sandbox).await?;
        String::from_utf8(bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    async fn write_file(
        &self,
        path: &PathUri,
        contents: Vec<u8>,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()>;

    async fn create_directory(
        &self,
        path: &PathUri,
        create_directory_options: CreateDirectoryOptions,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()>;

    async fn get_metadata(
        &self,
        path: &PathUri,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<FileMetadata>;

    async fn read_directory(
        &self,
        path: &PathUri,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<Vec<ReadDirectoryEntry>>;

    async fn remove(
        &self,
        path: &PathUri,
        remove_options: RemoveOptions,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()>;

    async fn copy(
        &self,
        source_path: &PathUri,
        destination_path: &PathUri,
        copy_options: CopyOptions,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()>;

    async fn glob_search(
        &self,
        request: GlobSearchRequest,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<GlobSearchResponse>;

    async fn grep_search(
        &self,
        request: GrepSearchRequest,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<GrepSearchResponse>;
}
