//! Streaming tables (#259) and table sorting and grouping (#429).
use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Console, Justify, Table, Text, Theme};
use rich_ext::a11y::AccessibleText;
use rich_ext::live::LiveCoordinator;
use rich_ext::table::sort::{compare_rows, sort_rows, sorted_indices};
use rich_ext::table::{
    Aggregate, Column, GroupBy, RenderStats, SortKey, StreamingTable, TableData, Value, Window,
};
use rich_ext::target::{RenderTarget, TargetKind};

fn plain(width: usize) -> Console {
    Console::builder().width(width).build()
}

fn colour(width: usize) -> Console {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build()
}

fn jobs() -> StreamingTable<&'static str> {
    let mut table = StreamingTable::new([
        Column::new("job"),
        Column::new("state"),
        Column::new("done").justify(Justify::Right),
    ]);
    table.upsert("build", ["build".into(), "running".into(), Value::Int(40)]);
    table.upsert("test", ["test".into(), "queued".into(), Value::Null]);
    table.upsert("lint", ["lint".into(), "done".into(), Value::Int(100)]);
    table
}

fn stats(frames: u64, rows_prepared: u64, rows_rendered: u64, relayouts: u64) -> RenderStats {
    RenderStats {
        frames,
        rows_prepared,
        rows_rendered,
        relayouts,
    }
}

// ---------------------------------------------------------------------------
// Streaming (#259)
// ---------------------------------------------------------------------------

#[test]
fn streaming_renders_rows_in_insertion_order() {
    assert_eq!(
        plain(40).render_export(&jobs()),
        "\
┏━━━━━━━┳━━━━━━━━━┳━━━━━━┓
┃ job   ┃ state   ┃ done ┃
┡━━━━━━━╇━━━━━━━━━╇━━━━━━┩
│ build │ running │   40 │
│ test  │ queued  │      │
│ lint  │ done    │  100 │
└───────┴─────────┴──────┘
"
    );
}

#[test]
fn only_changed_rows_are_rendered_again() {
    let console = plain(40);
    let mut table = jobs();
    console.render_export(&table);
    assert_eq!(table.stats(), stats(1, 3, 3, 1));

    // Nothing changed: every row comes from the cache.
    console.render_export(&table);
    assert_eq!(table.stats(), stats(2, 3, 3, 1));

    // One cell of one row, no wider than before: that row alone.
    assert!(table.update_cell(&"build", 2, Value::Int(80)));
    let out = console.render_export(&table);
    assert_eq!(table.stats(), stats(3, 4, 4, 1));
    assert!(out.contains("│ build │ running │   80 │"), "{out}");

    // Writing the same value is not a change.
    assert!(table.update_cell(&"build", 2, Value::Int(80)));
    assert!(!table.upsert("test", ["test".into(), "queued".into(), Value::Null]));
    console.render_export(&table);
    assert_eq!(table.stats(), stats(4, 4, 4, 1));

    // A new row that fits the widths renders alone.
    assert!(table.upsert("docs", ["docs".into(), "queued".into(), Value::Null]));
    console.render_export(&table);
    assert_eq!(table.stats(), stats(5, 5, 5, 1));

    // A row that widens a column changes every row's layout.
    table.update_cell(&"test", 1, "waiting for runner");
    let out = console.render_export(&table);
    assert_eq!(table.stats(), stats(6, 6, 9, 2));
    assert_eq!(
        out,
        "\
┏━━━━━━━┳━━━━━━━━━━━━━━━━━━━━┳━━━━━━┓
┃ job   ┃ state              ┃ done ┃
┡━━━━━━━╇━━━━━━━━━━━━━━━━━━━━╇━━━━━━┩
│ build │ running            │   80 │
│ test  │ waiting for runner │      │
│ lint  │ done               │  100 │
│ docs  │ queued             │      │
└───────┴────────────────────┴──────┘
"
    );

    // Removing the widest row narrows the column again.
    assert!(table.remove(&"test").is_some());
    let out = console.render_export(&table);
    assert_eq!(table.stats(), stats(7, 6, 12, 3));
    assert!(out.starts_with("┏━━━━━━━┳━━━━━━━━━┳━━━━━━┓\n"), "{out}");

    // A different width is a relayout too.
    colour(30).render_export(&table);
    assert_eq!(table.stats().relayouts, 4);
    table.reset_stats();
    assert_eq!(table.stats(), RenderStats::default());
}

