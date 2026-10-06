//! The Arrow schema explorer (#342): Arrow schemas in the model, drawn as a
//! tree and compared.
#![cfg(feature = "arrow")]

use std::collections::HashMap;
use std::sync::Arc;

use arrow_schema::{DataType as Arrow, Field, Fields, Schema, TimeUnit};
use rich::{Console, Renderable};
use rich_data::DataType;

fn plain(width: usize, r: &dyn Renderable) -> String {
    Console::builder()
        .width(width)
        .color_system(None)
        .build()
        .render_to_string(r)
}

/// Events, with every kind of nesting and the types the model keeps
/// parameters for. `v2` widens the id, drops `debug`, adds `country` to the
/// address and changes the metadata.
fn events(v2: bool) -> Schema {
    let address = Fields::from(vec![
        Field::new("city", Arrow::Utf8, false),
        Field::new("zip", Arrow::Utf8, true),
    ]);
    let address = if v2 {
        let mut fields: Vec<Field> = address.iter().map(|f| f.as_ref().clone()).collect();
        fields.push(Field::new("country", Arrow::Utf8, true));
        Fields::from(fields)
    } else {
        address
    };
    let entries = Field::new(
        "entries",
        Arrow::Struct(Fields::from(vec![
            Field::new("key", Arrow::Utf8, false),
            Field::new("value", Arrow::Float64, true),
        ])),
        false,
    );
    let mut fields = vec![
        Field::new("id", if v2 { Arrow::Int64 } else { Arrow::Int32 }, false),
        Field::new(
            "at",
            Arrow::Timestamp(TimeUnit::Microsecond, Some("Europe/Paris".into())),
            false,
        ),
        Field::new(
            "kind",
            Arrow::Dictionary(Box::new(Arrow::Int8), Box::new(Arrow::Utf8)),
            true,
        )
        .with_metadata(HashMap::from([(
            "description".to_string(),
            if v2 { "event kind" } else { "kind" }.to_string(),
        )])),
        Field::new("amount", Arrow::Decimal128(12, 2), true),
        Field::new("tags", Arrow::new_list(Arrow::Utf8, true), true),
        Field::new("address", Arrow::Struct(address), true),
        Field::new("scores", Arrow::Map(Arc::new(entries), false), true),
    ];
    if !v2 {
        fields.push(Field::new("debug", Arrow::Boolean, true));
    }
    Schema::new(fields).with_metadata(HashMap::from([(
        "version".to_string(),
        if v2 { "2" } else { "1" }.to_string(),
    )]))
}

#[test]
fn arrow_types_map_into_the_model() {
    let model = rich_data::arrow::schema(&events(false));
    let types: Vec<(&str, String, &str, bool)> = model
        .fields()
        .iter()
        .map(|f| {
            (
                f.name(),
                f.data_type().to_string(),
                f.native_type().unwrap(),
                f.is_required(),
            )
        })
        .collect();
    assert_eq!(
        types,
        [
            ("id", "integer".into(), "Int32", true),
            (
                "at",
                "timestamp[Europe/Paris]".into(),
                "Timestamp(µs, \"Europe/Paris\")",
                true
            ),
            ("kind", "string".into(), "Dictionary(Int8, Utf8)", false),
            ("amount", "decimal(12,2)".into(), "Decimal128(12, 2)", false),
            ("tags", "list<string>".into(), "List", false),
            ("address", "struct".into(), "Struct", false),
            ("scores", "map<string, float>".into(), "Map", false),
            ("debug", "boolean".into(), "Boolean", false),
        ]
    );
    assert_eq!(model.metadata(), [("version".into(), "1".into())]);
    let kind = model.field("kind").unwrap();
    assert_eq!(kind.metadata(), [("description".into(), "kind".into())]);
    let DataType::Map { key, value } = model.field("scores").unwrap().data_type() else {
        panic!("not a map");
    };
    assert!(key.is_required() && !value.is_required());
}

#[test]
fn the_explorer_draws_a_tree() {
    let tree = rich_data::arrow::tree(&events(false)).title("events");
    assert_eq!(
        plain(80, &tree),
        concat!(
            "events  8 fields  version=1\n",
            "├── id (required)  Int32\n",
            "├── at (required)  Timestamp(µs, \"Europe/Paris\")\n",
            "├── kind  Dictionary(Int8, Utf8)  description=kind\n",
            "├── amount  Decimal128(12, 2)\n",
            "├── tags  List\n",
            "│   └── item  Utf8\n",
            "├── address  Struct\n",
            "│   ├── city (required)  Utf8\n",
            "│   └── zip  Utf8\n",
            "├── scores  Map\n",
            "│   ├── key (required)  Utf8\n",
            "│   └── value  Float64\n",
            "└── debug  Boolean",
        )
    );
}

#[test]
fn the_explorer_draws_a_diff() {
    let diff = rich_data::arrow::diff(&events(false), &events(true)).names("v1", "v2");
    assert_eq!(
        plain(90, &diff),
        concat!(
            "┏━━━┳━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━┓\n",
            "┃   ┃ Where           ┃ Change                                 ┃          ┃\n",
            "┡━━━╇━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━┩\n",
            "│ ~ │ (root)          │ metadata version 1 → 2                 │          │\n",
            "│ - │ debug           │ field removed (Boolean)                │ breaking │\n",
            "│ ~ │ id              │ type Int32 → Int64                     │ breaking │\n",
            "│ ~ │ kind            │ metadata description kind → event kind │          │\n",
            "│ + │ address.country │ field added (Utf8)                     │          │\n",
            "└───┴─────────────────┴────────────────────────────────────────┴──────────┘\n",
            "v1 → v2: 5 changes, 2 breaking                                             ",
        )
    );
    assert!(rich_data::arrow::diff(&events(true), &events(true)).is_empty());
}

fn ipc_file(schema: &Schema) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut writer = arrow_ipc::writer::FileWriter::try_new(&mut bytes, schema).unwrap();
    writer.finish().unwrap();
    drop(writer);
    bytes
}

fn ipc_stream(schema: &Schema) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut writer = arrow_ipc::writer::StreamWriter::try_new(&mut bytes, schema).unwrap();
    writer.finish().unwrap();
    drop(writer);
    bytes
}

#[test]
fn an_ipc_file_or_stream_gives_its_schema() {
    use std::io::Cursor;
    let schema = events(false);
    for bytes in [ipc_file(&schema), ipc_stream(&schema)] {
        let read = rich_data::arrow::read_schema(Cursor::new(bytes)).unwrap();
        assert_eq!(
            rich_data::arrow::schema(&read),
            rich_data::arrow::schema(&schema)
        );
    }
}

#[test]
fn anything_else_is_an_error_not_a_panic() {
    use std::io::Cursor;
    let err = rich_data::arrow::read_schema(Cursor::new(Vec::new())).unwrap_err();
    assert_eq!(
        err.to_string(),
        "empty input: not an Arrow IPC file or stream"
    );
    let mut truncated = ipc_file(&events(false));
    truncated.truncate(truncated.len() / 2);
    for bytes in [b"id,name\n1,ada\n".to_vec(), b"ARROW1".to_vec(), truncated] {
        let err = rich_data::arrow::read_schema(Cursor::new(bytes)).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("not an Arrow IPC file or stream: "),
            "{err}"
        );
    }
}
