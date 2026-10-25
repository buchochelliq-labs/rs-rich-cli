//! The value specs of `scripts/parity_docs.py` (`python_value`), read back
//! into [`PrettyValue`]s: plain JSON is what `json.loads` makes, and an object
//! with a `"$"` key is a tagged Python value. Shared by the docs golden test
//! and the `diff_render` example.

use rich::pretty::{ObjectRepr, PrettyObject, PrettyValue};
use serde_json::Value;

/// The [`PrettyValue`] a value spec describes.
pub fn pretty_value(spec: &Value) -> PrettyValue {
    let Some(tag) = spec.get("$").and_then(Value::as_str) else {
        return match spec {
            Value::Array(items) => PrettyValue::List(items.iter().map(pretty_value).collect()),
            Value::Object(map) => PrettyValue::Dict(
                map.iter()
                    .map(|(k, v)| (PrettyValue::Str(k.clone()), pretty_value(v)))
                    .collect(),
            ),
            other => PrettyValue::from(other),
        };
    };
    let items = || -> Vec<PrettyValue> {
        spec["items"]
            .as_array()
            .expect("items")
            .iter()
            .map(pretty_value)
            .collect()
    };
    let text = || spec["value"].as_str().expect("value").to_string();
    match tag {
        "tuple" => PrettyValue::Tuple(items()),
        "list" => PrettyValue::List(items()),
        "set" => PrettyValue::Set(items()),
        "frozenset" => PrettyValue::FrozenSet(items()),
        "dict" => PrettyValue::Dict(
            spec["items"]
                .as_array()
                .expect("items")
                .iter()
                .map(|pair| (pretty_value(&pair[0]), pretty_value(&pair[1])))
                .collect(),
        ),
        "bytes" => {
            let hex = spec["hex"].as_str().expect("hex");
            PrettyValue::Bytes(
                (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex byte"))
                    .collect(),
            )
        }
        "float" => PrettyValue::Float(text().parse().expect("float")),
        "int" => PrettyValue::Repr(text()),
        "repr" => PrettyValue::Repr(text()),
        "obj" => {
            let kind = match spec["kind"].as_str().expect("kind") {
                "dataclass" => ObjectRepr::Dataclass,
                "namedtuple" => ObjectRepr::NamedTuple,
                "angular" => ObjectRepr::RichRepr { angular: true },
                _ => ObjectRepr::RichRepr { angular: false },
            };
            let fields = spec["fields"]
                .as_array()
                .expect("fields")
                .iter()
                .map(|field| {
                    (
                        field[0].as_str().map(str::to_string),
                        pretty_value(&field[1]),
                    )
                })
                .collect();
            PrettyValue::Object(PrettyObject::new(
                spec["name"].as_str().expect("name"),
                kind,
                fields,
            ))
        }
        other => panic!("unknown value tag {other:?}"),
    }
}
