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
