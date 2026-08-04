//! Typed form state for MCP elicitation prompts.
//!
//! The model consumes app-server's strong schema directly. Rendering and input
//! stay separate so both inline and fullscreen hosts can share one form state.

mod field;
pub(super) mod model;
mod presenter;

pub(in crate::prompt_interaction) use presenter::McpFormPrompt;

#[cfg(test)]
#[path = "mcp_form_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "mcp_form_model_tests.rs"]
mod model_tests;
