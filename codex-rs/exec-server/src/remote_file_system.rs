use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::path::Path;
use tokio::io;
use tracing::trace;

use crate::CopyOptions;
use crate::CreateDirectoryOptions;
use crate::ExecServerError;
use crate::ExecutorFileSystem;
use crate::FileMetadata;
use crate::FileSystemResult;
use crate::FileSystemSandboxContext;
use crate::GlobSearchRequest;
use crate::GlobSearchResponse;
use crate::GrepSearchRequest;
use crate::GrepSearchResponse;
use crate::ReadDirectoryEntry;
use crate::RemoveOptions;
use crate::client::LazyRemoteExecServerClient;
use crate::protocol::FsCanonicalizeParams;
use crate::protocol::FsCopyParams;
use crate::protocol::FsCreateDirectoryParams;
use crate::protocol::FsGetMetadataParams;
use crate::protocol::FsGlobParams;
use crate::protocol::FsGrepParams;
use crate::protocol::FsJoinParams;
use crate::protocol::FsParentParams;
use crate::protocol::FsReadDirectoryParams;
use crate::protocol::FsReadFileParams;
use crate::protocol::FsRemoveParams;
use crate::protocol::FsWriteFileParams;

const INVALID_REQUEST_ERROR_CODE: i64 = -32600;
const NOT_FOUND_ERROR_CODE: i64 = -32004;

pub(crate) struct RemoteFileSystem {
    client: LazyRemoteExecServerClient,
}

impl RemoteFileSystem {
    pub(crate) fn new(client: LazyRemoteExecServerClient) -> Self {
        trace!("remote fs new");
        Self { client }
    }
}

