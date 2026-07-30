use serde_json::Map;
use serde_json::Number;
use serde_json::Value;

pub(super) fn text_value(raw: &Map<String, Value>, key: &str) -> String {
    match raw.get(key) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        Some(Value::Bool(_) | Value::Object(_) | Value::Null) | None => String::new(),
    }
}

pub(super) fn set_optional_number(
    raw: &mut Map<String, Value>,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() {
        raw.remove(key);
        return Ok(());
    }
    let value = value
        .parse::<u64>()
        .map_err(|_| format!("{key} must be a positive integer"))?;
    if value == 0 {
        return Err(format!("{key} must be a positive integer"));
    }
    raw.insert(key.to_string(), Value::Number(Number::from(value)));
    Ok(())
}

pub(super) fn set_trimmed_string(raw: &mut Map<String, Value>, key: &str, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        raw.remove(key);
    } else {
        raw.insert(key.to_string(), Value::String(value.to_string()));
    }
}
