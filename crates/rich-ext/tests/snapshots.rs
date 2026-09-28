#![cfg(feature = "testing")]
use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Text, Theme};
use rich_ext::{
    target::{RenderTarget, TargetKind},
    testing::RenderSnapshot,
};
fn target() -> RenderTarget {
    RenderTarget::new(
        TargetKind::Capture,
        TargetCapabilities {
            width: 20,
            height: 8,
            color_system: Some(ColorSystem::Truecolor),
            interactive: false,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    )
}
#[test]
fn style_only_differences_are_visible_and_serialization_is_stable() {
    let t = target();
    let a = RenderSnapshot::capture(&t, &Text::styled("hello", "red"));
    let b = RenderSnapshot::capture(&t, &Text::styled("hello", "blue"));
    assert_eq!(a.plain, "hello");
    assert_eq!(a.plain, b.plain);
    assert!(a.diff(&b).unwrap().contains("foreground"));
    assert_eq!(
        a.to_json().unwrap(),
        RenderSnapshot::capture(&t, &Text::styled("hello", "red"))
            .to_json()
            .unwrap()
    );
    assert_eq!((a.width, a.height, a.schema_version), (20, 8, 1));
    assert!(a.ansi.contains("\x1b[31m"));
}
#[test]
fn visible_diff_identifies_changed_line_and_metadata_does_not_hide_links() {
    let t = target();
    let a = RenderSnapshot::capture(&t, &Text::new("one\ntwo"));
    let b = RenderSnapshot::capture(&t, &Text::new("one\nthree"));
    let diff = a.diff(&b).unwrap();
    assert!(diff.contains("-two"));
    assert!(diff.contains("+three"));
    assert_eq!(a.diff(&a), None);
}

/// Checked-in `RenderSnapshot` fixtures for diagnostics (#146): a multiline
/// message with notes, a real error chain and a source snippet. Regenerate
/// with `UPDATE_SNAPSHOTS=1 cargo test -p rs-rich-ext --features testing`.
#[test]
fn diagnostic_snapshots_match_fixtures() {
    use rich_ext::diagnostic::{Diagnostic, SourceSnippet};
    use rich_ext::event::EventView;

    #[derive(Debug)]
    struct Inner;
    impl std::fmt::Display for Inner {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "permission denied")
        }
    }
    impl std::error::Error for Inner {}
    #[derive(Debug)]
    struct Outer(Inner);
    impl std::fmt::Display for Outer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "cannot open config")
        }
    }
    impl std::error::Error for Outer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    let source = "[server]\nport = \"eighty\"\nhost = \"localhost\"\n";
    let start = source.find("\"eighty\"").unwrap();
    let cases: Vec<(&str, Diagnostic)> = vec![
        (
            "diagnostic_multiline",
            Diagnostic::new("migration failed\nrolled back 3 steps")
                .view(EventView::Expanded)
                .note("the database is unchanged")
                .help("rerun with --verbose"),
        ),
        (
            "diagnostic_chain",
            Diagnostic::from_error(&Outer(Inner), 8).view(EventView::Expanded),
        ),
        (
            "diagnostic_source",
            Diagnostic::new("expected an integer")
                .view(EventView::Expanded)
                .snippet(
                    SourceSnippet::new("app.toml".into(), source.into(), start..start + 8, 1)
                        .unwrap(),
                )
                .help("use a port number such as 8080"),
        ),
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let update = std::env::var_os("UPDATE_SNAPSHOTS").is_some();
    for (name, diagnostic) in cases {
        let got = RenderSnapshot::capture(&target(), &diagnostic)
            .to_json()
            .unwrap();
        let path = dir.join(format!("{name}.json"));
        if update {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &got).unwrap();
            continue;
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; run with UPDATE_SNAPSHOTS=1", path.display()));
        assert_eq!(
            got, expected,
            "{name} snapshot changed; run with UPDATE_SNAPSHOTS=1 if intended"
        );
    }
}

