//! Guide: Tabular data — run: cargo run -p rs-rich-data --example guide_tabular [-- --svg docs/media/guide]
//!
//! The snippets and pictures on `docs/guide/data/index.md` come from this
//! file.

use std::error::Error;
use std::path::PathBuf;

use rich::ColorSystem;

// --8<-- [start:imports]
use rich::{Console, Style};
use rich_data::csv::CsvReader;
use rich_data::infer::Inferrer;
use rich_data::profile::{Profile, ProfileOptions};
use rich_data::quality::{self, CheckResult, QualityReport, Status};
use rich_data::stats::{Stats, StatsOptions};
use rich_data::Rows;
use rich_ext::table::{Comparison, StyleRule, StyleRules, Target};
// --8<-- [end:imports]

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

/// A service dashboard's export: every cell is text until inferred.
const SERVICES: &str = "\
service,region,p99,errors,up,deployed
web,eu-west,120,3,true,2026-09-30
api,eu-west,35.5,0,true,2026-10-01
db,us-east,8,0,true,2026-08-14
billing,us-east,240,17,false,2026-10-02
search,ap-south,88.2,1,true,2026-09-21
auth,eu-west,12,,true,2026-09-30
";

/// A month of orders, with three missing emails, a missing total and a
/// repeated id.
const ORDERS: &str = "\
id,email,country,total,status,placed
1001,ada@example.com,SE,141.42,paid,2026-09-01
1002,grace@example.com,FR,276.19,paid,2026-09-02
1003,linus@example.com,US,158.66,paid,2026-09-03
1004,margaret@example.com,SE,23.45,shipped,2026-09-04
1005,alan@example.com,SE,36.78,paid,2026-09-05
1006,,FR,32.35,shipped,2026-09-06
1007,ken@example.com,DE,398.35,refunded,2026-09-07
1008,dennis@example.com,DE,245.77,shipped,2026-09-08
1009,frances@example.com,GB,410.22,paid,2026-09-09
1010,edsger@example.com,FR,,paid,2026-09-10
1011,john@example.com,GB,243.22,shipped,2026-09-11
1012,radia@example.com,US,50.46,shipped,2026-09-12
1013,tim@example.com,FR,161.43,shipped,2026-09-13
1014,,US,240.52,shipped,2026-09-14
1015,katherine@example.com,SE,212.52,shipped,2026-09-15
1016,donald@example.com,ES,328.22,shipped,2026-09-16
1017,niklaus@example.com,GB,156.97,paid,2026-09-17
1016,bjarne@example.com,FR,295.99,paid,2026-09-18
1019,guido@example.com,ES,244.66,shipped,2026-09-19
1020,yukihiro@example.com,ES,368.56,refunded,2026-09-20
1021,,FR,126.63,paid,2026-09-21
1022,anders@example.com,NL,218.92,paid,2026-09-22
1023,brendan@example.com,SE,70.62,shipped,2026-09-23
1024,rob@example.com,FR,24.15,refunded,2026-09-24
";

// --8<-- [start:read]
fn read(console: &Console) -> Result {
    let rows = CsvReader::new().read(SERVICES)?;
    assert_eq!(rows.columns()[..3], ["service", "region", "p99"]);
    console.print(&rows.to_table_data());
    Ok(())
}
// --8<-- [end:read]

// --8<-- [start:infer]
fn infer(console: &Console) -> Result {
    let mut rows = CsvReader::new().read(SERVICES)?;
    let inference = Inferrer::new().infer(&rows);
    assert_eq!(inference.columns()[2].data_type().to_string(), "float");
    assert_eq!(inference.columns()[2].summary(), "float: 6 of 6 values");
    console.print(&inference);
    // Numbers become numbers, so the table aligns them right.
    inference.apply(&mut rows);
    console.print(&rows.to_table_data());
    Ok(())
}
// --8<-- [end:infer]

/// The services, with their types inferred.
fn services() -> Result<Rows> {
    let mut rows = CsvReader::new().read(SERVICES)?;
    Inferrer::new().infer(&rows).apply(&mut rows);
    Ok(rows)
}

// --8<-- [start:stats]
fn stats(console: &Console) -> Result {
    let rows = services()?;
    let options = StatsOptions {
        quantiles: vec![0.9],
        top: 1,
    };
    console.print(&Stats::with_options(&rows, &options));
    Ok(())
}
// --8<-- [end:stats]

// --8<-- [start:rules]
fn rules(console: &Console) -> Result {
    let rows = services()?;
    let rules = StyleRules::new()
        .rule(StyleRule::new(
            "p99",
            Comparison::Gt,
            100,
            Style::parse("bold red")?,
        ))
        .rule(StyleRule::new(
            "errors",
            Comparison::Eq,
            0,
            Style::parse("green")?,
        ))
        .rule(
            StyleRule::when("up", |v| v.plain() == "false", Style::parse("reverse")?)
                .target(Target::Row),
        );
    console.print(&rows.to_table_data().style_rules(rules));
    Ok(())
}
// --8<-- [end:rules]

// --8<-- [start:profile]
fn profile(console: &Console) -> Result {
    // Any `BufRead` streams: a file through `BufReader`, here a string.
    let source = CsvReader::new().source(ORDERS.as_bytes())?;
    let options = ProfileOptions {
        sample: 10_000,
        top: 3,
        bins: 6,
        columns: Some(vec!["email".into(), "total".into(), "status".into()]),
        ..Default::default()
    };
    let profile = Profile::from_source(source, options)?.with_name("orders.csv");
    console.print(&profile);
    Ok(())
}
// --8<-- [end:profile]

// --8<-- [start:quality]
fn quality(console: &Console) -> Result {
    let rows = CsvReader::new().read(ORDERS)?;
    let report = QualityReport::new([
        quality::not_null(&rows, "email")?,
        quality::unique(&rows, "id")?,
        quality::not_null(&rows, "status")?,
        CheckResult::new("row_count", Status::Warn)
            .observed(rows.len().to_string())
            .expected(">= 100")
            .message("fewer orders than a usual month"),
    ]);
    console.print(&report);
    Ok(())
}
// --8<-- [end:quality]

fn main() {
    let shots = Shots::from_args("guide_tabular");
    shots.shot("read", 72, read);
    shots.shot("infer", 72, infer);
    shots.shot("stats", 100, stats);
    shots.shot("rules", 72, rules);
    shots.shot("profile", 80, profile);
    shots.shot("quality", 88, quality);
}

/// `--svg DIR` writes each shot as `DIR/<topic>-<shot>.svg` from a pinned
/// console; without it, shots print to the terminal.
struct Shots {
    topic: &'static str,
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args(topic: &'static str) -> Self {
        let args: Vec<String> = std::env::args().collect();
        let dir = args
            .iter()
            .position(|a| a == "--svg")
            .map(|i| PathBuf::from(args.get(i + 1).expect("--svg takes a directory")));
        Shots { topic, dir }
    }

    fn shot(&self, name: &str, width: usize, f: impl FnOnce(&Console) -> Result) {
        let Some(dir) = &self.dir else {
            let console = Console::builder().theme(rich_ext::extended_theme()).build();
            return f(&console).expect("the shot renders");
        };
        let console = Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .theme(rich_ext::extended_theme())
            .build();
        let id = format!("{}-{name}", self.topic);
        let svg = console.export_svg(&id, &id, |console| {
            f(console).expect("the shot renders");
        });
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        let path = dir.join(format!("{id}.svg"));
        std::fs::write(&path, svg).expect("write the SVG");
        eprintln!("wrote {}", path.display());
    }
}
