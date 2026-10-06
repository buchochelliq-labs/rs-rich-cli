//! Profiles and data quality (0.0.16 workstream 3, #343-#346, #269).
//! Snapshots live in `tests/snapshots`; set `UPDATE_SNAPSHOTS=1` to rewrite
//! them after a deliberate change, then review the diff.

use rich::cells::cell_len;
use rich::Console;
use rich_data::csv::CsvReader;
use rich_data::profile::{Distribution, Profile, ProfileOptions};
use rich_data::quality::{self, QualityReport, Status};
use rich_data::{jsonl, RowSource};

const ORDERS: &str = include_str!("fixtures/orders.csv");
const EVENTS: &str = include_str!("fixtures/events.jsonl");

fn render(renderable: &dyn rich::Renderable, width: usize) -> String {
    let console = Console::builder().width(width).color_system(None).build();
    console.render_export(renderable)
}

fn check(name: &str, actual: &str, width: usize) {
    for line in actual.lines() {
        assert!(
            cell_len(line) <= width,
            "{name}: wider than {width}: {line:?}"
        );
    }
    let path = format!("{}/tests/snapshots/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(format!("{}/tests/snapshots", env!("CARGO_MANIFEST_DIR"))).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing snapshot {path}; run with UPDATE_SNAPSHOTS=1"));
    assert_eq!(actual, expected, "{name} changed:\n{actual}");
}

fn orders() -> Profile {
    let source = CsvReader::new().source(ORDERS.as_bytes()).unwrap();
    Profile::from_source(source, ProfileOptions::default())
        .unwrap()
        .with_name("orders.csv")
}

fn events() -> Profile {
    let source = jsonl::source(EVENTS.as_bytes()).unwrap();
    Profile::from_source(source, ProfileOptions::default())
        .unwrap()
        .with_name("events.jsonl")
}

#[test]
fn the_csv_fixture_profiles_as_its_snapshot() {
    let profile = orders();
    assert_eq!((profile.rows(), profile.sampled()), (24, false));
    let types: Vec<&str> = profile
        .columns()
        .iter()
        .map(|c| c.data_type.as_str())
        .collect();
    assert_eq!(
        types,
        ["integer", "text", "float", "text", "date", "boolean"]
    );
    let amount = profile.column("amount").unwrap();
    assert_eq!(amount.nulls, 4);
    assert!(matches!(
        amount.distribution,
        Distribution::Histogram { .. }
    ));
    // `NA` is a null token.
    assert_eq!(profile.column("region").unwrap().nulls, 2);
    check("profile_orders", &render(&profile, 80), 80);
    check("profile_orders_narrow", &render(&profile, 50), 50);
}

#[test]
fn the_jsonl_fixture_profiles_as_its_snapshot() {
    let profile = events();
    assert_eq!(profile.rows(), 12);
    let columns: Vec<&str> = profile.columns().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(columns, ["ts", "level", "service", "latency_ms", "user"]);
    assert_eq!(profile.column("ts").unwrap().data_type, "timestamp");
    assert_eq!(profile.column("user").unwrap().nulls, 6);
    check("profile_events", &render(&profile, 80), 80);
}

#[test]
fn the_json_report_is_the_model() {
    let json = orders().to_json();
    let path = format!(
        "{}/tests/snapshots/profile_orders.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let actual = serde_json::to_string_pretty(&json).unwrap() + "\n";
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &actual).unwrap();
    } else {
        let expected = std::fs::read_to_string(&path).unwrap();
        assert_eq!(actual, expected);
    }
    assert_eq!(json["rows"], 24);
    assert_eq!(json["sampled"], false);
    assert_eq!(json["columns"][1]["distribution"]["kind"], "top");
    let back: Profile = serde_json::from_value(json).unwrap();
    assert_eq!(back, orders());
}

#[test]
fn a_large_input_is_sampled_and_says_so() {
    // 200,000 rows streamed through the CSV source: memory holds the sample.
    let mut text = String::from("id,group,value\n");
    for i in 0..200_000u32 {
        let value = if i % 7 == 0 {
            String::new()
        } else {
            (i % 1000).to_string()
        };
        text.push_str(&format!("{i},g{},{value}\n", i % 4));
    }
    let options = ProfileOptions {
        sample: 1_000,
        ..Default::default()
    };
    let source = CsvReader::new().source(text.as_bytes()).unwrap();
    let profile = Profile::from_source(source, options).unwrap();
    assert_eq!(profile.rows(), 200_000);
    assert_eq!(profile.sample_size(), 1_000);
    assert!(profile.sampled());
    // Nulls count every row, not the sample.
    assert_eq!(profile.column("value").unwrap().nulls, 28_572);
    let out = render(&profile, 80);
    assert!(
        out.starts_with("sampled 1,000 of 200,000 rows, 3 columns\n"),
        "{out}"
    );
    assert!(out.contains("of a uniform sample"), "{out}");
    let json = profile.to_json();
    assert_eq!(
        (json["sample"].clone(), json["sampled"].clone()),
        (1_000.into(), true.into())
    );
    // The map stays at its bucket limit.
    assert_eq!(profile.missing().buckets.len(), 13);
    assert_eq!(profile.missing().bucket_rows, 16_384);
}

