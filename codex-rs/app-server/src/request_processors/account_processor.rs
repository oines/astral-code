use super::*;

const ACCOUNT_TOKEN_USAGE_FETCH_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 10);

enum RefreshTokenRequestOutcome {
    NotAttemptedOrSucceeded,
    FailedTransiently,
    FailedPermanently,
}

#[derive(Clone)]
pub(crate) struct AccountRequestProcessor {
    auth_manager: Arc<AuthManager>,
    thread_manager: Arc<ThreadManager>,
    outgoing: Arc<OutgoingMessageSender>,
    config: Arc<Config>,
    config_manager: ConfigManager,
}

const CHATGPT_LOGIN_DISABLED_MESSAGE: &str =
    "ChatGPT login is disabled in astral-code. Use API key login instead.";

impl AccountRequestProcessor {
    pub(crate) fn new(
        auth_manager: Arc<AuthManager>,
        thread_manager: Arc<ThreadManager>,
        outgoing: Arc<OutgoingMessageSender>,
        config: Arc<Config>,
        config_manager: ConfigManager,
    ) -> Self {
        Self {
            auth_manager,
            thread_manager,
            outgoing,
            config,
            config_manager,
        }
    }

    pub(crate) async fn login_account(
        &self,
        request_id: ConnectionRequestId,
        params: LoginAccountParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.login_v2(request_id, params).await.map(|()| None)
    }

    pub(crate) async fn logout_account(
        &self,
        request_id: ConnectionRequestId,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.logout_v2(request_id).await.map(|()| None)
    }

