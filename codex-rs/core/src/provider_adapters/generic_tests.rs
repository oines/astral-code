use super::*;
use crate::client_common::Prompt;
use codex_protocol::config_types::ReasoningSummary;
use pretty_assertions::assert_eq;

#[test]
fn generic_responses_does_not_apply_codex_lite_projection() {
    let prompt = Prompt {
        parallel_tool_calls: true,
        ..Prompt::default()
    };
    let mut model_info = codex_models_manager::model_info::model_info_from_slug("gpt-test");
    model_info.use_responses_lite = true;

    let request = build_responses_request(ResponsesRequestParams {
        prompt: &prompt,
        model_info: &model_info,
        effort: None,
        summary: ReasoningSummary::None,
        service_tier: None,
        prompt_cache_key: "cache-key".to_string(),
    })
    .expect("generic Responses request should build");

    assert!(request.parallel_tool_calls);
    assert_eq!(request.reasoning, None);
}
