use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use codex_app_server_protocol::JSONRPCErrorError;
use serde::Deserialize;
use serde::Serialize;
use tokio::io;

use crate::CopyOptions;
use crate::CreateDirectoryOptions;
use crate::ExecutorFileSystem;
use crate::RemoveOptions;
use crate::local_file_system::DirectFileSystem;
use crate::protocol::FS_CANONICALIZE_METHOD;
use crate::protocol::FS_COPY_METHOD;
use crate::protocol::FS_CREATE_DIRECTORY_METHOD;
use crate::protocol::FS_GET_METADATA_METHOD;
use crate::protocol::FS_GLOB_METHOD;
use crate::protocol::FS_GREP_METHOD;
use crate::protocol::FS_READ_DIRECTORY_METHOD;
use crate::protocol::FS_READ_FILE_METHOD;
use crate::protocol::FS_REMOVE_METHOD;
use crate::protocol::FS_WRITE_FILE_METHOD;
use crate::protocol::FsCanonicalizeParams;
use crate::protocol::FsCanonicalizeResponse;
use crate::protocol::FsCopyParams;
use crate::protocol::FsCopyResponse;
use crate::protocol::FsCreateDirectoryParams;
use crate::protocol::FsCreateDirectoryResponse;
use crate::protocol::FsGetMetadataParams;
use crate::protocol::FsGetMetadataResponse;
use crate::protocol::FsGlobParams;
use crate::protocol::FsGlobResponse;
use crate::protocol::FsGrepParams;
use crate::protocol::FsGrepResponse;
use crate::protocol::FsReadDirectoryEntry;
use crate::protocol::FsReadDirectoryParams;
use crate::protocol::FsReadDirectoryResponse;
use crate::protocol::FsReadFileParams;
use crate::protocol::FsReadFileResponse;
use crate::protocol::FsRemoveParams;
use crate::protocol::FsRemoveResponse;
use crate::protocol::FsWriteFileParams;
use crate::protocol::FsWriteFileResponse;
use crate::rpc::internal_error;
use crate::rpc::invalid_request;
use crate::rpc::not_found;

pub const CODEX_FS_HELPER_ARG1: &str = "--codex-run-as-fs-helper";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "params")]
pub(crate) enum FsHelperRequest {
    #[serde(rename = "fs/readFile")]
    ReadFile(FsReadFileParams),
    #[serde(rename = "fs/writeFile")]
    WriteFile(FsWriteFileParams),
    #[serde(rename = "fs/createDirectory")]
    CreateDirectory(FsCreateDirectoryParams),
    #[serde(rename = "fs/getMetadata")]
    GetMetadata(FsGetMetadataParams),
    #[serde(rename = "fs/canonicalize")]
    Canonicalize(FsCanonicalizeParams),
    #[serde(rename = "fs/readDirectory")]
    ReadDirectory(FsReadDirectoryParams),
    #[serde(rename = "fs/remove")]
    Remove(FsRemoveParams),
    #[serde(rename = "fs/copy")]
    Copy(FsCopyParams),
    #[serde(rename = "fs/glob")]
    Glob(FsGlobParams),
    #[serde(rename = "fs/grep")]
    Grep(FsGrepParams),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", content = "payload", rename_all = "camelCase")]
pub(crate) enum FsHelperResponse {
    Ok(FsHelperPayload),
    Error(JSONRPCErrorError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "response")]
pub(crate) enum FsHelperPayload {
    #[serde(rename = "fs/readFile")]
    ReadFile(FsReadFileResponse),
    #[serde(rename = "fs/writeFile")]
    WriteFile(FsWriteFileResponse),
    #[serde(rename = "fs/createDirectory")]
    CreateDirectory(FsCreateDirectoryResponse),
    #[serde(rename = "fs/getMetadata")]
    GetMetadata(FsGetMetadataResponse),
    #[serde(rename = "fs/canonicalize")]
    Canonicalize(FsCanonicalizeResponse),
    #[serde(rename = "fs/readDirectory")]
    ReadDirectory(FsReadDirectoryResponse),
    #[serde(rename = "fs/remove")]
    Remove(FsRemoveResponse),
    #[serde(rename = "fs/copy")]
    Copy(FsCopyResponse),
    #[serde(rename = "fs/glob")]
    Glob(FsGlobResponse),
    #[serde(rename = "fs/grep")]
    Grep(FsGrepResponse),
}

impl FsHelperPayload {
    fn operation(&self) -> &'static str {
        match self {
            Self::ReadFile(_) => FS_READ_FILE_METHOD,
            Self::WriteFile(_) => FS_WRITE_FILE_METHOD,
            Self::CreateDirectory(_) => FS_CREATE_DIRECTORY_METHOD,
            Self::GetMetadata(_) => FS_GET_METADATA_METHOD,
            Self::Canonicalize(_) => FS_CANONICALIZE_METHOD,
            Self::ReadDirectory(_) => FS_READ_DIRECTORY_METHOD,
            Self::Remove(_) => FS_REMOVE_METHOD,
            Self::Copy(_) => FS_COPY_METHOD,
            Self::Glob(_) => FS_GLOB_METHOD,
            Self::Grep(_) => FS_GREP_METHOD,
        }
    }

