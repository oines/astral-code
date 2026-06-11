use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use crate::transport::RemoteControlHandle;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::RemoteControlClientsListParams;
use codex_app_server_protocol::RemoteControlClientsListResponse;
use codex_app_server_protocol::RemoteControlClientsRevokeParams;
use codex_app_server_protocol::RemoteControlClientsRevokeResponse;
use codex_app_server_protocol::RemoteControlDisableResponse;
use codex_app_server_protocol::RemoteControlEnableResponse;
use codex_app_server_protocol::RemoteControlPairingStartParams;
use codex_app_server_protocol::RemoteControlPairingStartResponse;
use codex_app_server_protocol::RemoteControlPairingStatusParams;
use codex_app_server_protocol::RemoteControlPairingStatusResponse;
use codex_app_server_protocol::RemoteControlStatusReadResponse;

const LEGACY_REMOTE_CONTROL_DISABLED_MESSAGE: &str = "legacy hosted remote control is disabled in Astral until a provider-neutral control plane exists";

#[derive(Clone)]
pub(crate) struct RemoteControlRequestProcessor {
    remote_control_handle: Option<RemoteControlHandle>,
}

impl RemoteControlRequestProcessor {
    pub(crate) fn new(remote_control_handle: Option<RemoteControlHandle>) -> Self {
        Self {
            remote_control_handle,
        }
    }

    pub(crate) fn enable(&self) -> Result<RemoteControlEnableResponse, JSONRPCErrorError> {
        Err(legacy_remote_control_disabled())
    }

    pub(crate) fn disable(&self) -> Result<RemoteControlDisableResponse, JSONRPCErrorError> {
        let handle = self.handle()?;
        Ok(RemoteControlDisableResponse::from(handle.disable()))
    }

    pub(crate) fn status_read(&self) -> Result<RemoteControlStatusReadResponse, JSONRPCErrorError> {
        let status = self.handle()?.status();
        Ok(RemoteControlStatusReadResponse {
            status: status.status,
            server_name: status.server_name,
            installation_id: status.installation_id,
            environment_id: status.environment_id,
        })
    }

    pub(crate) async fn pairing_start(
        &self,
        _params: RemoteControlPairingStartParams,
        _app_server_client_name: Option<&str>,
    ) -> Result<RemoteControlPairingStartResponse, JSONRPCErrorError> {
        Err(legacy_remote_control_disabled())
    }

    pub(crate) async fn pairing_status(
        &self,
        _params: RemoteControlPairingStatusParams,
    ) -> Result<RemoteControlPairingStatusResponse, JSONRPCErrorError> {
        Err(legacy_remote_control_disabled())
    }

    pub(crate) async fn clients_list(
        &self,
        _params: RemoteControlClientsListParams,
    ) -> Result<RemoteControlClientsListResponse, JSONRPCErrorError> {
        Err(legacy_remote_control_disabled())
    }

    pub(crate) async fn clients_revoke(
        &self,
        _params: RemoteControlClientsRevokeParams,
    ) -> Result<RemoteControlClientsRevokeResponse, JSONRPCErrorError> {
        Err(legacy_remote_control_disabled())
    }

    fn handle(&self) -> Result<&RemoteControlHandle, JSONRPCErrorError> {
        self.remote_control_handle
            .as_ref()
            .ok_or_else(|| internal_error("remote control is unavailable for this app-server"))
    }
}

fn legacy_remote_control_disabled() -> JSONRPCErrorError {
    invalid_request(LEGACY_REMOTE_CONTROL_DISABLED_MESSAGE)
}

#[cfg(test)]
mod remote_control_processor_tests;
