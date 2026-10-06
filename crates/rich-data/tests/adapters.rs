//! Every adapter reads the same services fixture to the same rows (0.0.16
//! workstream 1's acceptance), and the pieces compose: inference, tables,
//! conditional styles and statistics.

use rich::{Console, Style};
use rich_data::csv::CsvReader;
use rich_data::infer::{InferredType, Inferrer};
use rich_data::stats::Stats;
use rich_data::{jsonl, serialize, RowSource, Rows, Value};
use rich_ext::table::{Comparison, StyleRule, StyleRules, Target};

const CSV: &str = include_str!("fixtures/services.csv");
const TSV: &str = include_str!("fixtures/services.tsv");
const JSONL: &str = include_str!("fixtures/services.jsonl");

const COLUMNS: [&str; 5] = ["service", "replicas", "p99", "healthy", "deployed"];

fn expected() -> Vec<Vec<Value>> {
    vec![
        vec![
            "web".into(),
            Value::Int(3),
            Value::Float(120.5),
            "true".into(),
            "2026-10-01".into(),
        ],
        vec![
            "api".into(),
            Value::Int(2),
            Value::Float(35.0),
            "false".into(),
            "2026-10-02".into(),
        ],
        vec![
            "db".into(),
            Value::Int(1),
            Value::Null,
            "true".into(),
            "2026-09-30".into(),
        ],
    ]
}

/// Text adapters keep cells as written; inference types them.
fn typed(mut rows: Rows) -> Rows {
    Inferrer::new().infer(&rows).apply(&mut rows);
    rows
}

#[test]
fn csv_and_tsv_read_the_fixture_once_typed() {
    for rows in [
        CsvReader::new().read(CSV).unwrap(),
        CsvReader::new().read(TSV).unwrap(),
        CsvReader::tsv().read(TSV).unwrap(),
    ] {
        assert_eq!(rows.columns(), COLUMNS);
        // Untyped until asked.
        assert_eq!(rows.rows()[0][1], Value::from("3"));
        let rows = typed(rows);
        assert_eq!(rows.rows(), expected());
    }
}

#[test]
fn inference_reports_each_column_with_evidence() {
    let rows = CsvReader::new().read(CSV).unwrap();
    let inference = Inferrer::new().infer(&rows);
    let types: Vec<InferredType> = inference.columns().iter().map(|c| c.data_type()).collect();
    assert_eq!(
        types,
        [
            InferredType::Text,
            InferredType::Integer,
            InferredType::Float,
            InferredType::Boolean,
            InferredType::Date,
        ]
    );
    let summaries: Vec<String> = inference.columns().iter().map(|c| c.summary()).collect();
    assert_eq!(
        summaries,
        [
            "text: 3 values",
            "integer: 3 of 3 values",
            "float: 2 of 2 values, 1 null",
            "boolean: 3 of 3 values",
            "date: 3 of 3 values",
        ]
    );
}

#[test]
fn jsonl_reads_the_fixture_whole_and_streaming() {
    let rows = jsonl::read(JSONL).unwrap();
    assert_eq!(rows.columns(), COLUMNS);
    assert_eq!(rows.rows(), expected());
    let streamed = jsonl::source(JSONL.as_bytes())
        .unwrap()
        .collect_rows()
        .unwrap();
    assert_eq!(streamed.rows(), expected());
    let types: Vec<String> = rows
        .schema()
        .unwrap()
        .fields()
        .iter()
        .map(|f| f.data_type().to_string())
        .collect();
    assert_eq!(types, ["string", "integer", "float", "boolean", "string"]);
}

#[derive(serde::Serialize)]
struct Service {
    service: &'static str,
    replicas: u32,
    p99: Option<f64>,
    healthy: bool,
    deployed: &'static str,
}

#[test]
fn serde_reads_the_same_rows() {
    let services = [
        Service {
            service: "web",
            replicas: 3,
            p99: Some(120.5),
            healthy: true,
            deployed: "2026-10-01",
        },
        Service {
            service: "api",
            replicas: 2,
            p99: Some(35.0),
            healthy: false,
            deployed: "2026-10-02",
        },
        Service {
            service: "db",
            replicas: 1,
            p99: None,
            healthy: true,
            deployed: "2026-09-30",
        },
    ];
    let rows = serialize::read(&services).unwrap();
    assert_eq!(rows.columns(), COLUMNS);
    assert_eq!(rows.rows(), expected());
}

