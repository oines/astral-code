use std::collections::BTreeSet;

use codex_app_server_protocol::McpElicitationEnumSchema;
use codex_app_server_protocol::McpElicitationMultiSelectEnumSchema;
use codex_app_server_protocol::McpElicitationPrimitiveSchema;
use codex_app_server_protocol::McpElicitationSingleSelectEnumSchema;

use crate::mcp_form_schema::McpFormFieldSchema;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpFormChoice {
    pub(crate) label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum McpFormControl {
    Text {
        value: String,
    },
    Select {
        choices: Vec<McpFormChoice>,
        cursor: usize,
        selected: BTreeSet<usize>,
        multiple: bool,
    },
}

impl McpFormControl {
    pub(crate) fn preview_detail(&self) -> Option<String> {
        match self {
            Self::Text { value } if value.is_empty() => None,
            Self::Text { .. } => Some("default set".to_string()),
            Self::Select {
                choices, selected, ..
            } => {
                let defaults = if selected.is_empty() {
                    String::new()
                } else {
                    format!(" · {} default", selected.len())
                };
                Some(format!("{} options{defaults}", choices.len()))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpFormField {
    pub(crate) schema: McpFormFieldSchema,
    pub(crate) control: McpFormControl,
}

impl McpFormField {
    pub(super) fn from_schema(
        field: McpFormFieldSchema,
        schema: &McpElicitationPrimitiveSchema,
    ) -> Self {
        let control = match schema {
            McpElicitationPrimitiveSchema::String(schema) => McpFormControl::Text {
                value: schema.default.clone().unwrap_or_default(),
            },
            McpElicitationPrimitiveSchema::Number(schema) => McpFormControl::Text {
                value: schema
                    .default
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            },
            McpElicitationPrimitiveSchema::Boolean(schema) => select_control(
                vec![choice("Yes"), choice("No")],
                schema.default.map(|value| usize::from(!value)).into_iter(),
                /*multiple*/ false,
            ),
            McpElicitationPrimitiveSchema::Enum(schema) => enum_control(schema),
        };
        Self {
            schema: field,
            control,
        }
    }
}

fn enum_control(schema: &McpElicitationEnumSchema) -> McpFormControl {
    match schema {
        McpElicitationEnumSchema::Legacy(schema) => select_control(
            schema
                .enum_
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    choice(
                        schema
                            .enum_names
                            .as_ref()
                            .and_then(|names| names.get(index))
                            .unwrap_or(value),
                    )
                })
                .collect(),
            selected_default(&schema.enum_, schema.default.as_ref()).into_iter(),
            /*multiple*/ false,
        ),
        McpElicitationEnumSchema::SingleSelect(schema) => match schema {
            McpElicitationSingleSelectEnumSchema::Untitled(schema) => select_control(
                string_choices(&schema.enum_),
                selected_default(&schema.enum_, schema.default.as_ref()).into_iter(),
                /*multiple*/ false,
            ),
            McpElicitationSingleSelectEnumSchema::Titled(schema) => select_control(
                schema
                    .one_of
                    .iter()
                    .map(|option| choice(&option.title))
                    .collect(),
                schema
                    .default
                    .as_ref()
                    .and_then(|default| {
                        schema
                            .one_of
                            .iter()
                            .position(|option| &option.const_ == default)
                    })
                    .into_iter(),
                /*multiple*/ false,
            ),
        },
        McpElicitationEnumSchema::MultiSelect(schema) => match schema {
            McpElicitationMultiSelectEnumSchema::Untitled(schema) => select_control(
                string_choices(&schema.items.enum_),
                selected_defaults(&schema.items.enum_, schema.default.as_deref()),
                /*multiple*/ true,
            ),
            McpElicitationMultiSelectEnumSchema::Titled(schema) => select_control(
                schema
                    .items
                    .any_of
                    .iter()
                    .map(|option| choice(&option.title))
                    .collect(),
                schema.default.iter().flatten().filter_map(|default| {
                    schema
                        .items
                        .any_of
                        .iter()
                        .position(|option| &option.const_ == default)
                }),
                /*multiple*/ true,
            ),
        },
    }
}

fn select_control(
    choices: Vec<McpFormChoice>,
    defaults: impl Iterator<Item = usize>,
    multiple: bool,
) -> McpFormControl {
    let selected = defaults.collect::<BTreeSet<_>>();
    McpFormControl::Select {
        cursor: selected.first().copied().unwrap_or_default(),
        choices,
        selected,
        multiple,
    }
}

fn selected_default(values: &[String], default: Option<&String>) -> Option<usize> {
    default.and_then(|default| values.iter().position(|value| value == default))
}

fn selected_defaults<'a>(
    values: &'a [String],
    defaults: Option<&'a [String]>,
) -> impl Iterator<Item = usize> + 'a {
    defaults
        .unwrap_or_default()
        .iter()
        .filter_map(|default| values.iter().position(|value| value == default))
}

fn string_choices(values: &[String]) -> Vec<McpFormChoice> {
    values.iter().map(String::as_str).map(choice).collect()
}

fn choice(label: &str) -> McpFormChoice {
    McpFormChoice {
        label: label.to_string(),
    }
}
