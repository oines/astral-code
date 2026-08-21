use super::*;
use codex_app_server_protocol::AccountLoginCompletedNotification;
use codex_app_server_protocol::AccountRateLimitsUpdatedNotification;
use codex_app_server_protocol::AccountUpdatedNotification;
use codex_app_server_protocol::CancelLoginAccountParams;
use codex_app_server_protocol::CancelLoginAccountResponse;
use codex_app_server_protocol::CancelLoginAccountStatus;
use codex_app_server_protocol::GetAccountRateLimitsResponse;
use codex_app_server_protocol::LoginAccountParams;
use codex_app_server_protocol::LoginAccountResponse;
use codex_app_server_protocol::LogoutAccountResponse;
use codex_backend_client::Client as BackendClient;
use codex_login::DeviceCode;
use codex_login::ServerOptions;
use codex_login::ShutdownHandle;
use codex_login::complete_device_code_login;
use codex_login::request_device_code;
use codex_login::run_login_server;
use codex_model_provider::CHATGPT_CODEX_BASE_URL;
use codex_model_provider::CODEX_PROVIDER_ID;
use codex_models_manager::manager::RefreshStrategy;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

enum ActiveLogin {
    Browser {
        shutdown: ShutdownHandle,
        login_id: Uuid,
    },
    Device {
        cancel: CancellationToken,
        login_id: Uuid,
    },
}

impl ActiveLogin {
    fn login_id(&self) -> Uuid {
        match self {
            Self::Browser { login_id, .. } | Self::Device { login_id, .. } => *login_id,
        }
    }

    fn cancel(&self) {
        match self {
            Self::Browser { shutdown, .. } => shutdown.shutdown(),
            Self::Device { cancel, .. } => cancel.cancel(),
        }
    }
}

impl Drop for ActiveLogin {
    fn drop(&mut self) {
        self.cancel();
    }
}

enum RefreshTokenRequestOutcome {
    NotAttemptedOrSucceeded,
    FailedTransiently,
    FailedPermanently,
}

#[derive(Clone)]
pub(crate) struct AccountRequestProcessor {
    auth_manager: Arc<AuthManager>,
    outgoing: Arc<OutgoingMessageSender>,
    thread_manager: Arc<ThreadManager>,
    config: Arc<Config>,
    active_login: Arc<Mutex<Option<ActiveLogin>>>,
}

impl AccountRequestProcessor {
    pub(crate) fn new(
        auth_manager: Arc<AuthManager>,
        outgoing: Arc<OutgoingMessageSender>,
        thread_manager: Arc<ThreadManager>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            auth_manager,
            outgoing,
            thread_manager,
            config,
            active_login: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) async fn login_account(
        &self,
        params: LoginAccountParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let response = match params {
            LoginAccountParams::Chatgpt {
                codex_streamlined_login,
            } => self.start_browser_login(codex_streamlined_login).await?,
            LoginAccountParams::ChatgptDeviceCode => self.start_device_login().await?,
        };
        Ok(Some(response.into()))
    }

    pub(crate) async fn cancel_login_account(
        &self,
        params: CancelLoginAccountParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let login_id = Uuid::parse_str(&params.login_id)
            .map_err(|_| invalid_request(format!("invalid login id: {}", params.login_id)))?;
        let mut active = self.active_login.lock().await;
        let status = if active.as_ref().map(ActiveLogin::login_id) == Some(login_id) {
            active.take();
            CancelLoginAccountStatus::Canceled
        } else {
            CancelLoginAccountStatus::NotFound
        };
        Ok(Some(CancelLoginAccountResponse { status }.into()))
    }

    pub(crate) async fn logout_account(
        &self,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.cancel_active_login().await;
        self.auth_manager
            .logout_codex_oauth()
            .await
            .map_err(|err| internal_error(format!("failed to log out of Codex: {err}")))?;
        self.thread_manager
            .invalidate_model_provider(CODEX_PROVIDER_ID);
        self.outgoing
            .send_server_notification(ServerNotification::AccountUpdated(
                AccountUpdatedNotification {
                    auth_mode: None,
                    plan_type: None,
                },
            ))
            .await;
        Ok(Some(LogoutAccountResponse {}.into()))
    }

