//! Typed form state for MCP elicitation prompts.
//!
//! The model consumes app-server's strong schema directly. Rendering and input
//! stay separate so both inline and fullscreen hosts can share one form state.

// The schema projection lands one slice before the interaction model. Remove
// this temporary allowance when that model starts consuming it.
#[allow(dead_code)]
mod field;

#[cfg(test)]
#[path = "mcp_form_tests.rs"]
mod tests;