    pub(crate) fn expect_read_file(self) -> Result<FsReadFileResponse, JSONRPCErrorError> {
        match self {
            Self::ReadFile(response) => Ok(response),
            other => Err(unexpected_response(FS_READ_FILE_METHOD, other.operation())),
        }
    }

    pub(crate) fn expect_write_file(self) -> Result<FsWriteFileResponse, JSONRPCErrorError> {
        match self {
            Self::WriteFile(response) => Ok(response),
            other => Err(unexpected_response(FS_WRITE_FILE_METHOD, other.operation())),
        }
    }

    pub(crate) fn expect_create_directory(
        self,
    ) -> Result<FsCreateDirectoryResponse, JSONRPCErrorError> {
        match self {
            Self::CreateDirectory(response) => Ok(response),
            other => Err(unexpected_response(
                FS_CREATE_DIRECTORY_METHOD,
                other.operation(),
            )),
        }
    }

    pub(crate) fn expect_get_metadata(self) -> Result<FsGetMetadataResponse, JSONRPCErrorError> {
        match self {
            Self::GetMetadata(response) => Ok(response),
            other => Err(unexpected_response(
                FS_GET_METADATA_METHOD,
                other.operation(),
            )),
        }
    }

    pub(crate) fn expect_canonicalize(self) -> Result<FsCanonicalizeResponse, JSONRPCErrorError> {
        match self {
            Self::Canonicalize(response) => Ok(response),
            other => Err(unexpected_response(
                FS_CANONICALIZE_METHOD,
                other.operation(),
            )),
        }
    }

    pub(crate) fn expect_read_directory(
        self,
    ) -> Result<FsReadDirectoryResponse, JSONRPCErrorError> {
        match self {
            Self::ReadDirectory(response) => Ok(response),
            other => Err(unexpected_response(
                FS_READ_DIRECTORY_METHOD,
                other.operation(),
            )),
        }
    }

    pub(crate) fn expect_remove(self) -> Result<FsRemoveResponse, JSONRPCErrorError> {
        match self {
            Self::Remove(response) => Ok(response),
            other => Err(unexpected_response(FS_REMOVE_METHOD, other.operation())),
        }
    }

    pub(crate) fn expect_copy(self) -> Result<FsCopyResponse, JSONRPCErrorError> {
        match self {
            Self::Copy(response) => Ok(response),
            other => Err(unexpected_response(FS_COPY_METHOD, other.operation())),
        }
    }

    pub(crate) fn expect_glob(self) -> Result<FsGlobResponse, JSONRPCErrorError> {
        match self {
            Self::Glob(response) => Ok(response),
            other => Err(unexpected_response(FS_GLOB_METHOD, other.operation())),
        }
    }

    pub(crate) fn expect_grep(self) -> Result<FsGrepResponse, JSONRPCErrorError> {
        match self {
            Self::Grep(response) => Ok(response),
            other => Err(unexpected_response(FS_GREP_METHOD, other.operation())),
        }
    }
}

fn unexpected_response(expected: &str, actual: &str) -> JSONRPCErrorError {
    internal_error(format!(
        "unexpected fs sandbox helper response: expected {expected}, got {actual}"
    ))
}