/// Every line of a profile fits a narrow width: the summary table kept its
/// name column at least as wide as the names (up to 16), so at 40 to 50
/// columns it drew wider than the width it was given.
#[test]
fn a_narrow_profile_fits_its_width() {
    let long = "a_long_column_name,b\nx,1\n,2\ny,3\n";
    let narrow = "1e999null中\nx\n";
    // Measured to fit at 80, but drawn two columns wider.
    let measured = "é|true11inf\u{1b}[31m\r\nnan \"a\":1]{";
    for (name, profile) in [
        ("orders", orders()),
        ("events", events()),
        (
            "long",
            Profile::from_source(
                CsvReader::new().source(long.as_bytes()).unwrap(),
                ProfileOptions::default(),
            )
            .unwrap(),
        ),
        (
            "narrow",
            Profile::from_source(
                CsvReader::new()
                    .fallback(',')
                    .source(narrow.as_bytes())
                    .unwrap(),
                ProfileOptions::default(),
            )
            .unwrap(),
        ),
        (
            "measured",
            Profile::from_source(
                CsvReader::new()
                    .fallback(',')
                    .source(measured.as_bytes())
                    .unwrap(),
                ProfileOptions::default(),
            )
            .unwrap(),
        ),
    ] {
        for width in [40, 44, 50, 80] {
            let console = Console::builder().width(width).color_system(None).build();
            let segments = console.render(&profile, None);
            for line in rich::Segment::split_lines(&segments) {
                let cells: usize = line.iter().map(rich::Segment::cell_length).sum();
                let text: String = line.iter().map(|s| s.text.to_string()).collect();
                assert!(cells <= width, "{name} at {width}: {text:?}");
            }
        }
    }
}

#[test]
fn odd_csv_never_panics() {
    for text in [
        "",
        "\n\n",
        "a\n",
        "a,b\n1\n1,2,3,4\n",
        "a,b\n\"open,1\n",
        "a;b\n\"x\"\"y\";2\n\u{feff}\n",
        "\u{feff}a,b\r\n1,2\r3,4",
        "a,b\n,\n,\n",
    ] {
        let Ok(source) = CsvReader::new().fallback(',').source(text.as_bytes()) else {
            continue;
        };
        if let Ok(profile) = Profile::from_source(source, ProfileOptions::default()) {
            render(&profile, 40);
            render(&profile, 1);
        }
    }
    // A short row is padded; a long one after the sniffer's sample is cut
    // and counted (within the sample it adds columns, as `read` does).
    let text = format!("a,b\n1\n{}1,2,3,4\n", "5,6\n".repeat(300));
    let mut source = CsvReader::new()
        .header(true)
        .source(text.as_bytes())
        .unwrap();
    assert_eq!(source.columns(), ["a", "b"]);
    let first = source.next_row().unwrap().unwrap();
    assert_eq!(first[1], rich_data::Value::Null);
    while source.next_row().is_some() {}
    assert_eq!(source.longer_rows(), 1);
}

#[test]
fn a_header_unless_the_first_row_is_numbers() {
    use rich_data::csv::Header;
    let reader = CsvReader::new().header_mode(Header::UnlessNumeric);
    // The sniffer calls this headerless: one empty cell makes `p99` ragged.
    let text = "service,p99\nweb,120\napi,35\ndb,\n";
    assert_eq!(CsvReader::new().read(text).unwrap().columns(), ["1", "2"]);
    assert_eq!(reader.read(text).unwrap().columns(), ["service", "p99"]);
    let numbers = reader.read("1,2.5\n3,\n").unwrap();
    assert_eq!(
        (numbers.columns(), numbers.len()),
        (&["1".to_string(), "2".to_string()][..], 2)
    );
}

#[test]
fn a_csv_source_reads_like_the_whole_reader() {
    for text in [
        ORDERS,
        "name;note\n\"a;b\";\"two\nlines\"\nc;d\n",
        "x\ty\n1\t2\n\n3\t4",
        "1,2\n3,4\n",
    ] {
        let whole = CsvReader::new().read(text).unwrap();
        let streamed = CsvReader::new()
            .source(text.as_bytes())
            .unwrap()
            .collect_rows()
            .unwrap();
        assert_eq!(streamed.columns(), whole.columns(), "{text:?}");
        assert_eq!(streamed.rows(), whole.rows(), "{text:?}");
    }
}

#[test]
fn quality_checks_render_like_a_test_report() {
    let rows = CsvReader::new().read(ORDERS).unwrap();
    let report = QualityReport::new([
        quality::unique(&rows, "order_id").unwrap(),
        quality::not_null(&rows, "amount").unwrap(),
        quality::CheckResult::new("accepted_values", Status::Warn)
            .column("status")
            .observed("returned (2)")
            .expected("shipped, pending, cancelled"),
    ]);
    assert!(!report.is_success());
    check("quality_orders", &render(&report, 80), 80);
}
