use super::*;

enum RefreshTokenRequestOutcome {
    NotAttemptedOrSucceeded,
    FailedTransiently,
    FailedPermanently,
}

#[derive(Clone)]
pub(crate) struct AccountRequestProcessor {
    auth_manager: Arc<AuthManager>,
    config: Arc<Config>,
}

impl AccountRequestProcessor {
    pub(crate) fn new(auth_manager: Arc<AuthManager>, config: Arc<Config>) -> Self {
        Self {
            auth_manager,
            config,
        }
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

    pub(crate) async fn cancel_active_login(&self) {}

    pub(crate) fn clear_external_auth(&self) {
        self.auth_manager.clear_external_auth();
    }

    async fn refresh_token_if_requested(&self, do_refresh: bool) -> RefreshTokenRequestOutcome {
        if do_refresh && let Err(err) = self.auth_manager.refresh_token().await {
            let failed_reason = err.failed_reason();
            if failed_reason.is_none() {
                tracing::warn!("failed to refresh token while getting account: {err}");
                return RefreshTokenRequestOutcome::FailedTransiently;
            }
            return RefreshTokenRequestOutcome::FailedPermanently;
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

        let auth = if do_refresh {
            self.auth_manager.auth_cached()
        } else {
            self.auth_manager.auth().await
        };
        let response = match auth {
            Some(auth) => {
                let permanent_refresh_failure =
                    self.auth_manager.refresh_failure_for_auth(&auth).is_some();
                let auth_mode = auth.api_auth_mode();
                let (reported_auth_method, token_opt) =
                    if include_token && permanent_refresh_failure {
                        (Some(auth_mode), None)
                    } else {
                        match auth.get_token() {
                            Ok(token) if !token.is_empty() => {
                                let tok = if include_token { Some(token) } else { None };
                                (Some(auth_mode), tok)
                            }
                            Ok(_) => (None, None),
                            Err(err) => {
                                tracing::warn!("failed to get token for auth status: {err}");
                                (None, None)
                            }
                        }
                    };
                GetAuthStatusResponse {
                    auth_method: reported_auth_method,
                    auth_token: token_opt,
                    requires_astral_auth: Some(requires_astral_auth),
                }
            }
            None => GetAuthStatusResponse {
                auth_method: None,
                auth_token: None,
                requires_astral_auth: Some(requires_astral_auth),
            },
        };

        Ok(response)
    }

    async fn get_account_response(
        &self,
        params: GetAccountParams,
    ) -> Result<GetAccountResponse, JSONRPCErrorError> {
        let do_refresh = params.refresh_token;

        self.refresh_token_if_requested(do_refresh).await;

        let provider = create_model_provider(
            self.config.model_provider.clone(),
            Some(self.auth_manager.clone()),
        );
        let account_state = provider.account_state();
        let account = account_state.account.map(Account::from);

        Ok(GetAccountResponse {
            account,
            requires_astral_auth: account_state.requires_astral_auth,
        })
    }
}