pub(crate) async fn run_direct_request(
    request: FsHelperRequest,
) -> Result<FsHelperPayload, JSONRPCErrorError> {
    let file_system = DirectFileSystem;
    match request {
        FsHelperRequest::ReadFile(params) => {
            let data = file_system
                .read_file(&params.path, /*sandbox*/ None)
                .await
                .map_err(map_fs_error)?;
            Ok(FsHelperPayload::ReadFile(FsReadFileResponse {
                data_base64: STANDARD.encode(data),
            }))
        }
        FsHelperRequest::WriteFile(params) => {
            let bytes = STANDARD.decode(params.data_base64).map_err(|err| {
                invalid_request(format!(
                    "{FS_WRITE_FILE_METHOD} requires valid base64 dataBase64: {err}"
                ))
            })?;
            file_system
                .write_file(&params.path, bytes, /*sandbox*/ None)
                .await
                .map_err(map_fs_error)?;
            Ok(FsHelperPayload::WriteFile(FsWriteFileResponse {}))
        }
        FsHelperRequest::CreateDirectory(params) => {
            file_system
                .create_directory(
                    &params.path,
                    CreateDirectoryOptions {
                        recursive: params.recursive.unwrap_or(true),
                    },
                    /*sandbox*/ None,
                )
                .await
                .map_err(map_fs_error)?;
            Ok(FsHelperPayload::CreateDirectory(
                FsCreateDirectoryResponse {},
            ))
        }
        FsHelperRequest::GetMetadata(params) => {
            let metadata = file_system
                .get_metadata(&params.path, /*sandbox*/ None)
                .await
                .map_err(map_fs_error)?;
            Ok(FsHelperPayload::GetMetadata(FsGetMetadataResponse {
                is_directory: metadata.is_directory,
                is_file: metadata.is_file,
                is_symlink: metadata.is_symlink,
                size: metadata.size,
                created_at_ms: metadata.created_at_ms,
                modified_at_ms: metadata.modified_at_ms,
            }))
        }
        FsHelperRequest::Canonicalize(params) => {
            let path = file_system
                .canonicalize(&params.path, /*sandbox*/ None)
                .await
                .map_err(map_fs_error)?;
            Ok(FsHelperPayload::Canonicalize(FsCanonicalizeResponse {
                path,
            }))
        }
        FsHelperRequest::ReadDirectory(params) => {
            let entries = file_system
                .read_directory(&params.path, /*sandbox*/ None)
                .await
                .map_err(map_fs_error)?
                .into_iter()
                .map(|entry| FsReadDirectoryEntry {
                    file_name: entry.file_name,
                    is_directory: entry.is_directory,
                    is_file: entry.is_file,
                })
                .collect();
            Ok(FsHelperPayload::ReadDirectory(FsReadDirectoryResponse {
                entries,
            }))
        }
        FsHelperRequest::Remove(params) => {
            file_system
                .remove(
                    &params.path,
                    RemoveOptions {
                        recursive: params.recursive.unwrap_or(true),
                        force: params.force.unwrap_or(true),
                    },
                    /*sandbox*/ None,
                )
                .await
                .map_err(map_fs_error)?;
            Ok(FsHelperPayload::Remove(FsRemoveResponse {}))
        }
        FsHelperRequest::Copy(params) => {
            file_system
                .copy(
                    &params.source_path,
                    &params.destination_path,
                    CopyOptions {
                        recursive: params.recursive,
                    },
                    /*sandbox*/ None,
                )
                .await
                .map_err(map_fs_error)?;
            Ok(FsHelperPayload::Copy(FsCopyResponse {}))
        }
        FsHelperRequest::Glob(params) => {
            let response = file_system
                .glob_search(params.request, /*sandbox*/ None)
                .await
                .map_err(map_fs_error)?;
            Ok(FsHelperPayload::Glob(FsGlobResponse { response }))
        }
        FsHelperRequest::Grep(params) => {
            let response = file_system
                .grep_search(params.request, /*sandbox*/ None)
                .await
                .map_err(map_fs_error)?;
            Ok(FsHelperPayload::Grep(FsGrepResponse { response }))
        }
    }
}