/// Returns its segments as given, to control how output is split.
struct Segments(Vec<rich::Segment>);
impl rich::Renderable for Segments {
    fn rich_render(
        &self,
        _console: &rich::Console,
        _options: &rich::ConsoleOptions,
    ) -> Vec<rich::Segment> {
        self.0.clone()
    }
}
fn seg(text: &str, style: &str) -> rich::Segment {
    rich::Segment::new(text, Some(rich::Style::parse(style).unwrap()))
}
#[test]
fn schema_2_ignores_resegmentation_but_not_style() {
    let t = target();
    let whole = Segments(vec![seg("hello world", "bold"), seg("\nnext", "")]);
    let split = Segments(vec![
        seg("hello", "bold"),
        seg(" world", "bold"),
        seg("\n", ""),
        seg("ne", ""),
        seg("xt", ""),
    ]);
    let a = RenderSnapshot::capture_frame(&t, &whole);
    let b = RenderSnapshot::capture_frame(&t, &split);
    assert_eq!(a.schema_version, 2);
    assert_eq!(a.diff(&b), None);
    assert_eq!(a, b);
    let rows = a.rows.as_ref().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].len(), 1);
    assert_eq!(rows[0][0].text, "hello world");
    // Schema 1 still sees the split.
    let (a1, b1) = (
        RenderSnapshot::capture(&t, &whole),
        RenderSnapshot::capture(&t, &split),
    );
    assert!(a1.diff(&b1).is_some());
    // A style change still shows.
    let red = Segments(vec![seg("hello world", "bold red"), seg("\nnext", "")]);
    let diff = a.diff(&RenderSnapshot::capture_frame(&t, &red)).unwrap();
    assert!(diff.contains("foreground"), "{diff}");
    assert!(diff.contains("style changed on line 1"), "{diff}");
}
#[test]
fn schema_1_fixtures_load_and_compare_with_schema_2() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let json = std::fs::read_to_string(dir.join("diagnostic_chain.json")).unwrap();
    let old: RenderSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(old.schema_version, 1);
    assert!(old.rows.is_none());
    // Round-trips byte for byte: schema 1 output has no `rows` key.
    assert_eq!(old.to_json().unwrap(), json);
    let upgraded = old.upgrade();
    assert_eq!(upgraded.schema_version, 2);
    assert_eq!(upgraded.diff(&old), None);
    let rows = upgraded.rows.as_ref().unwrap();
    let plain: Vec<String> = rows
        .iter()
        .map(|row| row.iter().map(|run| run.text.as_str()).collect())
        .collect();
    assert_eq!(plain.join("\n"), old.plain);
}
#[test]
fn schema_2_round_trips_through_json() {
    let t = target();
    let a = RenderSnapshot::capture_frame(&t, &Text::styled("one\ntwo", "red"));
    let json = a.to_json().unwrap();
    assert!(json.contains("\"rows\""));
    let back: RenderSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(back, a);
    assert!(a.ansi.contains("\x1b[31m"));
}
#[test]
fn schema_3_records_regions_and_reads_older_schemas() {
    use rich::panel::Panel;
    use rich::Table;
    let t = target();
    let table = || {
        let mut table = Table::new();
        table.add_column("A");
        table.add_row(&["x"]);
        table
    };
    let a = RenderSnapshot::capture_regions(&t, &Panel::new(Box::new(table())).title("P"));
    assert_eq!(a.schema_version, 3);
    let regions = a.regions.as_ref().unwrap();
    let roles: Vec<&str> = regions.iter().map(|r| r.role.as_str()).collect();
    assert_eq!(roles, ["panel", "table", "table-header", "table-cell"]);
    assert_eq!(regions[0].label.as_deref(), Some("P"));
    assert_eq!(regions[1].parent, Some(0));
    assert_eq!((regions[3].row, regions[3].column), (Some(0), Some(0)));
    // The rows and text are schema 2's: regions add, they change nothing.
    let b = RenderSnapshot::capture_frame(&t, &Panel::new(Box::new(table())).title("P"));
    assert_eq!((&a.plain, &a.rows, &a.ansi), (&b.plain, &b.rows, &b.ansi));
    assert_eq!(a.diff(&b), None, "regions compare only when both have them");
    let json = a.to_json().unwrap();
    let back: RenderSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(back, a);
    // A schema 2 document has no regions key and still loads.
    let old: RenderSnapshot = serde_json::from_str(&b.to_json().unwrap()).unwrap();
    assert!(old.regions.is_none());
    // A region that moved shows as a difference.
    let mut moved = a.clone();
    moved.regions.as_mut().unwrap()[3].spans[0][1] += 1;
    let diff = a.diff(&moved).unwrap();
    assert!(diff.contains("regions[3].spans"), "{diff}");
}
/// Draws one more `-` before its tagged `x` on each render, and counts them.
struct ShiftsEachRender(std::sync::atomic::AtomicUsize);
impl rich::Renderable for ShiftsEachRender {
    fn rich_render(
        &self,
        console: &rich::Console,
        _options: &rich::ConsoleOptions,
    ) -> Vec<rich::Segment> {
        use rich::protocol::{report_region, RegionInfo, RegionRole};
        let n = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut segments = vec![rich::Segment::new("-".repeat(n), None)];
        segments.extend(report_region(
            console,
            || RegionInfo::new(RegionRole::Other("mark".into())),
            || vec![rich::Segment::new("x", None)],
        ));
        segments
    }
}
#[test]
fn capture_regions_renders_once_and_its_regions_match_its_rows() {
    let t = target();
    let counter = ShiftsEachRender(std::sync::atomic::AtomicUsize::new(0));
    let snapshot = RenderSnapshot::capture_regions(&t, &counter);
    assert_eq!(counter.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(snapshot.plain, "x");
    let rows = snapshot.rows.as_ref().unwrap();
    let row: String = rows[0].iter().map(|run| run.text.as_str()).collect();
    assert_eq!(row, "x");
    let regions = snapshot.regions.as_ref().unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].role, "mark");
    // The span covers the `x` in the stored rows, not one from another render.
    assert_eq!(regions[0].spans, [[0, 0, 1]]);
}