#[test]
fn streaming_output_matches_a_core_table_built_from_scratch() {
    // A deterministic mix of inserts, updates and removals, checked after
    // every step against the core table of the same rows, at widths where
    // columns fit and where they must wrap.
    let messages = [
        "ok",
        "connection reset by peer while reading the response body",
        "retrying",
        "",
        "timed out after 30s",
    ];
    let builders: Vec<Box<dyn Fn() -> StreamingTable<u32>>> = vec![
        Box::new(|| StreamingTable::new(log_columns())),
        Box::new(|| {
            StreamingTable::new(log_columns())
                .title("[b]events[/]")
                .caption("live")
                .box_set(rich::r#box::ROUNDED)
        }),
        Box::new(|| StreamingTable::new(log_columns()).without_box()),
        Box::new(|| {
            StreamingTable::new(log_columns())
                .show_edge(false)
                .expand(true)
        }),
    ];
    for build in &builders {
        for width in [80, 36, 20] {
            let console = colour(width);
            let mut table = build();
            let mut seed = 7u64;
            for step in 0..40u32 {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let key = (seed >> 33) as u32 % 8;
                match (seed >> 20) % 4 {
                    0 | 1 => {
                        let message = messages[(seed >> 40) as usize % messages.len()];
                        table.upsert(key, [Value::from(key), step.into(), message.into()]);
                    }
                    2 => {
                        table.update_cell(&key, 1, Value::Int(i64::from(step) * 1000));
                    }
                    _ => {
                        table.remove(&key);
                    }
                }
                let streamed = console.render_export(&table);
                let fresh = console.render_export(&table.to_table(&console));
                assert_eq!(streamed, fresh, "width {width}, step {step}");
            }
            assert!(table.stats().rows_rendered > 0);
        }
    }
}

fn log_columns() -> Vec<Column> {
    vec![
        Column::new("key").justify(Justify::Right),
        Column::new("step").justify(Justify::Right),
        Column::new("message"),
    ]
}

#[test]
fn upsert_keeps_position_and_remove_forgets_the_key() {
    let mut table = jobs();
    assert!(!table.upsert("build", ["build".into(), "done".into(), Value::Int(100)]));
    let keys: Vec<&str> = table.rows().map(|(key, _)| *key).collect();
    assert_eq!(keys, ["build", "test", "lint"]);
    assert_eq!(table.get(&"build").unwrap()[1], Value::from("done"));
    assert_eq!(table.remove(&"test").unwrap()[1], Value::from("queued"));
    assert!(!table.contains_key(&"test"));
    assert!(!table.update_cell(&"test", 0, "x"));
    assert!(!table.update_cell(&"lint", 9, "x"));
    // A re-inserted key goes to the end.
    assert!(table.upsert("test", ["test".into()]));
    let keys: Vec<&str> = table.rows().map(|(key, _)| *key).collect();
    assert_eq!(keys, ["build", "lint", "test"]);
    // Short rows are padded with nulls.
    assert_eq!(table.get(&"test").unwrap().len(), 3);
    assert_eq!(table.len(), 3);
    table.clear();
    assert!(table.is_empty());
    assert_eq!(
        plain(40).render_export(&table),
        "\
┏━━━━━┳━━━━━━━┳━━━━━━┓
┃ job ┃ state ┃ done ┃
┡━━━━━╇━━━━━━━╇━━━━━━┩
└─────┴───────┴──────┘
"
    );
}

#[test]
fn tail_window_keeps_the_last_rows_and_counts_the_rest() {
    let console = plain(40);
    let mut table = jobs().window(Window::Tail(2));
    assert_eq!(
        console.render_export(&table),
        "\
… 1 earlier row
┏━━━━━━┳━━━━━━━━┳━━━━━━┓
┃ job  ┃ state  ┃ done ┃
┡━━━━━━╇━━━━━━━━╇━━━━━━┩
│ test │ queued │      │
│ lint │ done   │  100 │
└──────┴────────┴──────┘
"
    );
    // Appending renders only the new row; the rows sliding out are dropped.
    table.reset_stats();
    table.upsert("docs", ["docs".into(), "queued".into(), Value::Null]);
    let out = console.render_export(&table);
    assert!(out.starts_with("… 2 earlier rows\n"), "{out}");
    assert_eq!(table.stats(), stats(1, 1, 1, 0));

    table.set_window(Window::Head(1));
    assert_eq!(
        console.render_export(&table),
        "\
┏━━━━━━━┳━━━━━━━━━┳━━━━━━┓
┃ job   ┃ state   ┃ done ┃
┡━━━━━━━╇━━━━━━━━━╇━━━━━━┩
│ build │ running │   40 │
└───────┴─────────┴──────┘
… 3 more rows
"
    );
    // ASCII-only consoles get a plain ellipsis.
    let ascii = Console::builder().width(40).ascii_only(true).build();
    assert!(ascii.render_export(&table).ends_with("\n... 3 more rows\n"));
}

#[test]
fn capacity_evicts_the_oldest_rows() {
    let mut log = StreamingTable::new([Column::new("#"), Column::new("event")])
        .capacity(2)
        .window(Window::Tail(2));
    for (n, event) in ["start", "fetch", "build", "test"].into_iter().enumerate() {
        log.upsert(n, [n.into(), event.into()]);
    }
    assert_eq!(log.len(), 2);
    assert_eq!(log.evicted(), 2);
    assert!(!log.contains_key(&0));
    assert_eq!(
        plain(40).render_export(&log),
        "\
… 2 earlier rows
┏━━━┳━━━━━━━┓
┃ # ┃ event ┃
┡━━━╇━━━━━━━┩
│ 2 │ build │
│ 3 │ test  │
└───┴───────┘
"
    );
}

#[test]
fn sorting_a_stream_reorders_cached_rows_without_rendering_them() {
    let console = plain(40);
    let mut table = StreamingTable::new([
        Column::new("job"),
        Column::new("progress").justify(Justify::Right),
    ]);
    for (job, done) in [("deploy-b", 7), ("deploy-a", 30), ("deploy-c", 7)] {
        table.upsert(job, [job.into(), Value::Int(done)]);
    }
    console.render_export(&table);
    table.set_sort([SortKey::desc(0)]);
    let out = console.render_export(&table);
    // `job ▼` is no wider than the job names, so no row renders again.
    assert_eq!(table.stats(), stats(2, 3, 3, 1));
    assert_eq!(
        out,
        "\
┏━━━━━━━━━━┳━━━━━━━━━━┓
┃ job ▼    ┃ progress ┃
┡━━━━━━━━━━╇━━━━━━━━━━┩
│ deploy-c │        7 │
│ deploy-b │        7 │
│ deploy-a │       30 │
└──────────┴──────────┘
"
    );
    // A new row lands in sorted position and renders alone.
    table.upsert("deploy-d", ["deploy-d".into(), Value::Int(8)]);
    let order: Vec<String> = console
        .render_export(&table)
        .lines()
        .skip(3)
        .take(4)
        .map(|line| line.split('│').nth(1).unwrap().trim().to_string())
        .collect();
    assert_eq!(order, ["deploy-d", "deploy-c", "deploy-b", "deploy-a"]);
    assert_eq!(table.stats().rows_rendered, 4);

    // Ties keep insertion order; clearing the sort restores it.
    table.set_sort([SortKey::asc(1)]);
    let text = console.render_export(&table);
    let b = text.find("deploy-b").unwrap();
    assert!(b < text.find("deploy-c").unwrap(), "{text}");
    table.set_sort([]);
    let text = console.render_export(&table);
    assert!(text.find("deploy-b").unwrap() < text.find("deploy-a").unwrap());
}

#[test]
fn a_stream_drives_a_live_coordinator_region() {
    let capabilities = TargetCapabilities {
        width: 40,
        height: 10,
        color_system: None,
        interactive: false,
        unicode: true,
        hyperlinks: false,
        sixel: Support::Unsupported,
    };
    let target = RenderTarget::new(TargetKind::Custom, capabilities, Theme::default_theme());
    let mut table = jobs().window(Window::Tail(2));
    let mut out = Vec::new();
    let mut live = LiveCoordinator::new(&mut out, target.clone());
    let region = live.add(target.segments(&table)).unwrap();
    live.refresh().unwrap();
    for step in 1..=3 {
        table.update_cell(&"lint", 1, format!("step {step}"));
        live.update(region.clone(), target.segments(&table))
            .unwrap();
        live.refresh().unwrap();
    }
    live.finish().unwrap();
    drop(live);
    // Non-interactive: one final snapshot.
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "\
… 1 earlier row
┏━━━━━━┳━━━━━━━━┳━━━━━━┓
┃ job  ┃ state  ┃ done ┃
┡━━━━━━╇━━━━━━━━╇━━━━━━┩
│ test │ queued │      │
│ lint │ step 3 │  100 │
└──────┴────────┴──────┘
"
    );
    // Three changes to one row: that row alone rendered each time.
    assert_eq!(table.stats(), stats(4, 5, 5, 1));
}

