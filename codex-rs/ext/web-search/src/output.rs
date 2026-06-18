use codex_extension_api::ToolOutput;
use codex_extension_api::ToolPayload;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;

pub(crate) struct WebToolOutput {
    output: String,
    success: bool,
}

impl WebToolOutput {
    pub(crate) fn new(output: String) -> Self {
        Self {
            output,
            success: true,
        }
    }

    pub(crate) fn failure(output: String) -> Self {
        Self {
            output,
            success: false,
        }
    }
}

impl ToolOutput for WebToolOutput {
    fn log_preview(&self) -> String {
        "[web tool output]".to_string()
    }

    fn success_for_logging(&self) -> bool {
        self.success
    }

    fn contains_external_context(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, _payload: &ToolPayload) -> ResponseInputItem {
        ResponseInputItem::FunctionCallOutput {
            call_id: call_id.to_string(),
            output: FunctionCallOutputPayload {
                body: codex_protocol::models::FunctionCallOutputBody::ContentItems(vec![
                    FunctionCallOutputContentItem::InputText {
                        text: self.output.clone(),
                    },
                ]),
                success: Some(self.success),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use codex_extension_api::ToolPayload;
    use codex_protocol::models::FunctionCallOutputContentItem;
    use codex_protocol::models::FunctionCallOutputPayload;
    use codex_protocol::models::ResponseInputItem;
    use pretty_assertions::assert_eq;

    use super::ToolOutput;
    use super::WebToolOutput;

    #[test]
    fn emits_plaintext_function_call_output() {
        let output = WebToolOutput::new("web output".to_string());

        assert_eq!(
            output.to_response_item(
                "call-1",
                &ToolPayload::Function {
                    arguments: "{}".to_string(),
                },
            ),
            ResponseInputItem::FunctionCallOutput {
                call_id: "call-1".to_string(),
                output: FunctionCallOutputPayload {
                    body: codex_protocol::models::FunctionCallOutputBody::ContentItems(vec![
                        FunctionCallOutputContentItem::InputText {
                            text: "web output".to_string(),
                        },
                    ]),
                    success: Some(true),
                },
            }
        );
    }

    #[test]
    fn emits_failed_function_call_output() {
        let output = WebToolOutput::failure("search failed".to_string());

        assert_eq!(
            output.to_response_item(
                "call-1",
                &ToolPayload::Function {
                    arguments: "{}".to_string(),
                },
            ),
            ResponseInputItem::FunctionCallOutput {
                call_id: "call-1".to_string(),
                output: FunctionCallOutputPayload {
                    body: codex_protocol::models::FunctionCallOutputBody::ContentItems(vec![
                        FunctionCallOutputContentItem::InputText {
                            text: "search failed".to_string(),
                        },
                    ]),
                    success: Some(false),
                },
            }
        );
    }
}