fn map_fs_error(err: io::Error) -> JSONRPCErrorError {
    match err.kind() {
        io::ErrorKind::NotFound => not_found(err.to_string()),
        io::ErrorKind::InvalidInput | io::ErrorKind::PermissionDenied => {
            invalid_request(err.to_string())
        }
        _ => internal_error(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use codex_utils_path_uri::PathUri;
    use serde_json::json;

    use super::*;

    #[test]
    fn helper_protocol_uses_path_uris() -> serde_json::Result<()> {
        let local_path =
            PathUri::from_host_native_path(std::env::current_dir().expect("cwd").join("file"))
                .expect("path URI");
        let paths = [
            local_path,
            PathUri::parse("file://server/share/file").expect("path URI"),
        ];

        for path in paths {
            let expected_path = path.to_string();
            let request = serde_json::to_value(FsHelperRequest::WriteFile(FsWriteFileParams {
                path: path.clone(),
                data_base64: String::new(),
                sandbox: None,
            }))?;
            assert_eq!(
                request,
                json!({
                    "operation": FS_WRITE_FILE_METHOD,
                    "params": {
                        "path": expected_path.as_str(),
                        "dataBase64": "",
                        "sandbox": null,
                    },
                }),
            );

            let response = serde_json::to_value(FsHelperResponse::Ok(
                FsHelperPayload::Canonicalize(FsCanonicalizeResponse { path }),
            ))?;
            assert_eq!(
                response,
                json!({
                    "status": "ok",
                    "payload": {
                        "operation": FS_CANONICALIZE_METHOD,
                        "response": {
                            "path": expected_path.as_str(),
                        },
                    },
                }),
            );
        }

        Ok(())
    }

    #[test]
    fn helper_requests_use_fs_method_names() -> serde_json::Result<()> {
        let root = absolute_test_path("fs-helper-root");
        assert_eq!(
            serde_json::to_value(FsHelperRequest::WriteFile(FsWriteFileParams {
                path: root.join("file").expect("file URI"),
                data_base64: String::new(),
                sandbox: None,
            }))?["operation"],
            FS_WRITE_FILE_METHOD,
        );
        assert_eq!(
            serde_json::to_value(FsHelperRequest::Glob(FsGlobParams {
                request: crate::GlobSearchRequest {
                    root: root.clone(),
                    pattern: "**/*.rs".to_string(),
                    max_results: 10,
                },
                sandbox: None,
            }))?["operation"],
            FS_GLOB_METHOD,
        );
        assert_eq!(
            serde_json::to_value(FsHelperRequest::Grep(FsGrepParams {
                request: crate::GrepSearchRequest {
                    root,
                    pattern: "needle".to_string(),
                    glob: Some("*.rs".to_string()),
                    file_type: None,
                    output_mode: crate::GrepOutputMode::FilesWithMatches,
                    context_before: 0,
                    context_after: 0,
                    line_numbers: false,
                    ignore_case: false,
                    head_limit: 250,
                    offset: 0,
                    multiline: false,
                },
                sandbox: None,
            }))?["operation"],
            FS_GREP_METHOD,
        );
        Ok(())
    }

    #[test]
    fn helper_payload_expect_accepts_search_responses() {
        let glob_response = FsHelperPayload::Glob(FsGlobResponse {
            response: crate::GlobSearchResponse {
                matches: Vec::new(),
                truncated: false,
            },
        })
        .expect_glob()
        .expect("glob payload should match");
        assert_eq!(
            glob_response,
            FsGlobResponse {
                response: crate::GlobSearchResponse {
                    matches: Vec::new(),
                    truncated: false,
                },
            }
        );

        let grep_response = FsHelperPayload::Grep(FsGrepResponse {
            response: crate::GrepSearchResponse {
                lines: vec!["src/main.rs".to_string()],
                num_files: 1,
                num_matches: None,
                applied_limit: None,
                applied_offset: None,
                truncated: false,
            },
        })
        .expect_grep()
        .expect("grep payload should match");
        assert_eq!(
            grep_response,
            FsGrepResponse {
                response: crate::GrepSearchResponse {
                    lines: vec!["src/main.rs".to_string()],
                    num_files: 1,
                    num_matches: None,
                    applied_limit: None,
                    applied_offset: None,
                    truncated: false,
                },
            }
        );
    }

    fn absolute_test_path(name: &str) -> PathUri {
        PathUri::from_host_native_path(std::env::current_dir().expect("cwd").join(name))
            .expect("path URI")
    }
}
