//! UI-owned projection of MCP form schemas.
//!
//! This keeps protocol decoding in app-server v2 while giving request panes a
//! small, stable field description that does not depend on raw JSON rendering.

use codex_app_server_protocol::McpElicitationEnumSchema;
use codex_app_server_protocol::McpElicitationMultiSelectEnumSchema;
use codex_app_server_protocol::McpElicitationPrimitiveSchema;
use codex_app_server_protocol::McpElicitationSchema;
use codex_app_server_protocol::McpElicitationSingleSelectEnumSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpFormFieldKind {
    Text,
    Number,
    Boolean,
    SingleSelect,
    MultiSelect,
}

impl McpFormFieldKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Number => "number",
            Self::Boolean => "yes/no",
            Self::SingleSelect => "single select",
            Self::MultiSelect => "multi select",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpFormFieldSchema {
    pub(crate) name: String,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) required: bool,
    pub(crate) kind: McpFormFieldKind,
}

pub(crate) fn project_fields(schema: &McpElicitationSchema) -> Vec<McpFormFieldSchema> {
    schema
        .properties
        .iter()
        .map(|(name, property)| {
            let (title, description, kind) = field_metadata(property);
            McpFormFieldSchema {
                name: name.clone(),
                title: title.cloned().unwrap_or_else(|| name.clone()),
                description: description.cloned(),
                required: schema
                    .required
                    .as_ref()
                    .is_some_and(|required| required.contains(name)),
                kind,
            }
        })
        .collect()
}

fn field_metadata(
    schema: &McpElicitationPrimitiveSchema,
) -> (Option<&String>, Option<&String>, McpFormFieldKind) {
    match schema {
        McpElicitationPrimitiveSchema::String(schema) => (
            schema.title.as_ref(),
            schema.description.as_ref(),
            McpFormFieldKind::Text,
        ),
        McpElicitationPrimitiveSchema::Number(schema) => (
            schema.title.as_ref(),
            schema.description.as_ref(),
            McpFormFieldKind::Number,
        ),
        McpElicitationPrimitiveSchema::Boolean(schema) => (
            schema.title.as_ref(),
            schema.description.as_ref(),
            McpFormFieldKind::Boolean,
        ),
        McpElicitationPrimitiveSchema::Enum(schema) => match schema {
            McpElicitationEnumSchema::Legacy(schema) => (
                schema.title.as_ref(),
                schema.description.as_ref(),
                McpFormFieldKind::SingleSelect,
            ),
            McpElicitationEnumSchema::SingleSelect(schema) => match schema {
                McpElicitationSingleSelectEnumSchema::Untitled(schema) => (
                    schema.title.as_ref(),
                    schema.description.as_ref(),
                    McpFormFieldKind::SingleSelect,
                ),
                McpElicitationSingleSelectEnumSchema::Titled(schema) => (
                    schema.title.as_ref(),
                    schema.description.as_ref(),
                    McpFormFieldKind::SingleSelect,
                ),
            },
            McpElicitationEnumSchema::MultiSelect(schema) => match schema {
                McpElicitationMultiSelectEnumSchema::Untitled(schema) => (
                    schema.title.as_ref(),
                    schema.description.as_ref(),
                    McpFormFieldKind::MultiSelect,
                ),
                McpElicitationMultiSelectEnumSchema::Titled(schema) => (
                    schema.title.as_ref(),
                    schema.description.as_ref(),
                    McpFormFieldKind::MultiSelect,
                ),
            },
        },
    }
}

#[cfg(test)]
#[path = "mcp_form_schema_tests.rs"]
mod tests;
