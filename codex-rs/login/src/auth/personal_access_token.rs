use codex_protocol::account::PlanType as AccountPlanType;
use codex_protocol::auth::PlanType as InternalPlanType;
use serde::Deserialize;
use std::fmt;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct PersonalAccessTokenMetadata {
    email: String,
    chatgpt_user_id: String,
    chatgpt_account_id: String,
    chatgpt_plan_type: String,
    chatgpt_account_is_fedramp: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct PersonalAccessTokenAuth {
    access_token: String,
    metadata: PersonalAccessTokenMetadata,
}

impl fmt::Debug for PersonalAccessTokenAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PersonalAccessTokenAuth")
            .field("access_token", &"<redacted>")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl PersonalAccessTokenAuth {
    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    pub fn account_id(&self) -> &str {
        &self.metadata.chatgpt_account_id
    }

    pub fn chatgpt_user_id(&self) -> &str {
        &self.metadata.chatgpt_user_id
    }

    pub fn email(&self) -> &str {
        &self.metadata.email
    }

    pub fn plan_type(&self) -> AccountPlanType {
        InternalPlanType::from_raw_value(&self.metadata.chatgpt_plan_type).into()
    }

    pub fn is_fedramp_account(&self) -> bool {
        self.metadata.chatgpt_account_is_fedramp
    }
}
