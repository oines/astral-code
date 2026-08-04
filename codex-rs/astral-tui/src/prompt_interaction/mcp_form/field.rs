//! Projection of MCP primitive schemas into typed form fields.

use std::collections::BTreeSet;

use codex_app_server_protocol::McpElicitationEnumSchema;
use codex_app_server_protocol::McpElicitationMultiSelectEnumSchema;
use codex_app_server_protocol::McpElicitationNumberType;
use codex_app_server_protocol::McpElicitationPrimitiveSchema;
use codex_app_server_protocol::McpElicitationSingleSelectEnumSchema;
use codex_app_server_protocol::McpElicitationStringFormat;
use serde_json::Number;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct McpFormOption {
    pub(super) label: String,
    value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum McpFormTextKind {
    String {
        min_length: Option<u32>,
        max_length: Option<u32>,
        format: Option<McpElicitationStringFormat>,
    },
    Number {
        integer: bool,
        minimum: Option<f64>,
        maximum: Option<f64>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum McpFormControl {
    Text {
        draft: String,
        cursor: usize,
        committed: bool,
        kind: McpFormTextKind,
    },
    Select {
        options: Vec<McpFormOption>,
        cursor: usize,
        selected: BTreeSet<usize>,
        multiple: bool,
        min_selected: Option<u64>,
        max_selected: Option<u64>,
        committed: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct McpFormField {
    pub(super) name: String,
    pub(super) title: String,
    pub(super) description: Option<String>,
    pub(super) required: bool,
    pub(super) control: McpFormControl,
}

impl McpFormField {
    pub(super) fn new(name: &str, schema: &McpElicitationPrimitiveSchema, required: bool) -> Self {
        let (title, description, control) = match schema {
            McpElicitationPrimitiveSchema::String(schema) => (
                schema.title.clone(),
                schema.description.clone(),
                McpFormControl::Text {
                    draft: schema.default.clone().unwrap_or_default(),
                    cursor: schema.default.as_ref().map_or(0, String::len),
                    committed: schema.default.is_some(),
                    kind: McpFormTextKind::String {
                        min_length: schema.min_length,
                        max_length: schema.max_length,
                        format: schema.format,
                    },
                },
            ),
            McpElicitationPrimitiveSchema::Number(schema) => {
                let draft = schema.default.map_or_else(String::new, |value| {
                    if schema.type_ == McpElicitationNumberType::Integer && value.fract() == 0.0 {
                        format!("{value:.0}")
                    } else {
                        value.to_string()
                    }
                });
                (
                    schema.title.clone(),
                    schema.description.clone(),
                    McpFormControl::Text {
                        cursor: draft.len(),
                        draft,
                        committed: schema.default.is_some(),
                        kind: McpFormTextKind::Number {
                            integer: schema.type_ == McpElicitationNumberType::Integer,
                            minimum: schema.minimum,
                            maximum: schema.maximum,
                        },
                    },
                )
            }
            McpElicitationPrimitiveSchema::Boolean(schema) => (
                schema.title.clone(),
                schema.description.clone(),
                select_control(
                    vec![
                        option("Yes", Value::Bool(true)),
                        option("No", Value::Bool(false)),
                    ],
                    schema.default.map(Value::Bool),
                    false,
                    None,
                    None,
                    schema.default.is_some(),
                ),
            ),
            McpElicitationPrimitiveSchema::Enum(schema) => enum_control(schema),
        };
        Self {
            name: name.to_string(),
            title: title.unwrap_or_else(|| name.to_string()),
            description,
            required,
            control,
        }
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        match &self.control {
            McpFormControl::Text {
                draft,
                committed,
                kind,
                ..
            } => {
                if !committed {
                    return self.require_answer();
                }
                match kind {
                    McpFormTextKind::String {
                        min_length,
                        max_length,
                        ..
                    } => {
                        let length = draft.chars().count() as u32;
                        if let Some(minimum) = min_length
                            && length < *minimum
                        {
                            return Err(format!("Enter at least {minimum} characters"));
                        }
                        if let Some(maximum) = max_length
                            && length > *maximum
                        {
                            return Err(format!("Enter at most {maximum} characters"));
                        }
                    }
                    McpFormTextKind::Number {
                        integer,
                        minimum,
                        maximum,
                    } => {
                        let value = parse_number(draft, *integer)?;
                        if let Some(minimum) = minimum
                            && value < *minimum
                        {
                            return Err(format!("Value must be at least {minimum}"));
                        }
                        if let Some(maximum) = maximum
                            && value > *maximum
                        {
                            return Err(format!("Value must be at most {maximum}"));
                        }
                    }
                }
                Ok(())
            }
            McpFormControl::Select {
                selected,
                committed,
                min_selected,
                max_selected,
                ..
            } => {
                if !committed {
                    return self.require_answer();
                }
                let count = selected.len() as u64;
                if let Some(minimum) = min_selected
                    && count < *minimum
                {
                    return Err(format!("Choose at least {minimum} options"));
                }
                if let Some(maximum) = max_selected
                    && count > *maximum
                {
                    return Err(format!("Choose at most {maximum} options"));
                }
                Ok(())
            }
        }
    }

    pub(super) fn value(&self) -> Option<Value> {
        match &self.control {
            McpFormControl::Text {
                draft,
                committed: true,
                kind,
                ..
            } => match kind {
                McpFormTextKind::String { .. } => Some(Value::String(draft.clone())),
                McpFormTextKind::Number { integer, .. } => {
                    number_value(draft, *integer).map(Value::Number)
                }
            },
            McpFormControl::Select {
                options,
                selected,
                multiple,
                committed: true,
                ..
            } => {
                let values = selected
                    .iter()
                    .filter_map(|index| options.get(*index).map(|option| option.value.clone()))
                    .collect::<Vec<_>>();
                if *multiple {
                    Some(Value::Array(values))
                } else {
                    values.into_iter().next()
                }
            }
            _ => None,
        }
    }

    fn require_answer(&self) -> Result<(), String> {
        if self.required {
            Err("This field is required".to_string())
        } else {
            Ok(())
        }
    }
}

fn enum_control(
    schema: &McpElicitationEnumSchema,
) -> (Option<String>, Option<String>, McpFormControl) {
    match schema {
        McpElicitationEnumSchema::Legacy(schema) => (
            schema.title.clone(),
            schema.description.clone(),
            select_control(
                schema
                    .enum_
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        option(
                            schema
                                .enum_names
                                .as_ref()
                                .and_then(|names| names.get(index))
                                .unwrap_or(value),
                            Value::String(value.clone()),
                        )
                    })
                    .collect(),
                schema.default.clone().map(Value::String),
                false,
                None,
                None,
                schema.default.is_some(),
            ),
        ),
        McpElicitationEnumSchema::SingleSelect(schema) => {
            let (title, description, options, default) = match schema {
                McpElicitationSingleSelectEnumSchema::Untitled(schema) => (
                    schema.title.clone(),
                    schema.description.clone(),
                    string_options(&schema.enum_),
                    schema.default.clone(),
                ),
                McpElicitationSingleSelectEnumSchema::Titled(schema) => (
                    schema.title.clone(),
                    schema.description.clone(),
                    schema
                        .one_of
                        .iter()
                        .map(|item| option(&item.title, Value::String(item.const_.clone())))
                        .collect(),
                    schema.default.clone(),
                ),
            };
            let committed = default.is_some();
            (
                title,
                description,
                select_control(
                    options,
                    default.map(Value::String),
                    false,
                    None,
                    None,
                    committed,
                ),
            )
        }
        McpElicitationEnumSchema::MultiSelect(schema) => {
            let (title, description, options, defaults, minimum, maximum) = match schema {
                McpElicitationMultiSelectEnumSchema::Untitled(schema) => (
                    schema.title.clone(),
                    schema.description.clone(),
                    string_options(&schema.items.enum_),
                    schema.default.clone(),
                    schema.min_items,
                    schema.max_items,
                ),
                McpElicitationMultiSelectEnumSchema::Titled(schema) => (
                    schema.title.clone(),
                    schema.description.clone(),
                    schema
                        .items
                        .any_of
                        .iter()
                        .map(|item| option(&item.title, Value::String(item.const_.clone())))
                        .collect(),
                    schema.default.clone(),
                    schema.min_items,
                    schema.max_items,
                ),
            };
            let committed = defaults.is_some();
            (
                title,
                description,
                select_control(
                    options,
                    defaults.unwrap_or_default().into_iter().map(Value::String),
                    true,
                    minimum,
                    maximum,
                    committed,
                ),
            )
        }
    }
}

fn select_control(
    options: Vec<McpFormOption>,
    defaults: impl IntoIterator<Item = Value>,
    multiple: bool,
    min_selected: Option<u64>,
    max_selected: Option<u64>,
    committed: bool,
) -> McpFormControl {
    let selected = defaults
        .into_iter()
        .filter_map(|default| options.iter().position(|option| option.value == default))
        .collect::<BTreeSet<_>>();
    McpFormControl::Select {
        cursor: selected.first().copied().unwrap_or_default(),
        options,
        selected,
        multiple,
        min_selected,
        max_selected,
        committed,
    }
}

fn string_options(values: &[String]) -> Vec<McpFormOption> {
    values
        .iter()
        .map(|value| option(value, Value::String(value.clone())))
        .collect()
}

fn option(label: &str, value: Value) -> McpFormOption {
    McpFormOption {
        label: label.to_string(),
        value,
    }
}

fn parse_number(draft: &str, integer: bool) -> Result<f64, String> {
    if integer {
        draft
            .trim()
            .parse::<i64>()
            .map(|value| value as f64)
            .map_err(|_| "Enter a whole number".to_string())
    } else {
        draft
            .trim()
            .parse::<f64>()
            .map_err(|_| "Enter a valid number".to_string())
    }
}

fn number_value(draft: &str, integer: bool) -> Option<Number> {
    if integer {
        draft.trim().parse::<i64>().ok().map(Number::from)
    } else {
        draft.trim().parse::<f64>().ok().and_then(Number::from_f64)
    }
}