#[test]
fn streams_and_tables_read_as_accessible_text() {
    let table = jobs();
    let text = table.accessible_text(80);
    assert!(
        text.starts_with("Table with 3 rows, columns: job, state, done"),
        "{text}"
    );
    assert!(
        text.contains("Row 1: job: build; state: running; done: 40"),
        "{text}"
    );
    let data = table.to_data().sort_by([SortKey::desc(2)]);
    let text = data.accessible_text(80);
    assert!(text.contains("columns: job, state, done ▼"), "{text}");
}

// ---------------------------------------------------------------------------
// Sorting and grouping (#429)
// ---------------------------------------------------------------------------

/// A small deterministic generator for the property tests.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % bound
    }

    fn value(&mut self) -> Value {
        match self.next(6) {
            0 => Value::Null,
            1 => Value::Int(self.next(4) as i64 - 2),
            2 => Value::Float(self.next(4) as f64 / 2.0),
            3 => Value::from(format!("item{}", self.next(12))),
            4 => Value::from(""),
            _ => Value::from(["b", "A", "a", "10", "9"][self.next(5) as usize]),
        }
    }
}

#[test]
fn sorting_is_stable_for_any_keys() {
    let mut rng = Lcg(42);
    for _ in 0..300 {
        let columns = 1 + rng.next(3) as usize;
        let rows: Vec<Vec<Value>> = (0..rng.next(40))
            .map(|_| (0..columns).map(|_| rng.value()).collect())
            .collect();
        let keys: Vec<SortKey> = (0..1 + rng.next(3))
            .map(|_| {
                let column = rng.next(columns as u64 + 1) as usize; // may be missing
                let key = if rng.next(2) == 0 {
                    SortKey::asc(column)
                } else {
                    SortKey::desc(column)
                };
                if rng.next(4) == 0 {
                    key.lexical()
                } else {
                    key
                }
            })
            .collect();
        let order = sorted_indices(&rows, &keys);
        for pair in order.windows(2) {
            let (a, b) = (&rows[pair[0]], &rows[pair[1]]);
            match compare_rows(a, b, &keys) {
                std::cmp::Ordering::Less => {}
                // Ties keep their original order: that is stability.
                std::cmp::Ordering::Equal => assert!(pair[0] < pair[1], "{keys:?}"),
                std::cmp::Ordering::Greater => panic!("out of order under {keys:?}"),
            }
        }
        // Empty cells of the first key are last whatever the direction.
        let first = keys[0].column;
        let empty = |i: &usize| rows[*i].get(first).is_none_or(Value::is_empty);
        let boundary = order.iter().position(empty).unwrap_or(order.len());
        assert!(order[boundary..].iter().all(empty));
        // Sorting in place agrees with the index order.
        let mut sorted = rows.clone();
        sort_rows(&mut sorted, &keys);
        let expected: Vec<String> = order.iter().map(|&i| format!("{:?}", rows[i])).collect();
        let actual: Vec<String> = sorted.iter().map(|r| format!("{r:?}")).collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn sort_helpers_build_plain_core_tables() {
    let mut rows = vec![
        vec![Value::from("v1.10"), Value::Int(2)],
        vec![Value::from("v1.9"), Value::Null],
        vec![Value::from("v1.9"), Value::Int(5)],
        vec![Value::from("v1.2"), Value::Int(2)],
    ];
    sort_rows(&mut rows, &[SortKey::desc(1), SortKey::asc(0)]);
    let mut table = Table::new();
    table.add_column("version").add_column("n");
    for row in &rows {
        table.add_row_text(row.iter().map(Value::to_text).collect());
    }
    assert_eq!(
        plain(40).render_export(&table),
        "\
┏━━━━━━━━━┳━━━┓
┃ version ┃ n ┃
┡━━━━━━━━━╇━━━┩
│ v1.9    │ 5 │
│ v1.2    │ 2 │
│ v1.10   │ 2 │
│ v1.9    │   │
└─────────┴───┘
"
    );
}

fn services() -> TableData {
    let mut data = TableData::new([
        Column::new("service"),
        Column::new("region"),
        Column::new("errors").justify(Justify::Right),
        Column::new("latency")
            .justify(Justify::Right)
            .format(|v| match v.as_f64() {
                Some(ms) => Text::new(format!("{ms:.0} ms")),
                None => Text::new("-"),
            }),
    ]);
    data.push([
        "api".into(),
        "eu".into(),
        Value::Int(3),
        Value::Float(120.0),
    ]);
    data.push(["web".into(), "us".into(), Value::Int(0), Value::Float(80.0)]);
    data.push(["db".into(), "eu".into(), Value::Int(7), Value::Null]);
    data.push([
        "cache".into(),
        Value::Null,
        Value::Int(1),
        Value::Float(4.0),
    ]);
    data
}

#[test]
fn natural_multi_key_sort_with_indicators() {
    let data = services().sort_by([SortKey::asc(1), SortKey::desc(2)]);
    assert_eq!(
        plain(60).render_export(&data),
        "\
┏━━━━━━━━━┳━━━━━━━━━━━┳━━━━━━━━━━━┳━━━━━━━━━┓
┃ service ┃ region ▲1 ┃ errors ▼2 ┃ latency ┃
┡━━━━━━━━━╇━━━━━━━━━━━╇━━━━━━━━━━━╇━━━━━━━━━┩
│ db      │ eu        │         7 │       - │
│ api     │ eu        │         3 │  120 ms │
│ web     │ us        │         0 │   80 ms │
│ cache   │           │         1 │    4 ms │
└─────────┴───────────┴───────────┴─────────┘
"
    );
    // ASCII-only: `^`/`v`, and the core swaps in an ASCII box.
    let ascii = Console::builder().width(60).ascii_only(true).build();
    let out = ascii.render_export(&data.sort_by([SortKey::desc(3)]));
    assert_eq!(
        out.lines().nth(1).unwrap(),
        "| service | region | errors | latency v |"
    );
}

#[test]
fn groups_render_headers_rows_and_aggregates() {
    let data = services()
        .sort_by([SortKey::asc(1), SortKey::desc(2)])
        .group_by(
            GroupBy::new(1)
                .aggregate(Aggregate::count(1))
                .aggregate(Aggregate::sum(2))
                .aggregate(Aggregate::max(3)),
        )
        .totals(
            "all",
            [
                Aggregate::sum(2),
                Aggregate::mean(3),
                Aggregate::custom(1, |cells| {
                    Value::from(cells.iter().filter(|v| !v.is_empty()).count() * 100)
                }),
            ],
        );
    assert_eq!(
        plain(60).render_export(&data),
        "\
┏━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━┳━━━━━━━━━━━┳━━━━━━━━━┓
┃ service         ┃ region ▲1 ┃ errors ▼2 ┃ latency ┃
┡━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━╇━━━━━━━━━━━╇━━━━━━━━━┩
│ region: eu      │           │           │         │
│ db              │ eu        │         7 │       - │
│ api             │ eu        │         3 │  120 ms │
│ subtotal        │ 2         │        10 │  120 ms │
│ region: us      │           │           │         │
│ web             │ us        │         0 │   80 ms │
│ subtotal        │ 1         │         0 │   80 ms │
│ region: (empty) │           │           │         │
│ cache           │           │         1 │    4 ms │
│ subtotal        │ 0         │         1 │    4 ms │
│ all             │ 300       │        11 │   68 ms │
└─────────────────┴───────────┴───────────┴─────────┘
"
    );
    let groups = GroupBy::new(1)
        .aggregate(Aggregate::min(2))
        .groups(data.rows(), &data.order());
    let keys: Vec<String> = groups.iter().map(|g| g.key.plain()).collect();
    assert_eq!(keys, ["eu", "us", ""]);
    assert_eq!(groups[0].rows, [2, 0]);
    assert_eq!(groups[0].aggregates, [Value::Int(3)]);
}

#[test]
fn grouping_by_the_first_column_labels_with_the_key_alone() {
    let data = services()
        .sort_by([SortKey::asc(0)])
        .group_by(GroupBy::new(0).label("").aggregate(Aggregate::sum(2)));
    let out = plain(60).render_export(&data);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[1], "┃ service ▲ ┃ region ┃ errors ┃ latency ┃");
    assert_eq!(lines[3], "│ api       │        │        │         │");
    assert_eq!(lines[4], "│ api       │ eu     │      3 │  120 ms │");
    assert_eq!(lines[5], "│           │        │      3 │         │");
}

#[test]
fn styles_come_from_the_theme_and_fall_back() {
    let data = services()
        .sort_by([SortKey::asc(0)])
        .group_by(GroupBy::new(1).aggregate(Aggregate::sum(2)));
    let out = colour(60).render_export(&data);
    // Indicator cyan, group header bold, aggregates italic.
    assert!(out.contains("\u{1b}[1;36m▲\u{1b}[0m"), "{out:?}");
    assert!(out.contains("\u{1b}[1mregion: eu\u{1b}[0m"), "{out:?}");
    assert!(out.contains("\u{1b}[3msubtotal\u{1b}[0m"), "{out:?}");

    // extended_theme carries the keys, and a theme may override them.
    let theme = rich_ext::extended_theme();
    for (name, _) in rich_ext::table::STYLES {
        assert!(theme.get(name).is_some(), "{name}");
    }
    let mut theme = Theme::default_theme();
    theme.insert("table.group", rich::Style::parse("underline").unwrap());
    let console = Console::builder()
        .width(60)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .theme(theme)
        .build();
    let out = console.render_export(&data);
    assert!(out.contains("\u{1b}[4mregion: eu\u{1b}[0m"), "{out:?}");

    // The window indicator is dim.
    let out = colour(40).render_export(&jobs().window(Window::Head(1)));
    assert!(
        out.ends_with("\u{1b}[2m… 2 more rows\u{1b}[0m\n"),
        "{out:?}"
    );
}

#[test]
fn a_stream_snapshot_groups_like_plain_data() {
    let mut table = jobs().sort_by([SortKey::desc(2)]);
    table.upsert("docs", ["docs".into(), "done".into(), Value::Int(100)]);
    let data = table
        .to_data()
        .group_by(GroupBy::new(1).aggregate(Aggregate::count(0)));
    let out = plain(60).render_export(&data);
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    assert_eq!(
        lines[3..lines.len() - 1],
        [
            "│ state: done    │         │        │",
            "│ lint           │ done    │    100 │",
            "│ docs           │ done    │    100 │",
            "│ subtotal: 2    │         │        │",
            "│ state: running │         │        │",
            "│ build          │ running │     40 │",
            "│ subtotal: 1    │         │        │",
            "│ state: queued  │         │        │",
            "│ test           │ queued  │        │",
            "│ subtotal: 1    │         │        │",
        ]
    );
}

#[test]
fn tables_can_move_between_threads() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<StreamingTable<String>>();
    send_sync::<TableData>();
}