#[cfg(feature = "arrow")]
#[test]
fn arrow_reads_the_same_rows() {
    use std::sync::Arc;

    use arrow_array::{ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch, StringArray};

    let batch = RecordBatch::try_from_iter([
        (
            "service",
            Arc::new(StringArray::from(vec!["web", "api", "db"])) as ArrayRef,
        ),
        (
            "replicas",
            Arc::new(Int64Array::from(vec![3, 2, 1])) as ArrayRef,
        ),
        (
            "p99",
            Arc::new(Float64Array::from(vec![Some(120.5), Some(35.0), None])) as ArrayRef,
        ),
        (
            "healthy",
            Arc::new(BooleanArray::from(vec![true, false, true])) as ArrayRef,
        ),
        (
            "deployed",
            Arc::new(StringArray::from(vec![
                "2026-10-01",
                "2026-10-02",
                "2026-09-30",
            ])) as ArrayRef,
        ),
    ])
    .unwrap();
    let rows = rich_data::arrow::rows(&batch);
    assert_eq!(rows.columns(), COLUMNS);
    assert_eq!(rows.rows(), expected());
    let streamed = rich_data::arrow::BatchSource::new([batch.clone(), batch])
        .unwrap()
        .collect_rows()
        .unwrap();
    assert_eq!(streamed.len(), 6);
}

#[test]
fn typed_rows_render_with_rules_and_statistics() {
    let rows = typed(CsvReader::new().read(CSV).unwrap());
    let rules = StyleRules::new()
        .rule(StyleRule::new(
            "p99",
            Comparison::Gt,
            100,
            Style::parse("bold red").unwrap(),
        ))
        .rule(
            StyleRule::new(
                "healthy",
                Comparison::Eq,
                "false",
                Style::parse("dim").unwrap(),
            )
            .target(Target::Row),
        );
    let table = rows.to_table_data().style_rules(rules);
    let console = Console::builder().width(60).build();
    let out = console.render_export(&table);
    assert_eq!(
        out,
        "\
┏━━━━━━━━━┳━━━━━━━━━━┳━━━━━━━┳━━━━━━━━━┳━━━━━━━━━━━━┓
┃ service ┃ replicas ┃   p99 ┃ healthy ┃ deployed   ┃
┡━━━━━━━━━╇━━━━━━━━━━╇━━━━━━━╇━━━━━━━━━╇━━━━━━━━━━━━┩
│ web     │        3 │ 120.5 │ true    │ 2026-10-01 │
│ api     │        2 │    35 │ false   │ 2026-10-02 │
│ db      │        1 │       │ true    │ 2026-09-30 │
└─────────┴──────────┴───────┴─────────┴────────────┘
"
    );
    let colored = Console::builder()
        .width(60)
        .force_terminal(true)
        .color_system(Some(rich::ColorSystem::Truecolor))
        .build()
        .render_export(&table);
    assert!(colored.contains("\x1b[1;31m120.5\x1b[0m"), "{colored:?}");
    assert!(colored.contains("\x1b[2m"), "{colored:?}");

    let stats = Stats::of(&rows);
    let p99 = &stats.columns()[2];
    assert_eq!((p99.count, p99.nulls), (2, 1));
    assert_eq!(p99.mean, Some(77.75));
    assert_eq!(stats.columns()[3].top[0], ("true".to_string(), 2));
    let wide = Console::builder().width(140).build();
    let out = wide.render_export(&stats);
    let p99_line = out.lines().find(|l| l.starts_with("│ p99")).unwrap();
    assert!(p99_line.contains("│ 77.75 │"), "{out}");
}

#[test]
fn the_readme_example_runs() {
    let mut rows = CsvReader::new()
        .read("service,p99,up\nweb,120,true\napi,35.5,false\ndb,8,true\n")
        .unwrap();
    let inference = Inferrer::new().infer(&rows);
    inference.apply(&mut rows);
    let console = Console::builder().width(80).build();
    let report = console.render_export(&inference);
    assert!(
        report.contains("│ p99     │ float   │    3/3 │     0 │"),
        "{report}"
    );
    assert!(console
        .render_export(&rows.to_table_data())
        .contains("│ api     │ 35.5 │ false │"));
    assert!(console.render_export(&Stats::of(&rows)).contains("│ p99"));
}