    pub(crate) async fn get_account_rate_limits(
        &self,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let auth = self
            .auth_manager
            .codex_oauth_auth()
            .await
            .ok_or_else(|| invalid_request("Codex login is required to read rate limits"))?;
        let client = BackendClient::from_auth(CHATGPT_CODEX_BASE_URL, &auth)
            .map_err(|err| internal_error(format!("failed to create usage client: {err}")))?;
        let snapshots = client
            .get_rate_limits_many()
            .await
            .map_err(|err| internal_error(format!("failed to fetch Codex rate limits: {err}")))?;
        let preferred = snapshots
            .iter()
            .find(|snapshot| snapshot.limit_id.as_deref() == Some("codex"))
            .or_else(|| snapshots.first())
            .cloned()
            .ok_or_else(|| internal_error("Codex rate limit response was empty"))?;
        let rate_limits_by_limit_id = snapshots
            .into_iter()
            .map(|snapshot| {
                let id = snapshot
                    .limit_id
                    .clone()
                    .unwrap_or_else(|| "codex".to_string());
                (id, snapshot.into())
            })
            .collect();
        let response = GetAccountRateLimitsResponse {
            rate_limits: preferred.into(),
            rate_limits_by_limit_id: Some(rate_limits_by_limit_id),
        };
        self.outgoing
            .send_server_notification(ServerNotification::AccountRateLimitsUpdated(
                AccountRateLimitsUpdatedNotification {
                    rate_limits: response.rate_limits.clone(),
                    rate_limits_by_limit_id: response.rate_limits_by_limit_id.clone(),
                },
            ))
            .await;
        Ok(Some(response.into()))
    }

    pub(crate) async fn get_account(
        &self,
        params: GetAccountParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.get_account_response(params)
            .await
            .map(|response| Some(response.into()))
    }

    pub(crate) async fn get_auth_status(
        &self,
        params: GetAuthStatusParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.get_auth_status_response(params)
            .await
            .map(|response| Some(response.into()))
    }

    pub(crate) async fn cancel_active_login(&self) {
        self.active_login.lock().await.take();
    }

    pub(crate) fn clear_external_auth(&self) {
        self.auth_manager.clear_external_auth();
    }

    fn login_options(&self) -> ServerOptions {
        let mut options = ServerOptions::new(
            self.config.codex_home.to_path_buf(),
            codex_login::CLIENT_ID.to_string(),
            /*forced_chatgpt_workspace_id*/ None,
            self.config.cli_auth_credentials_store_mode,
        );
        options.open_browser = false;
        options
    }

