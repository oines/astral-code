use std::collections::BTreeSet;

use codex_app_server_protocol::McpElicitationEnumSchema;
use codex_app_server_protocol::McpElicitationMultiSelectEnumSchema;
use codex_app_server_protocol::McpElicitationNumberType;
use codex_app_server_protocol::McpElicitationPrimitiveSchema;
use codex_app_server_protocol::McpElicitationSingleSelectEnumSchema;
use serde_json::Number;
use serde_json::Value;

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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct McpFormField {
    pub(crate) schema: McpFormFieldSchema,
    pub(crate) control: McpFormControl,
    raw_schema: McpElicitationPrimitiveSchema,
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
            raw_schema: schema.clone(),
        }
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        match (&self.raw_schema, &self.control) {
            (McpElicitationPrimitiveSchema::String(schema), McpFormControl::Text { value }) => {
                if value.is_empty() {
                    return self.required();
                }
                let length = value.chars().count() as u32;
                if let Some(minimum) = schema.min_length
                    && length < minimum
                {
                    return Err(format!("Enter at least {minimum} characters"));
                }
                if let Some(maximum) = schema.max_length
                    && length > maximum
                {
                    return Err(format!("Enter at most {maximum} characters"));
                }
                Ok(())
            }
            (McpElicitationPrimitiveSchema::Number(schema), McpFormControl::Text { value }) => {
                if value.trim().is_empty() {
                    return self.required();
                }
                let number = parse_number(schema.type_, value)?;
                if let Some(minimum) = schema.minimum
                    && number < minimum
                {
                    return Err(format!("Value must be at least {minimum}"));
                }
                if let Some(maximum) = schema.maximum
                    && number > maximum
                {
                    return Err(format!("Value must be at most {maximum}"));
                }
                Ok(())
            }
            (
                McpElicitationPrimitiveSchema::Boolean(_)
                | McpElicitationPrimitiveSchema::Enum(
                    McpElicitationEnumSchema::Legacy(_) | McpElicitationEnumSchema::SingleSelect(_),
                ),
                McpFormControl::Select { selected, .. },
            ) => {
                if selected.is_empty() {
                    self.required()
                } else {
                    Ok(())
                }
            }
            (
                McpElicitationPrimitiveSchema::Enum(McpElicitationEnumSchema::MultiSelect(schema)),
                McpFormControl::Select { selected, .. },
            ) => {
                if selected.is_empty() {
                    return self.required();
                }
                let (minimum, maximum) = match schema {
                    McpElicitationMultiSelectEnumSchema::Untitled(schema) => {
                        (schema.min_items, schema.max_items)
                    }
                    McpElicitationMultiSelectEnumSchema::Titled(schema) => {
                        (schema.min_items, schema.max_items)
                    }
                };
                let count = selected.len() as u64;
                if let Some(minimum) = minimum
                    && count < minimum
                {
                    return Err(format!("Choose at least {minimum} options"));
                }
                if let Some(maximum) = maximum
                    && count > maximum
                {
                    return Err(format!("Choose at most {maximum} options"));
                }
                Ok(())
            }
            _ => Err("Unsupported MCP form field".to_string()),
        }
    }

    pub(super) fn value(&self) -> Option<Value> {
        match (&self.raw_schema, &self.control) {
            (McpElicitationPrimitiveSchema::String(_), McpFormControl::Text { value })
                if !value.is_empty() =>
            {
                Some(Value::String(value.clone()))
            }
            (McpElicitationPrimitiveSchema::Number(schema), McpFormControl::Text { value })
                if !value.trim().is_empty() =>
            {
                let number = if schema.type_ == McpElicitationNumberType::Integer {
                    value.trim().parse::<i64>().ok().map(Number::from)
                } else {
                    value.trim().parse().ok().and_then(Number::from_f64)
                };
                number.map(Value::Number)
            }
            (
                McpElicitationPrimitiveSchema::Boolean(_),
                McpFormControl::Select { selected, .. },
            ) => selected.first().map(|index| Value::Bool(*index == 0)),
            (
                McpElicitationPrimitiveSchema::Enum(schema),
                McpFormControl::Select { selected, .. },
            ) => enum_value(schema, selected),
            _ => None,
        }
    }

    fn required(&self) -> Result<(), String> {
        if self.schema.required {
            Err("This field is required".to_string())
        } else {
            Ok(())
        }
    }
}

fn enum_value(schema: &McpElicitationEnumSchema, selected: &BTreeSet<usize>) -> Option<Value> {
    match schema {
        McpElicitationEnumSchema::Legacy(schema) => selected
            .first()
            .and_then(|index| schema.enum_.get(*index))
            .cloned()
            .map(Value::String),
        McpElicitationEnumSchema::SingleSelect(schema) => {
            let value = match schema {
                McpElicitationSingleSelectEnumSchema::Untitled(schema) => selected
                    .first()
                    .and_then(|index| schema.enum_.get(*index))
                    .cloned(),
                McpElicitationSingleSelectEnumSchema::Titled(schema) => selected
                    .first()
                    .and_then(|index| schema.one_of.get(*index))
                    .map(|option| option.const_.clone()),
            };
            value.map(Value::String)
        }
        McpElicitationEnumSchema::MultiSelect(schema) if !selected.is_empty() => {
            let values = match schema {
                McpElicitationMultiSelectEnumSchema::Untitled(schema) => selected
                    .iter()
                    .filter_map(|index| schema.items.enum_.get(*index).cloned())
                    .map(Value::String)
                    .collect(),
                McpElicitationMultiSelectEnumSchema::Titled(schema) => selected
                    .iter()
                    .filter_map(|index| schema.items.any_of.get(*index))
                    .map(|option| Value::String(option.const_.clone()))
                    .collect(),
            };
            Some(Value::Array(values))
        }
        McpElicitationEnumSchema::MultiSelect(_) => None,
    }
}

fn parse_number(type_: McpElicitationNumberType, value: &str) -> Result<f64, String> {
    match type_ {
        McpElicitationNumberType::Integer => value
            .trim()
            .parse::<i64>()
            .map(|value| value as f64)
            .map_err(|_| "Enter a whole number".to_string()),
        McpElicitationNumberType::Number => {
            let number = value
                .trim()
                .parse::<f64>()
                .map_err(|_| "Enter a valid number".to_string())?;
            if number.is_finite() {
                Ok(number)
            } else {
                Err("Enter a finite number".to_string())
            }
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