#[async_trait]
impl ExecutorFileSystem for RemoteFileSystem {
    async fn canonicalize(
        &self,
        path: &AbsolutePathBuf,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<AbsolutePathBuf> {
        trace!("remote fs canonicalize");
        let client = self.client.get().await.map_err(map_remote_error)?;
        let response = client
            .fs_canonicalize(FsCanonicalizeParams {
                path: path.clone(),
                sandbox: remote_sandbox_context(sandbox),
            })
            .await
            .map_err(map_remote_error)?;
        Ok(response.path)
    }

    async fn join(
        &self,
        base_path: &AbsolutePathBuf,
        path: &Path,
    ) -> FileSystemResult<AbsolutePathBuf> {
        trace!("remote fs join");
        let client = self.client.get().await.map_err(map_remote_error)?;
        let response = client
            .fs_join(FsJoinParams {
                base_path: base_path.clone(),
                path: path.to_path_buf(),
            })
            .await
            .map_err(map_remote_error)?;
        Ok(response.path)
    }

    async fn parent(&self, path: &AbsolutePathBuf) -> FileSystemResult<Option<AbsolutePathBuf>> {
        trace!("remote fs parent");
        let client = self.client.get().await.map_err(map_remote_error)?;
        let response = client
            .fs_parent(FsParentParams { path: path.clone() })
            .await
            .map_err(map_remote_error)?;
        Ok(response.path)
    }

    async fn read_file(
        &self,
        path: &AbsolutePathBuf,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<Vec<u8>> {
        trace!("remote fs read_file");
        let client = self.client.get().await.map_err(map_remote_error)?;
        let response = client
            .fs_read_file(FsReadFileParams {
                path: path.clone(),
                sandbox: remote_sandbox_context(sandbox),
            })
            .await
            .map_err(map_remote_error)?;
        STANDARD.decode(response.data_base64).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("remote fs/readFile returned invalid base64 dataBase64: {err}"),
            )
        })
    }

    async fn write_file(
        &self,
        path: &AbsolutePathBuf,
        contents: Vec<u8>,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()> {
        trace!("remote fs write_file");
        let client = self.client.get().await.map_err(map_remote_error)?;
        client
            .fs_write_file(FsWriteFileParams {
                path: path.clone(),
                data_base64: STANDARD.encode(contents),
                sandbox: remote_sandbox_context(sandbox),
            })
            .await
            .map_err(map_remote_error)?;
        Ok(())
    }

    async fn create_directory(
        &self,
        path: &AbsolutePathBuf,
        options: CreateDirectoryOptions,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()> {
        trace!("remote fs create_directory");
        let client = self.client.get().await.map_err(map_remote_error)?;
        client
            .fs_create_directory(FsCreateDirectoryParams {
                path: path.clone(),
                recursive: Some(options.recursive),
                sandbox: remote_sandbox_context(sandbox),
            })
            .await
            .map_err(map_remote_error)?;
        Ok(())
    }

    async fn get_metadata(
        &self,
        path: &AbsolutePathBuf,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<FileMetadata> {
        trace!("remote fs get_metadata");
        let client = self.client.get().await.map_err(map_remote_error)?;
        let response = client
            .fs_get_metadata(FsGetMetadataParams {
                path: path.clone(),
                sandbox: remote_sandbox_context(sandbox),
            })
            .await
            .map_err(map_remote_error)?;
        Ok(FileMetadata {
            is_directory: response.is_directory,
            is_file: response.is_file,
            is_symlink: response.is_symlink,
            created_at_ms: response.created_at_ms,
            modified_at_ms: response.modified_at_ms,
        })
    }

    async fn read_directory(
        &self,
        path: &AbsolutePathBuf,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<Vec<ReadDirectoryEntry>> {
        trace!("remote fs read_directory");
        let client = self.client.get().await.map_err(map_remote_error)?;
        let response = client
            .fs_read_directory(FsReadDirectoryParams {
                path: path.clone(),
                sandbox: remote_sandbox_context(sandbox),
            })
            .await
            .map_err(map_remote_error)?;
        Ok(response
            .entries
            .into_iter()
            .map(|entry| ReadDirectoryEntry {
                file_name: entry.file_name,
                is_directory: entry.is_directory,
                is_file: entry.is_file,
            })
            .collect())
    }

    async fn remove(
        &self,
        path: &AbsolutePathBuf,
        options: RemoveOptions,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()> {
        trace!("remote fs remove");
        let client = self.client.get().await.map_err(map_remote_error)?;
        client
            .fs_remove(FsRemoveParams {
                path: path.clone(),
                recursive: Some(options.recursive),
                force: Some(options.force),
                sandbox: remote_sandbox_context(sandbox),
            })
            .await
            .map_err(map_remote_error)?;
        Ok(())
    }

    async fn copy(
        &self,
        source_path: &AbsolutePathBuf,
        destination_path: &AbsolutePathBuf,
        options: CopyOptions,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()> {
        trace!("remote fs copy");
        let client = self.client.get().await.map_err(map_remote_error)?;
        client
            .fs_copy(FsCopyParams {
                source_path: source_path.clone(),
                destination_path: destination_path.clone(),
                recursive: options.recursive,
                sandbox: remote_sandbox_context(sandbox),
            })
            .await
            .map_err(map_remote_error)?;
        Ok(())
    }

    async fn glob_search(
        &self,
        request: GlobSearchRequest,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<GlobSearchResponse> {
        trace!("remote fs glob_search");
        let client = self.client.get().await.map_err(map_remote_error)?;
        let response = client
            .fs_glob(FsGlobParams {
                request,
                sandbox: remote_sandbox_context(sandbox),
            })
            .await
            .map_err(map_remote_error)?;
        Ok(response.response)
    }

    async fn grep_search(
        &self,
        request: GrepSearchRequest,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<GrepSearchResponse> {
        trace!("remote fs grep_search");
        let client = self.client.get().await.map_err(map_remote_error)?;
        let response = client
            .fs_grep(FsGrepParams {
                request,
                sandbox: remote_sandbox_context(sandbox),
            })
            .await
            .map_err(map_remote_error)?;
        Ok(response.response)
    }
}

fn remote_sandbox_context(
    sandbox: Option<&FileSystemSandboxContext>,
) -> Option<FileSystemSandboxContext> {
    sandbox
        .cloned()
        .map(FileSystemSandboxContext::drop_cwd_if_unused)
}

fn map_remote_error(error: ExecServerError) -> io::Error {
    match error {
        ExecServerError::Server { code, message } if code == NOT_FOUND_ERROR_CODE => {
            io::Error::new(io::ErrorKind::NotFound, message)
        }
        ExecServerError::Server { code, message } if code == INVALID_REQUEST_ERROR_CODE => {
            io::Error::new(io::ErrorKind::InvalidInput, message)
        }
        ExecServerError::Server { message, .. } => io::Error::other(message),
        ExecServerError::Closed | ExecServerError::Disconnected(_) => {
            io::Error::new(io::ErrorKind::BrokenPipe, "exec-server transport closed")
        }
        _ => io::Error::other(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use codex_app_server_protocol::JSONRPCMessage;
    use codex_app_server_protocol::JSONRPCNotification;
    use codex_app_server_protocol::JSONRPCRequest;
    use codex_app_server_protocol::JSONRPCResponse;
    use codex_protocol::models::PermissionProfile;
    use codex_protocol::permissions::FileSystemAccessMode;
    use codex_protocol::permissions::FileSystemPath;
    use codex_protocol::permissions::FileSystemSandboxEntry;
    use codex_protocol::permissions::FileSystemSandboxPolicy;
    use codex_protocol::permissions::FileSystemSpecialPath;
    use codex_protocol::permissions::NetworkSandboxPolicy;
    use futures::SinkExt;
    use futures::StreamExt;
    use pretty_assertions::assert_eq;
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio::net::TcpStream;
    use tokio::time::timeout;
    use tokio_tungstenite::WebSocketStream;
    use tokio_tungstenite::accept_async;
    use tokio_tungstenite::tungstenite::Message;

    use super::*;
    use crate::client_api::ExecServerTransportParams;
    use crate::protocol::FsGlobResponse;
    use crate::protocol::FsGrepResponse;
    use crate::protocol::INITIALIZE_METHOD;
    use crate::protocol::INITIALIZED_METHOD;
    use crate::protocol::InitializeResponse;

    #[test]
    fn remote_sandbox_context_drops_unused_cwd() {
        let policy = FileSystemSandboxPolicy::restricted(vec![FileSystemSandboxEntry {
            path: FileSystemPath::Path {
                path: absolute_test_path("remote-root"),
            },
            access: FileSystemAccessMode::Read,
        }]);
        let permissions =
            PermissionProfile::from_runtime_permissions(&policy, NetworkSandboxPolicy::Restricted);
        let sandbox_context = FileSystemSandboxContext::from_permission_profile_with_cwd(
            permissions,
            absolute_test_path("host-checkout"),
        );

        let remote_context =
            remote_sandbox_context(Some(&sandbox_context)).expect("remote sandbox context");

        assert_eq!(remote_context.cwd, None);
    }

    #[test]
    fn remote_sandbox_context_preserves_required_cwd() {
        let policy = FileSystemSandboxPolicy::restricted(vec![FileSystemSandboxEntry {
            path: FileSystemPath::Special {
                value: FileSystemSpecialPath::project_roots(/*subpath*/ None),
            },
            access: FileSystemAccessMode::Write,
        }]);
        let permissions =
            PermissionProfile::from_runtime_permissions(&policy, NetworkSandboxPolicy::Restricted);
        let cwd = absolute_test_path("host-checkout");
        let sandbox_context =
            FileSystemSandboxContext::from_permission_profile_with_cwd(permissions, cwd.clone());

        let remote_context =
            remote_sandbox_context(Some(&sandbox_context)).expect("remote sandbox context");

        assert_eq!(remote_context.cwd, Some(cwd));
    }

    #[test]
    fn transport_errors_map_to_broken_pipe() {
        let errors = [
            ExecServerError::Closed,
            ExecServerError::Disconnected("exec-server transport disconnected".to_string()),
        ];

        let mapped_errors = errors
            .into_iter()
            .map(|error| {
                let error = map_remote_error(error);
                (error.kind(), error.to_string())
            })
            .collect::<Vec<_>>();

        assert_eq!(
            mapped_errors,
            vec![
                (
                    io::ErrorKind::BrokenPipe,
                    "exec-server transport closed".to_string()
                ),
                (
                    io::ErrorKind::BrokenPipe,
                    "exec-server transport closed".to_string()
                ),
            ]
        );
    }

    #[tokio::test]
    async fn remote_searches_are_single_rpc_calls() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let websocket_url = format!(
            "ws://{}",
            listener.local_addr().expect("listener should have address")
        );
        let server = tokio::spawn(async move {
            let mut websocket = accept_websocket(&listener).await;
            complete_initialize(&mut websocket).await;

            let glob = read_jsonrpc_websocket(&mut websocket).await;
            let glob_request = request_for_method(glob, crate::protocol::FS_GLOB_METHOD);
            write_jsonrpc_websocket(
                &mut websocket,
                JSONRPCMessage::Response(JSONRPCResponse {
                    id: glob_request.id,
                    result: serde_json::to_value(FsGlobResponse {
                        response: GlobSearchResponse {
                            matches: Vec::new(),
                            truncated: false,
                        },
                    })
                    .expect("glob response should serialize"),
                }),
            )
            .await;

            let grep = read_jsonrpc_websocket(&mut websocket).await;
            let grep_request = request_for_method(grep, crate::protocol::FS_GREP_METHOD);
            write_jsonrpc_websocket(
                &mut websocket,
                JSONRPCMessage::Response(JSONRPCResponse {
                    id: grep_request.id,
                    result: serde_json::to_value(FsGrepResponse {
                        response: GrepSearchResponse {
                            lines: vec!["src/main.rs".to_string()],
                            num_files: 1,
                            num_matches: None,
                            applied_limit: None,
                            applied_offset: None,
                            truncated: false,
                        },
                    })
                    .expect("grep response should serialize"),
                }),
            )
            .await;
        });

        let file_system = RemoteFileSystem::new(LazyRemoteExecServerClient::new(
            ExecServerTransportParams::WebSocketUrl {
                websocket_url,
                connect_timeout: Duration::from_secs(1),
                initialize_timeout: Duration::from_secs(1),
            },
        ));
        let root = absolute_test_path("remote-search-root");

        let glob_response = file_system
            .glob_search(
                GlobSearchRequest {
                    root: root.clone(),
                    pattern: "**/*.rs".to_string(),
                    max_results: 10,
                },
                /*sandbox*/ None,
            )
            .await
            .expect("remote glob should succeed");
        assert_eq!(
            glob_response,
            GlobSearchResponse {
                matches: Vec::new(),
                truncated: false,
            }
        );

        let grep_response = file_system
            .grep_search(
                GrepSearchRequest {
                    root,
                    pattern: "needle".to_string(),
                    glob: None,
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
                /*sandbox*/ None,
            )
            .await
            .expect("remote grep should succeed");
        assert_eq!(
            grep_response,
            GrepSearchResponse {
                lines: vec!["src/main.rs".to_string()],
                num_files: 1,
                num_matches: None,
                applied_limit: None,
                applied_offset: None,
                truncated: false,
            }
        );

        drop(file_system);
        server.await.expect("server task should finish");
    }

    fn absolute_test_path(name: &str) -> AbsolutePathBuf {
        let path = std::env::temp_dir().join(name);
        AbsolutePathBuf::from_absolute_path(&path).expect("absolute path")
    }

    async fn accept_websocket(listener: &TcpListener) -> WebSocketStream<TcpStream> {
        let (stream, _) = listener.accept().await.expect("listener should accept");
        accept_async(stream)
            .await
            .expect("websocket handshake should succeed")
    }

    async fn complete_initialize(websocket: &mut WebSocketStream<TcpStream>) {
        let initialize = read_jsonrpc_websocket(websocket).await;
        let request = request_for_method(initialize, INITIALIZE_METHOD);
        write_jsonrpc_websocket(
            websocket,
            JSONRPCMessage::Response(JSONRPCResponse {
                id: request.id,
                result: serde_json::to_value(InitializeResponse {
                    session_id: "session-1".to_string(),
                })
                .expect("initialize response should serialize"),
            }),
        )
        .await;

        let initialized = read_jsonrpc_websocket(websocket).await;
        match initialized {
            JSONRPCMessage::Notification(JSONRPCNotification { method, .. })
                if method == INITIALIZED_METHOD => {}
            other => panic!("expected initialized notification, got {other:?}"),
        }
    }

    fn request_for_method(message: JSONRPCMessage, method: &str) -> JSONRPCRequest {
        match message {
            JSONRPCMessage::Request(request) if request.method == method => request,
            other => panic!("expected {method} request, got {other:?}"),
        }
    }

    async fn read_jsonrpc_websocket(websocket: &mut WebSocketStream<TcpStream>) -> JSONRPCMessage {
        loop {
            match timeout(Duration::from_secs(1), websocket.next())
                .await
                .expect("json-rpc websocket read should not time out")
                .expect("websocket should stay open")
                .expect("websocket frame should read")
            {
                Message::Text(text) => {
                    return serde_json::from_str(text.as_ref())
                        .expect("json-rpc text frame should parse");
                }
                Message::Binary(bytes) => {
                    return serde_json::from_slice(bytes.as_ref())
                        .expect("json-rpc binary frame should parse");
                }
                Message::Ping(_) | Message::Pong(_) => {}
                other => panic!("expected json-rpc websocket frame, got {other:?}"),
            }
        }
    }

    async fn write_jsonrpc_websocket(
        websocket: &mut WebSocketStream<TcpStream>,
        message: JSONRPCMessage,
    ) {
        let encoded = serde_json::to_string(&message).expect("json-rpc should serialize");
        websocket
            .send(Message::Text(encoded.into()))
            .await
            .expect("json-rpc websocket frame should write");
    }
}