    async fn start_browser_login(
        &self,
        codex_streamlined_login: bool,
    ) -> Result<LoginAccountResponse, JSONRPCErrorError> {
        let mut options = self.login_options();
        options.codex_streamlined_login = codex_streamlined_login;
        let server = run_login_server(options)
            .map_err(|err| internal_error(format!("failed to start Codex login server: {err}")))?;
        let login_id = Uuid::new_v4();
        let shutdown = server.cancel_handle();
        self.replace_active_login(ActiveLogin::Browser {
            shutdown: shutdown.clone(),
            login_id,
        })
        .await;
        let auth_url = server.auth_url.clone();
        let processor = self.clone();
        tokio::spawn(async move {
            let result = match tokio::time::timeout(LOGIN_TIMEOUT, server.block_until_done()).await
            {
                Ok(result) => result,
                Err(_) => {
                    shutdown.shutdown();
                    Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Codex login timed out",
                    ))
                }
            };
            processor.finish_login(login_id, result).await;
        });
        Ok(LoginAccountResponse::Chatgpt {
            login_id: login_id.to_string(),
            auth_url,
        })
    }

    async fn start_device_login(&self) -> Result<LoginAccountResponse, JSONRPCErrorError> {
        let options = self.login_options();
        let device_code = request_device_code(&options)
            .await
            .map_err(|err| invalid_request(format!("failed to request device code: {err}")))?;
        let login_id = Uuid::new_v4();
        let cancel = CancellationToken::new();
        self.replace_active_login(ActiveLogin::Device {
            cancel: cancel.clone(),
            login_id,
        })
        .await;
        let response = LoginAccountResponse::ChatgptDeviceCode {
            login_id: login_id.to_string(),
            verification_url: device_code.verification_url.clone(),
            user_code: device_code.user_code.clone(),
        };
        let processor = self.clone();
        tokio::spawn(async move {
            let result = tokio::select! {
                _ = cancel.cancelled() => Err(std::io::Error::other("Codex login canceled")),
                result = complete_device_login(options, device_code) => result,
            };
            processor.finish_login(login_id, result).await;
        });
        Ok(response)
    }

    async fn replace_active_login(&self, login: ActiveLogin) {
        let mut active = self.active_login.lock().await;
        active.take();
        *active = Some(login);
    }

    async fn finish_login(&self, login_id: Uuid, result: std::io::Result<()>) {
        let success = result.is_ok();
        if success {
            self.auth_manager.reload_codex_oauth().await;
            self.thread_manager
                .invalidate_model_provider(CODEX_PROVIDER_ID);
            if let Some(provider) = self.config.model_providers.get(CODEX_PROVIDER_ID) {
                let mut codex_config = self.config.as_ref().clone();
                codex_config.model_provider_id = CODEX_PROVIDER_ID.to_string();
                codex_config.model_provider = provider.clone();
                codex_config.model_catalog = None;
                let manager = self.thread_manager.models_manager_for_config(&codex_config);
                let _ = manager.list_models(RefreshStrategy::Online).await;
            }
        }
        self.outgoing
            .send_server_notification(ServerNotification::AccountLoginCompleted(
                AccountLoginCompletedNotification {
                    login_id: Some(login_id.to_string()),
                    success,
                    error: result.err().map(|err| err.to_string()),
                },
            ))
            .await;
        if success {
            let auth = self.auth_manager.codex_oauth_auth_cached();
            self.outgoing
                .send_server_notification(ServerNotification::AccountUpdated(
                    AccountUpdatedNotification {
                        auth_mode: Some(AuthMode::Chatgpt),
                        plan_type: auth.as_ref().and_then(CodexAuth::account_plan_type),
                    },
                ))
                .await;
        }
        let mut active = self.active_login.lock().await;
        if active.as_ref().map(ActiveLogin::login_id) == Some(login_id) {
            *active = None;
        }
    }

    async fn refresh_token_if_requested(&self, do_refresh: bool) -> RefreshTokenRequestOutcome {
        if !do_refresh {
            return RefreshTokenRequestOutcome::NotAttemptedOrSucceeded;
        }
        let refresh = if self.auth_manager.codex_oauth_auth_cached().is_some() {
            self.auth_manager.refresh_codex_oauth_from_authority().await
        } else {
            self.auth_manager.refresh_token().await
        };
        if let Err(err) = refresh {
            if err.failed_reason().is_some() {
                return RefreshTokenRequestOutcome::FailedPermanently;
            }
            tracing::warn!("failed to refresh token while getting account: {err}");
            return RefreshTokenRequestOutcome::FailedTransiently;
        }
        RefreshTokenRequestOutcome::NotAttemptedOrSucceeded
    }

    async fn get_auth_status_response(
        &self,
        params: GetAuthStatusParams,
    ) -> Result<GetAuthStatusResponse, JSONRPCErrorError> {
        let include_token = params.include_token.unwrap_or(false);
        let do_refresh = params.refresh_token.unwrap_or(false);
        self.refresh_token_if_requested(do_refresh).await;
        let requires_astral_auth = self.config.model_provider.requires_astral_auth;
        if let Ok(Some(api_key)) = self.config.model_provider.api_key() {
            return Ok(GetAuthStatusResponse {
                auth_method: Some(AuthMode::ApiKey),
                auth_token: include_token.then_some(api_key),
                requires_astral_auth: Some(requires_astral_auth),
            });
        }
        let auth = self.auth_manager.auth().await;
        Ok(match auth {
            Some(auth) => GetAuthStatusResponse {
                auth_method: Some(auth.api_auth_mode()),
                auth_token: include_token.then(|| auth.get_token().ok()).flatten(),
                requires_astral_auth: Some(requires_astral_auth),
            },
            None => GetAuthStatusResponse {
                auth_method: None,
                auth_token: None,
                requires_astral_auth: Some(requires_astral_auth),
            },
        })
    }

    async fn get_account_response(
        &self,
        params: GetAccountParams,
    ) -> Result<GetAccountResponse, JSONRPCErrorError> {
        self.refresh_token_if_requested(params.refresh_token).await;
        let provider = create_model_provider(
            self.config.model_provider.clone(),
            Some(self.auth_manager.clone()),
        );
        let account_state = provider.account_state();
        let codex_account = self.auth_manager.codex_oauth_auth_cached().map(|auth| {
            Account::from(codex_protocol::account::ProviderAccount::Chatgpt {
                email: auth.account_email(),
                plan_type: auth.account_plan_type().unwrap_or_default(),
            })
        });
        Ok(GetAccountResponse {
            account: codex_account.or_else(|| account_state.account.map(Account::from)),
            requires_astral_auth: account_state.requires_astral_auth,
            requires_openai_auth: account_state.requires_openai_auth,
        })
    }
}

async fn complete_device_login(
    options: ServerOptions,
    device_code: DeviceCode,
) -> std::io::Result<()> {
    complete_device_code_login(options, device_code).await
}
