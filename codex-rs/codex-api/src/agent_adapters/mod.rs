use codex_agent_protocol::AgentRequest;
use codex_agent_protocol::PROVIDER_FLAVOR_METADATA_KEY;
use serde_json::Map;
use serde_json::Value;

pub mod anthropic;
pub mod chat_completions;

pub(crate) const CHAT_REASONING_CONTENT_METADATA_KEY: &str = "astral_chat_reasoning_content";

pub(crate) fn apply_provider_body_overrides(body: &mut Map<String, Value>, request: &AgentRequest) {
    for (key, value) in &request.metadata.provider {
        if matches!(
            key.as_str(),
            PROVIDER_FLAVOR_METADATA_KEY | CHAT_REASONING_CONTENT_METADATA_KEY
        ) {
            continue;
        }
        if value.is_null() {
            body.remove(key);
        } else {
            body.insert(key.clone(), value.clone());
        }
    }
}
