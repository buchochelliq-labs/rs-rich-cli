//! `rich_ext::inspect`, `#[derive(Inspect)]` and `#[inspect]` on impl
//! blocks (#796). Rendered panels are snapshots at 80 columns, plain and
//! ANSI, in `tests/snapshots/inspect_*.txt`; set `UPDATE_SNAPSHOTS=1` to
//! rewrite them after a deliberate change, then review the diff.
#![cfg(feature = "macros")]

use std::fmt;

use rich::{ColorSystem, Console};
use rich_ext::inspect::{FieldValue, InspectOptions, Inspectable, Shape, TypeKind};
use rich_ext::{Inspect, inspect};

/// A point in the *plane*, with a `label`.
///
/// The second paragraph shows only with `help`.
#[derive(Inspect)]
#[inspect(methods)]
pub struct Point {
    /// Horizontal position.
    pub x: i32,
    pub y: f64,
    /// What to call it.
    pub label: String,
    secret: u8,
    #[inspect(skip)]
    #[allow(dead_code)]
    pub cache: NotDebug,
}

pub struct NotDebug;

#[inspect]
impl Point {
    /// The distance from the origin, times `scale`.
    pub fn norm(&self, scale: f64) -> f64 {
        (f64::from(self.x).powi(2) + self.y.powi(2)).sqrt() * scale
    }

    /// Move it.
    ///
    /// Both coordinates change.
    pub fn translate<T: Into<f64>>(&mut self, dx: i32, dy: T) {
        self.x += dx;
        self.y += dy.into();
    }

    /// Load one from disk.
    pub async fn load(path: &str) -> Option<Self> {
        let _ = path;
        None
    }

    #[allow(dead_code)]
    fn private_helper(&self) -> u8 {
        self.secret
    }

    #[inspect(skip)]
    pub fn hidden(&self) {}
}

fn point() -> Point {
    Point {
        x: 1,
        y: 2.5,
        label: "origin".to_string(),
        secret: 3,
        cache: NotDebug,
    }
}

/// A pair of numbers.
#[derive(Inspect)]
pub struct Pair(pub u8, pub u16, i64);

/// Nothing at all.
#[derive(Inspect)]
pub struct Unit;

/// A shape to draw.
#[derive(Inspect)]
pub enum Shape2 {
    /// A circle around the origin.
    Circle { radius: f64 },
    /// A rectangle.
    Rect(u32, u32),
    /// No shape.
    Empty,
}

/// A value in a box.
#[derive(Inspect)]
pub struct Wrapper<T> {
    pub inner: T,
    pub tags: Vec<&'static str>,
    #[inspect(skip)]
    pub _marker: std::marker::PhantomData<T>,
}

/// A linked list node.
#[derive(Inspect)]
pub struct Node {
    pub value: u32,
    #[inspect(nested)]
    pub next: Option<Box<Node>>,
}

fn hex(value: &u32, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{value:#06x}")
}

/// A colour with a custom-formatted field.
#[derive(Inspect)]
pub struct Colour {
    #[inspect(with = hex)]
    pub rgb: u32,
    #[inspect(with = opaque)]
    pub handle: NotDebug,
}

fn opaque(_: &NotDebug, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("<handle>")
}

fn plain(renderable: &dyn rich::Renderable) -> String {
    Console::builder()
        .width(80)
        .build()
        .render_to_string(renderable)
}

fn ansi(renderable: &dyn rich::Renderable) -> String {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(80)
        .build()
        .render_to_string(renderable)
}

fn assert_snapshot(name: &str, actual: &str) {
    let path = format!(
        "{}/tests/snapshots/inspect_{name}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing snapshot {path}; run with UPDATE_SNAPSHOTS=1"));
    assert_eq!(actual, expected, "{name} changed:\n{actual}");
}

fn snapshot(name: &str, report: &Inspect<'_>) {
    let text = plain(report);
    for line in text.lines() {
        assert!(rich::cells::cell_len(line) <= 80, "{name}: {line:?}");
    }
    assert_snapshot(name, &text);
    assert_snapshot(&format!("{name}_ansi"), &ansi(report));
}

