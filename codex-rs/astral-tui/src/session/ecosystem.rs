//! Typed app-server requests for Astral ecosystem inventory.

use codex_app_server_protocol::AppsListParams;
use codex_app_server_protocol::AppsListResponse;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::HooksListParams;
use codex_app_server_protocol::HooksListResponse;
use codex_app_server_protocol::ListMcpServerStatusParams;
use codex_app_server_protocol::ListMcpServerStatusResponse;
use codex_app_server_protocol::McpServerStatusDetail;
use codex_app_server_protocol::PluginListParams;
use codex_app_server_protocol::PluginListResponse;
use codex_app_server_protocol::SkillsListParams;
use codex_app_server_protocol::SkillsListResponse;

use super::AstralSession;
use super::SessionError;

impl AstralSession {
    pub(crate) async fn list_mcp_servers(
        &mut self,
        detail: McpServerStatusDetail,
    ) -> Result<ListMcpServerStatusResponse, SessionError> {
        let thread_id = self
            .state
            .as_ref()
            .map(|state| state.thread.id.clone())
            .ok_or(SessionError::NoThread)?;
        let request_id = self.next_request_id();
        let response = self
            .client
            .request_typed(ClientRequest::McpServerStatusList {
                request_id,
                params: ListMcpServerStatusParams {
                    cursor: None,
                    limit: Some(100),
                    detail: Some(detail),
                    thread_id: Some(thread_id),
                },
            })
            .await?;
        Ok(response)
    }

    pub(crate) async fn list_skills(&mut self) -> Result<SkillsListResponse, SessionError> {
        let cwd = self
            .state
            .as_ref()
            .map(|state| state.thread.cwd.to_path_buf())
            .ok_or(SessionError::NoThread)?;
        let request_id = self.next_request_id();
        let response = self
            .client
            .request_typed(ClientRequest::SkillsList {
                request_id,
                params: SkillsListParams {
                    cwds: vec![cwd],
                    force_reload: false,
                },
            })
            .await?;
        Ok(response)
    }

    pub(crate) async fn list_hooks(&mut self) -> Result<HooksListResponse, SessionError> {
        let cwd = self
            .state
            .as_ref()
            .map(|state| state.thread.cwd.to_path_buf())
            .ok_or(SessionError::NoThread)?;
        let request_id = self.next_request_id();
        let response = self
            .client
            .request_typed(ClientRequest::HooksList {
                request_id,
                params: HooksListParams { cwds: vec![cwd] },
            })
            .await?;
        Ok(response)
    }

    pub(crate) async fn list_apps(&mut self) -> Result<AppsListResponse, SessionError> {
        let thread_id = self
            .state
            .as_ref()
            .map(|state| state.thread.id.clone())
            .ok_or(SessionError::NoThread)?;
        let request_id = self.next_request_id();
        let response = self
            .client
            .request_typed(ClientRequest::AppsList {
                request_id,
                params: AppsListParams {
                    cursor: None,
                    limit: Some(100),
                    thread_id: Some(thread_id),
                    force_refetch: false,
                },
            })
            .await?;
        Ok(response)
    }

    pub(crate) async fn list_plugins(&mut self) -> Result<PluginListResponse, SessionError> {
        let cwd = self
            .state
            .as_ref()
            .map(|state| state.thread.cwd.clone())
            .ok_or(SessionError::NoThread)?;
        let request_id = self.next_request_id();
        let response = self
            .client
            .request_typed(ClientRequest::PluginList {
                request_id,
                params: PluginListParams {
                    cwds: Some(vec![cwd]),
                    marketplace_kinds: None,
                },
            })
            .await?;
        Ok(response)
    }
}