    pub(crate) async fn cancel_login_account(
        &self,
        params: CancelLoginAccountParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.cancel_login_response(params)
            .await
            .map(|response| Some(response.into()))
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

    pub(crate) async fn get_account_rate_limits(
        &self,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.get_account_rate_limits_response()
            .await
            .map(|response| Some(response.into()))
    }

    pub(crate) async fn get_account_token_usage(
        &self,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.get_account_token_usage_response()
            .await
            .map(|response| Some(response.into()))
    }

    pub(crate) async fn send_add_credits_nudge_email(
        &self,
        params: SendAddCreditsNudgeEmailParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.send_add_credits_nudge_email_response(params)
            .await
            .map(|response| Some(response.into()))
    }

    pub(crate) async fn cancel_active_login(&self) {}

    pub(crate) fn clear_external_auth(&self) {
        self.auth_manager.clear_external_auth();
    }

    fn current_account_updated_notification(&self) -> AccountUpdatedNotification {
        let auth = self.auth_manager.auth_cached();
        AccountUpdatedNotification {
            auth_mode: auth.as_ref().map(CodexAuth::api_auth_mode),
            plan_type: auth.as_ref().and_then(CodexAuth::account_plan_type),
        }
    }

    async fn maybe_refresh_remote_installed_plugins_cache_for_current_config(
        config_manager: &ConfigManager,
        thread_manager: &Arc<ThreadManager>,
        auth: Option<CodexAuth>,
    ) {
        match config_manager
            .load_latest_config(/*fallback_cwd*/ None)
            .await
        {
            Ok(config) => {
                let refresh_thread_manager = Arc::clone(thread_manager);
                let refresh_config_manager = config_manager.clone();
                thread_manager
                    .plugins_manager()
                    .maybe_start_remote_installed_plugins_cache_refresh(
                        &config.plugins_config_input(),
                        auth,
                        Some(Arc::new(move || {
                            Self::spawn_effective_plugins_changed_task(
                                Arc::clone(&refresh_thread_manager),
                                refresh_config_manager.clone(),
                            );
                        })),
                    );
            }
            Err(err) => {
                warn!(
                    "failed to reload config after account changed, skipping remote installed plugins cache refresh: {err}"
                );
            }
        }
    }

    fn spawn_effective_plugins_changed_task(
        thread_manager: Arc<ThreadManager>,
        config_manager: ConfigManager,
    ) {
        tokio::spawn(async move {
            thread_manager.plugins_manager().clear_cache();
            thread_manager.skills_manager().clear_cache();
            if thread_manager.list_thread_ids().await.is_empty() {
                return;
            }
            crate::mcp_refresh::queue_best_effort_refresh(&thread_manager, &config_manager).await;
        });
    }

    async fn login_v2(
        &self,
        request_id: ConnectionRequestId,
        params: LoginAccountParams,
    ) -> Result<(), JSONRPCErrorError> {
        match params {
            LoginAccountParams::ApiKey { api_key } => {
                self.login_api_key_v2(request_id, LoginApiKeyParams { api_key })
                    .await;
            }
            LoginAccountParams::Chatgpt { .. } | LoginAccountParams::ChatgptDeviceCode => {
                self.reject_chatgpt_login_v2(request_id).await;
            }
            LoginAccountParams::ChatgptAuthTokens {
                access_token,
                chatgpt_account_id,
                chatgpt_plan_type,
            } => {
                self.login_chatgpt_auth_tokens(
                    request_id,
                    access_token,
                    chatgpt_account_id,
                    chatgpt_plan_type,
                )
                .await;
            }
        }
        Ok(())
    }

    async fn reject_chatgpt_login_v2(&self, request_id: ConnectionRequestId) {
        let result: Result<LoginAccountResponse, JSONRPCErrorError> =
            Err(invalid_request(CHATGPT_LOGIN_DISABLED_MESSAGE));
        self.outgoing.send_result(request_id, result).await;
    }

    fn external_auth_active_error(&self) -> JSONRPCErrorError {
        invalid_request(
            "External auth is active. Use account/login/start (chatgptAuthTokens) to update it or account/logout to clear it.",
        )
    }

    async fn login_api_key_common(
        &self,
        params: &LoginApiKeyParams,
    ) -> std::result::Result<(), JSONRPCErrorError> {
        if self.auth_manager.is_external_chatgpt_auth_active() {
            return Err(self.external_auth_active_error());
        }

        if matches!(
            self.config.forced_login_method,
            Some(ForcedLoginMethod::Chatgpt)
        ) {
            return Err(invalid_request(
                "API key login is disabled. Use ChatGPT login instead.",
            ));
        }

        match login_with_api_key(
            &self.config.codex_home,
            &params.api_key,
            self.config.cli_auth_credentials_store_mode,
        ) {
            Ok(()) => {
                self.auth_manager.reload().await;
                Ok(())
            }
            Err(err) => Err(internal_error(format!("failed to save api key: {err}"))),
        }
    }

    async fn login_api_key_v2(&self, request_id: ConnectionRequestId, params: LoginApiKeyParams) {
        let result = self
            .login_api_key_common(&params)
            .await
            .map(|()| LoginAccountResponse::ApiKey {});
        let logged_in = result.is_ok();
        self.outgoing.send_result(request_id, result).await;

        if logged_in {
            self.send_login_success_notifications(/*login_id*/ None)
                .await;
        }
    }

    async fn cancel_login_response(
        &self,
        params: CancelLoginAccountParams,
    ) -> Result<CancelLoginAccountResponse, JSONRPCErrorError> {
        let login_id = params.login_id;
        let _uuid = Uuid::parse_str(&login_id)
            .map_err(|_| invalid_request(format!("invalid login id: {login_id}")))?;
        Ok(CancelLoginAccountResponse {
            status: CancelLoginAccountStatus::NotFound,
        })
    }

    async fn login_chatgpt_auth_tokens(
        &self,
        request_id: ConnectionRequestId,
        access_token: String,
        chatgpt_account_id: String,
        chatgpt_plan_type: Option<String>,
    ) {
        let result = self
            .login_chatgpt_auth_tokens_response(access_token, chatgpt_account_id, chatgpt_plan_type)
            .await;
        let logged_in = result.is_ok();
        self.outgoing.send_result(request_id, result).await;

        if logged_in {
            self.send_login_success_notifications(/*login_id*/ None)
                .await;
        }
    }

    async fn login_chatgpt_auth_tokens_response(
        &self,
        access_token: String,
        chatgpt_account_id: String,
        chatgpt_plan_type: Option<String>,
    ) -> Result<LoginAccountResponse, JSONRPCErrorError> {
        if matches!(
            self.config.forced_login_method,
            Some(ForcedLoginMethod::Api)
        ) {
            return Err(invalid_request(
                "External ChatGPT auth is disabled. Use API key login instead.",
            ));
        }

        if let Some(expected_workspaces) = self.config.forced_chatgpt_workspace_id.as_deref()
            && !expected_workspaces.contains(&chatgpt_account_id)
        {
            return Err(invalid_request(format!(
                "External auth must use one of workspace(s) {expected_workspaces:?}, but received {chatgpt_account_id:?}.",
            )));
        }

        login_with_chatgpt_auth_tokens(
            &self.config.codex_home,
            &access_token,
            &chatgpt_account_id,
            chatgpt_plan_type.as_deref(),
        )
        .map_err(|err| internal_error(format!("failed to set external auth: {err}")))?;
        self.auth_manager.reload().await;
        self.config_manager.replace_cloud_config_bundle_loader(
            self.auth_manager.clone(),
            self.config.chatgpt_base_url.clone(),
        );
        self.config_manager
            .sync_default_client_residency_requirement()
            .await;

        Ok(LoginAccountResponse::ChatgptAuthTokens {})
    }

    async fn send_login_success_notifications(&self, login_id: Option<Uuid>) {
        Self::maybe_refresh_remote_installed_plugins_cache_for_current_config(
            &self.config_manager,
            &self.thread_manager,
            self.auth_manager.auth_cached(),
        )
        .await;

        let payload_login_completed = AccountLoginCompletedNotification {
            login_id: login_id.map(|id| id.to_string()),
            success: true,
            error: None,
        };
        self.outgoing
            .send_server_notification(ServerNotification::AccountLoginCompleted(
                payload_login_completed,
            ))
            .await;

        self.outgoing
            .send_server_notification(ServerNotification::AccountUpdated(
                self.current_account_updated_notification(),
            ))
            .await;
    }

    async fn logout_common(&self) -> std::result::Result<Option<AuthMode>, JSONRPCErrorError> {
        match self.auth_manager.logout_with_revoke().await {
            Ok(_) => {}
            Err(err) => {
                return Err(internal_error(format!("logout failed: {err}")));
            }
        }

        Self::maybe_refresh_remote_installed_plugins_cache_for_current_config(
            &self.config_manager,
            &self.thread_manager,
            self.auth_manager.auth_cached(),
        )
        .await;

        // Reflect the current auth method after logout (likely None).
        Ok(self
            .auth_manager
            .auth_cached()
            .as_ref()
            .map(CodexAuth::api_auth_mode))
    }

    async fn logout_v2(&self, request_id: ConnectionRequestId) -> Result<(), JSONRPCErrorError> {
        let result = self.logout_common().await;
        let account_updated =
            result
                .as_ref()
                .ok()
                .cloned()
                .map(|auth_mode| AccountUpdatedNotification {
                    auth_mode,
                    plan_type: None,
                });
        self.outgoing
            .send_result(request_id, result.map(|_| LogoutAccountResponse {}))
            .await;

        if let Some(payload) = account_updated {
            self.outgoing
                .send_server_notification(ServerNotification::AccountUpdated(payload))
                .await;
        }
        Ok(())
    }

    async fn refresh_token_if_requested(&self, do_refresh: bool) -> RefreshTokenRequestOutcome {
        if self.auth_manager.is_external_chatgpt_auth_active() {
            return RefreshTokenRequestOutcome::NotAttemptedOrSucceeded;
        }
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

        // Determine whether auth is required based on the active model provider.
        // If a custom provider is configured with `requires_openai_auth == false`,
        // then no auth step is required; otherwise, default to requiring auth.
        let requires_openai_auth = self.config.model_provider.requires_openai_auth;

        let response = if !requires_openai_auth {
            GetAuthStatusResponse {
                auth_method: None,
                auth_token: None,
                requires_openai_auth: Some(false),
            }
        } else {
            let auth = if do_refresh {
                self.auth_manager.auth_cached()
            } else {
                self.auth_manager.auth().await
            };
            match auth {
                Some(auth) => {
                    let permanent_refresh_failure =
                        self.auth_manager.refresh_failure_for_auth(&auth).is_some();
                    let auth_mode = auth.api_auth_mode();
                    let (reported_auth_method, token_opt) = if matches!(
                        auth,
                        CodexAuth::AgentIdentity(_) | CodexAuth::PersonalAccessToken(_)
                    ) || include_token
                        && permanent_refresh_failure
                    {
                        // This response cannot represent the metadata needed to reuse these
                        // credentials.
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
                        requires_openai_auth: Some(true),
                    }
                }
                None => GetAuthStatusResponse {
                    auth_method: None,
                    auth_token: None,
                    requires_openai_auth: Some(true),
                },
            }
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
        let account_state = match provider.account_state() {
            Ok(account_state) => account_state,
            Err(err) => return Err(invalid_request(err.to_string())),
        };
        let account = account_state.account.map(Account::from);

        Ok(GetAccountResponse {
            account,
            requires_openai_auth: account_state.requires_openai_auth,
        })
    }

    async fn get_account_rate_limits_response(
        &self,
    ) -> Result<GetAccountRateLimitsResponse, JSONRPCErrorError> {
        self.fetch_account_rate_limits()
            .await
            .map(
                |(rate_limits, rate_limits_by_limit_id)| GetAccountRateLimitsResponse {
                    rate_limits: rate_limits.into(),
                    rate_limits_by_limit_id: Some(
                        rate_limits_by_limit_id
                            .into_iter()
                            .map(|(limit_id, snapshot)| (limit_id, snapshot.into()))
                            .collect(),
                    ),
                },
            )
    }

    async fn get_account_token_usage_response(
        &self,
    ) -> Result<GetAccountTokenUsageResponse, JSONRPCErrorError> {
        let Some(auth) = self.auth_manager.auth().await else {
            return Err(invalid_request(
                "codex account authentication required to read token usage",
            ));
        };

        if !auth.uses_codex_backend() {
            return Err(invalid_request(
                "chatgpt authentication required to read token usage",
            ));
        }

        let client = BackendClient::from_auth(self.config.chatgpt_base_url.clone(), &auth)
            .map_err(|err| internal_error(format!("failed to construct backend client: {err}")))?;
        let profile = tokio::time::timeout(
            ACCOUNT_TOKEN_USAGE_FETCH_TIMEOUT,
            client.get_token_usage_profile(),
        )
        .await
        .map_err(|_| internal_error("token usage profile fetch timed out"))?
        .map_err(|err| internal_error(format!("failed to fetch token usage profile: {err}")))?;
        Ok(Self::account_token_usage_response(profile))
    }

    fn account_token_usage_response(profile: TokenUsageProfile) -> GetAccountTokenUsageResponse {
        let stats = profile.stats;
        GetAccountTokenUsageResponse {
            summary: AccountTokenUsageSummary {
                lifetime_tokens: stats.lifetime_tokens,
                peak_daily_tokens: stats.peak_daily_tokens,
                longest_running_turn_sec: stats.longest_running_turn_sec,
                current_streak_days: stats.current_streak_days,
                longest_streak_days: stats.longest_streak_days,
            },
            daily_usage_buckets: stats.daily_usage_buckets.map(|buckets| {
                buckets
                    .into_iter()
                    .map(|bucket| AccountTokenUsageDailyBucket {
                        start_date: bucket.start_date,
                        tokens: bucket.tokens,
                    })
                    .collect()
            }),
        }
    }

    async fn send_add_credits_nudge_email_response(
        &self,
        params: SendAddCreditsNudgeEmailParams,
    ) -> Result<SendAddCreditsNudgeEmailResponse, JSONRPCErrorError> {
        self.send_add_credits_nudge_email_inner(params)
            .await
            .map(|status| SendAddCreditsNudgeEmailResponse { status })
    }

    async fn send_add_credits_nudge_email_inner(
        &self,
        params: SendAddCreditsNudgeEmailParams,
    ) -> Result<AddCreditsNudgeEmailStatus, JSONRPCErrorError> {
        let Some(auth) = self.auth_manager.auth().await else {
            return Err(invalid_request(
                "codex account authentication required to notify workspace owner",
            ));
        };

        if !auth.uses_codex_backend() {
            return Err(invalid_request(
                "chatgpt authentication required to notify workspace owner",
            ));
        }

        let client = BackendClient::from_auth(self.config.chatgpt_base_url.clone(), &auth)
            .map_err(|err| internal_error(format!("failed to construct backend client: {err}")))?;

        match client
            .send_add_credits_nudge_email(Self::backend_credit_type(params.credit_type))
            .await
        {
            Ok(()) => Ok(AddCreditsNudgeEmailStatus::Sent),
            Err(err) if err.status().is_some_and(|status| status.as_u16() == 429) => {
                Ok(AddCreditsNudgeEmailStatus::CooldownActive)
            }
            Err(err) => Err(internal_error(format!(
                "failed to notify workspace owner: {err}"
            ))),
        }
    }

    fn backend_credit_type(value: AddCreditsNudgeCreditType) -> BackendAddCreditsNudgeCreditType {
        match value {
            AddCreditsNudgeCreditType::Credits => BackendAddCreditsNudgeCreditType::Credits,
            AddCreditsNudgeCreditType::UsageLimit => BackendAddCreditsNudgeCreditType::UsageLimit,
        }
    }

    async fn fetch_account_rate_limits(
        &self,
    ) -> Result<
        (
            CoreRateLimitSnapshot,
            HashMap<String, CoreRateLimitSnapshot>,
        ),
        JSONRPCErrorError,
    > {
        let Some(auth) = self.auth_manager.auth().await else {
            return Err(invalid_request(
                "codex account authentication required to read rate limits",
            ));
        };

        if !auth.uses_codex_backend() {
            return Err(invalid_request(
                "chatgpt authentication required to read rate limits",
            ));
        }

        let client = BackendClient::from_auth(self.config.chatgpt_base_url.clone(), &auth)
            .map_err(|err| internal_error(format!("failed to construct backend client: {err}")))?;

        let snapshots = client
            .get_rate_limits_many()
            .await
            .map_err(|err| internal_error(format!("failed to fetch codex rate limits: {err}")))?;
        if snapshots.is_empty() {
            return Err(internal_error(
                "failed to fetch codex rate limits: no snapshots returned",
            ));
        }

        let rate_limits_by_limit_id: HashMap<String, CoreRateLimitSnapshot> = snapshots
            .iter()
            .cloned()
            .map(|snapshot| {
                let limit_id = snapshot
                    .limit_id
                    .clone()
                    .unwrap_or_else(|| "codex".to_string());
                (limit_id, snapshot)
            })
            .collect();

        let primary = snapshots
            .iter()
            .find(|snapshot| snapshot.limit_id.as_deref() == Some("codex"))
            .cloned()
            .unwrap_or_else(|| snapshots[0].clone());

        Ok((primary, rate_limits_by_limit_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_backend_client::TokenUsageProfileDailyBucket;
    use codex_backend_client::TokenUsageProfileStats;
    use pretty_assertions::assert_eq;

    #[test]
    fn account_token_usage_response_maps_profile_stats_and_daily_buckets() {
        let response = AccountRequestProcessor::account_token_usage_response(TokenUsageProfile {
            stats: TokenUsageProfileStats {
                lifetime_tokens: Some(123),
                peak_daily_tokens: Some(45),
                longest_running_turn_sec: Some(67),
                current_streak_days: Some(8),
                longest_streak_days: Some(9),
                daily_usage_buckets: Some(vec![TokenUsageProfileDailyBucket {
                    start_date: "2026-05-29".to_string(),
                    tokens: 10,
                }]),
            },
        });

        assert_eq!(
            response,
            GetAccountTokenUsageResponse {
                summary: AccountTokenUsageSummary {
                    lifetime_tokens: Some(123),
                    peak_daily_tokens: Some(45),
                    longest_running_turn_sec: Some(67),
                    current_streak_days: Some(8),
                    longest_streak_days: Some(9),
                },
                daily_usage_buckets: Some(vec![AccountTokenUsageDailyBucket {
                    start_date: "2026-05-29".to_string(),
                    tokens: 10,
                }]),
            }
        );
    }
}