#[test]
fn struct_defaults_hide_private_fields_and_methods() {
    let point = point();
    let text = plain(&Inspect::new(&point));
    assert!(text.contains("<struct 'inspect::Point'>"), "{text}");
    assert!(text.contains("A point in the *plane*, with a `label`."));
    assert!(!text.contains("second paragraph"));
    assert!(text.contains(r#"Point { x: 1, y: 2.5, label: "origin", secret: 3, .. }"#));
    assert!(text.contains("x = 1 Horizontal position."));
    assert!(!text.contains("secret ="));
    assert!(!text.contains("cache"));
    assert!(!text.contains("norm"));
    snapshot("point", &Inspect::new(&point));
}

#[test]
fn methods_private_and_help() {
    let point = point();
    let text = plain(&Inspect::new(&point).methods(true));
    assert!(
        text.contains("pub fn norm(&self, scale: f64) -> f64: The distance from the"),
        "{text}"
    );
    assert!(text.contains("pub async fn load(path: &str) -> Option<Self>:"));
    assert!(text.contains("pub fn translate<T: Into<f64>>(&mut self, dx: i32, dy: T):"));
    assert!(!text.contains("private_helper"));
    assert!(!text.contains("hidden"));
    snapshot("point_methods", &Inspect::new(&point).methods(true));

    let all = Inspect::new(&point).all(true).help(true);
    let text = plain(&all);
    assert!(text.contains("secret: u8 = 3"), "{text}");
    assert!(text.contains("fn private_helper(&self) -> u8:"));
    assert!(text.contains("The second paragraph shows only with `help`."));
    assert!(text.contains("Both coordinates change."));
    snapshot("point_all", &all);
}

#[test]
fn sort_off_keeps_declaration_order() {
    let point = point();
    let text = plain(&Inspect::new(&point).sort(false).value(false).docs(false));
    let x = text.find("x =").unwrap();
    let label = text.find("label =").unwrap();
    assert!(x < label, "{text}");
    let sorted = plain(&Inspect::new(&point).value(false).docs(false));
    assert!(sorted.find("label =").unwrap() < sorted.find("x =").unwrap());
    assert!(!text.contains("A point"));
}

#[test]
fn tuple_and_unit_structs() {
    let pair = Pair(1, 2, -3);
    let text = plain(&Inspect::new(&pair));
    assert!(text.contains("Pair(1, 2, -3)"), "{text}");
    assert!(text.contains("0 = 1"));
    assert!(text.contains("1 = 2"));
    assert!(!text.contains("2 = -3"));
    snapshot("pair", &Inspect::new(&pair).private(true));

    let text = plain(&Inspect::new(&Unit));
    assert!(text.contains("<struct 'inspect::Unit'>"));
    assert!(text.contains("Nothing at all."));
    assert!(text.contains("│ Unit"), "{text}");
}

#[test]
fn enums_show_their_variant() {
    let circle = Shape2::Circle { radius: 1.5 };
    let text = plain(&Inspect::new(&circle));
    assert!(text.contains("<enum 'inspect::Shape2'>"), "{text}");
    assert!(text.contains("Circle { radius: 1.5 }"));
    assert!(text.contains("variant = Circle A circle around the origin."));
    assert!(text.contains("radius = 1.5"));
    snapshot("enum", &Inspect::new(&circle).variants(true));

    let rect = Shape2::Rect(2, 3);
    let text = plain(&Inspect::new(&rect).variants(true));
    assert!(text.contains("Rect(2, 3)"));
    assert!(text.contains("variants = Circle, Rect, Empty"), "{text}");
    let empty = plain(&Inspect::new(&Shape2::Empty));
    assert!(empty.contains("variant = Empty No shape."), "{empty}");

    let ty = Shape2::Empty.inspect_type();
    assert_eq!(ty.kind, TypeKind::Enum);
    assert_eq!(ty.variants.len(), 3);
    assert_eq!(ty.variants[1].shape, Shape::Tuple);
}

#[test]
fn generics_use_the_concrete_type() {
    let wrapper = Wrapper {
        inner: (1u8, "two"),
        tags: vec!["a", "b"],
        _marker: std::marker::PhantomData,
    };
    let text = plain(&Inspect::new(&wrapper).types(true));
    assert!(
        text.contains("<struct 'inspect::Wrapper<(u8, &str)>'>"),
        "{text}"
    );
    assert!(text.contains(r#"inner: T = (1, "two")"#), "{text}");
    assert!(text.contains(r#"tags: Vec<&'static str> = ["a", "b"]"#));
    snapshot("generic", &Inspect::new(&wrapper).types(true));
}

/// A list node whose value counts how often it is read.
#[derive(Inspect)]
pub struct Counted {
    #[inspect(with = counted)]
    pub value: u32,
    #[inspect(nested)]
    pub next: Option<Box<Counted>>,
}

static READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn counted(value: &u32, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    write!(f, "{value}")
}

#[test]
fn the_depth_limit_stops_the_walk_not_only_the_output() {
    // A long list: past the limit, nothing of the rest may be read (the
    // walk used to visit every node, and a deep enough list overflowed the
    // stack).
    let mut list = None;
    for value in 0..1000 {
        list = Some(Box::new(Counted { value, next: list }));
    }
    let list = list.unwrap();
    READS.store(0, std::sync::atomic::Ordering::Relaxed);
    let text = plain(&Inspect::new(&*list).max_depth(1));
    assert!(text.contains("Counted { .. }"), "{text}");
    // Each node shown is read for its `{:?}` and `{:#?}`; 2 nodes are shown.
    assert!(
        READS.load(std::sync::atomic::Ordering::Relaxed) <= 4,
        "{} reads",
        READS.load(std::sync::atomic::Ordering::Relaxed)
    );
}

#[test]
fn nested_reports_stop_at_the_depth_limit() {
    let list = Node {
        value: 1,
        next: Some(Box::new(Node {
            value: 2,
            next: Some(Box::new(Node {
                value: 3,
                next: None,
            })),
        })),
    };
    let text = plain(&Inspect::new(&list).max_depth(1));
    assert!(
        text.contains("Node { value: 1, next: Node { value: 2, next: Node { .. } } }"),
        "{text}"
    );
    assert!(text.contains("next = ╭"), "{text}");
    snapshot("nested", &Inspect::new(&list).max_depth(1));

    let deep = plain(&Inspect::new(&list));
    assert!(deep.contains("next = None"), "{deep}");
}

#[test]
fn with_formats_a_field() {
    let colour = Colour {
        rgb: 0xff,
        handle: NotDebug,
    };
    let text = plain(&Inspect::new(&colour));
    assert!(text.contains("rgb = 0x00ff"), "{text}");
    assert!(text.contains("handle = <handle>"));
    assert!(text.contains("Colour { rgb: 0x00ff, handle: <handle> }"));
}

#[test]
fn long_values_expand_to_fit() {
    let wrapper = Wrapper {
        inner: vec!["a long string value"; 6],
        tags: vec![],
        _marker: std::marker::PhantomData,
    };
    let text = plain(&Inspect::new(&wrapper));
    assert!(text.contains("│ │ Wrapper {"), "{text}");
    assert!(text.contains("│ │ │   inner: ["), "{text}");
    for line in text.lines() {
        assert!(rich::cells::cell_len(line) <= 80, "{line:?}");
    }
}

#[test]
fn nothing_shown_says_so() {
    /// Secrets only.
    #[derive(Inspect)]
    struct Hidden {
        #[allow(dead_code)]
        key: u8,
    }
    let text = plain(&Inspect::new(&Hidden { key: 1 }).value(false));
    assert!(
        text.contains("1 attribute(s) not shown. Run Inspect::all for options."),
        "{text}"
    );
}

#[test]
fn options_titles_and_labels() {
    let point = point();
    let options = InspectOptions::new().title("[bold]here[/]").value(false);
    let text = plain(&Inspect::new(&point).options(options));
    assert!(text.contains("─ here ─"), "{text}");
    let text = plain(&Inspect::new(&point).label("point"));
    assert!(text.contains("point: <struct 'inspect::Point'>"), "{text}");
    // The print entry points compile and run.
    inspect(&point, InspectOptions::new().value(false));
    rich_ext::rich_inspect!(point, methods = true, docs = false);
}

#[test]
fn hand_written_impls_and_records() {
    struct Manual;
    impl Inspectable for Manual {
        fn inspect_type(&self) -> rich_ext::inspect::TypeInfo {
            rich_ext::inspect::TypeInfo::new("manual::Manual", "Manual", TypeKind::Struct)
                .doc(" Written by hand.")
        }
        fn inspect_record(&self, _depth: usize) -> rich_ext::inspect::Record {
            rich_ext::inspect::Record::new(
                Shape::Named,
                vec![
                    rich_ext::inspect::Field::new("level", "u8", FieldValue::debug(&7u8))
                        .public(true),
                ],
            )
        }
    }
    let text = plain(&Inspect::new(&Manual));
    assert!(text.contains("<struct 'manual::Manual'>"), "{text}");
    assert!(text.contains("Written by hand."));
    assert!(text.contains("level = 7"));
}

#[test]
fn clean_doc_removes_the_common_indent() {
    use rich_ext::inspect::clean_doc;
    assert_eq!(
        clean_doc(" One.\n\n Two\n   indented"),
        "One.\n\nTwo\n  indented"
    );
    assert_eq!(clean_doc(""), "");
    assert_eq!(clean_doc("\n  a\n"), "a");
}

#[test]
fn derive_compiles_for_its_cases() {
    trybuild::TestCases::new().pass("tests/ui_pass/inspect_*.rs");
}
