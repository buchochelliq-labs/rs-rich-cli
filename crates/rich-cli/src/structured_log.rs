//! JSONL boundary conversion; presentation and typed fields live in rich-ext.
use rich_ext::event::{EventContext, Message, Severity, StructuredEvent, Value};
pub(super) fn event(input: &serde_json::Value) -> StructuredEvent {
    let Some(object) = input.as_object() else {
        return StructuredEvent::new(Message::Literal(input.to_string()));
    };
    let message_key = ["message", "msg"]
        .into_iter()
        .find(|key| object.contains_key(*key));
    let level_key = ["level", "severity"]
        .into_iter()
        .find(|key| object.contains_key(*key));
    let time_key = ["timestamp", "time", "@timestamp"]
        .into_iter()
        .find(|key| object.contains_key(*key));
    let display = |v: &serde_json::Value| {
        v.as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| v.to_string())
    };
    let severity = level_key
        .and_then(|key| object[key].as_str())
        .and_then(|v| match v.to_ascii_lowercase().as_str() {
            "trace" => Some(Severity::Trace),
            "debug" => Some(Severity::Debug),
            "info" => Some(Severity::Info),
            "warn" | "warning" => Some(Severity::Warn),
            "error" => Some(Severity::Error),
            "fatal" => Some(Severity::Fatal),
            _ => None,
        });
    let mut event = StructuredEvent::new(Message::Literal(
        message_key
            .map(|key| display(&object[key]))
            .unwrap_or_default(),
    ))
    .context(EventContext {
        severity,
        timestamp: time_key.map(|key| display(&object[key])),
        ..Default::default()
    });
    for (key, value) in object {
        if Some(key.as_str()) == message_key
            || Some(key.as_str()) == time_key
            || (severity.is_some() && Some(key.as_str()) == level_key)
        {
            continue;
        }
        event = event.field(key, typed(value));
    }
    event
}
/// A record as the JSON object its fields make, in order.
pub(super) fn to_json(record: &rich_ext::log_stream::Record) -> serde_json::Value {
    serde_json::Value::Object(
        record
            .fields
            .iter()
            .map(|(key, value)| (key.clone(), json(value)))
            .collect(),
    )
}

/// A typed value as JSON: [`typed`] undone. A float that JSON cannot hold
/// (NaN, infinity) is `null`.
pub(super) fn json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(v) => serde_json::Value::Bool(*v),
        Value::Integer(v) => (*v).into(),
        Value::Unsigned(v) => (*v).into(),
        Value::Integer128(v) => i64::try_from(*v)
            .map(Into::into)
            .unwrap_or_else(|_| serde_json::Value::from(*v as f64)),
        Value::Unsigned128(v) => u64::try_from(*v)
            .map(Into::into)
            .unwrap_or_else(|_| serde_json::Value::from(*v as f64)),
        Value::Float(v) => serde_json::Number::from_f64(*v)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::String(v) => serde_json::Value::String(v.clone()),
        Value::List(v) => serde_json::Value::Array(v.iter().map(json).collect()),
        Value::Map(v) => serde_json::Value::Object(
            v.iter()
                .map(|(key, value)| (key.clone(), json(value)))
                .collect(),
        ),
    }
}

fn typed(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(v) => Value::Bool(*v),
        serde_json::Value::Number(v) => {
            if let Some(v) = v.as_i64() {
                Value::Integer(v)
            } else if let Some(v) = v.as_u64() {
                Value::Unsigned(v)
            } else {
                Value::Float(v.as_f64().unwrap_or_default())
            }
        }
        serde_json::Value::String(v) => Value::String(v.clone()),
        serde_json::Value::Array(v) => Value::List(v.iter().map(typed).collect()),
        serde_json::Value::Object(v) => {
            Value::Map(v.iter().map(|(k, v)| (k.clone(), typed(v))).collect())
        }
    }
}
