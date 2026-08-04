//! Typed form state for MCP elicitation prompts.
//!
//! The model consumes app-server's strong schema directly. Rendering and input
//! stay separate so both inline and fullscreen hosts can share one form state.

mod field;

// The interaction model lands one slice before its presenter. Remove this
// temporary allowance when the presenter starts consuming it.
#[allow(dead_code)]
pub(super) mod model;

#[cfg(test)]
#[path = "mcp_form_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "mcp_form_model_tests.rs"]
mod model_tests;
