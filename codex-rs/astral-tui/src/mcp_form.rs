//! Compiled MCP form controls shared by request rendering and interaction.

mod field;

use codex_app_server_protocol::McpElicitationSchema;

pub(crate) use field::McpFormField;

pub(crate) fn compile_fields(schema: &McpElicitationSchema) -> Vec<McpFormField> {
    crate::mcp_form_schema::project_fields(schema)
        .into_iter()
        .filter_map(|field| {
            schema
                .properties
                .get(&field.name)
                .map(|property| McpFormField::from_schema(field, property))
        })
        .collect()
}

#[cfg(test)]
#[path = "mcp_form_tests.rs"]
mod tests;
