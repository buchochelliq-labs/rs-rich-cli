//! The record inspector: one record as fields, nested values as trees.
#![cfg(feature = "data")]

use rich::Console;
use rich_ext::data::*;

fn plain(width: usize, renderable: &dyn rich::Renderable) -> String {
    Console::builder()
        .width(width)
        .build()
        .render_export(renderable)
}

fn json(source: &str) -> Node {
    parse(Format::Json, source).unwrap()
}

fn path(s: &str) -> Path {
    s.parse().unwrap()
}

const RECORD: &str = r#"{
    "id": 7,
    "name": "web",
    "enabled": true,
    "note": null,
    "ports": [80, 443],
    "owner": {"team": "infra", "oncall": {"primary": "ana", "backup": "bo"}},
    "tags": []
}"#;

#[test]
fn record_at_depth_one() {
    let record = json(RECORD);
    assert_eq!(
        plain(50, &RecordView::new(&record)),
        "\
┏━━━━━━━━━┳━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━┓
┃ field   ┃ type ┃ value                  ┃
┡━━━━━━━━━╇━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━┩
│ id      │ int  │ 7                      │
│ name    │ str  │ web                    │
│ enabled │ bool │ true                   │
│ note    │ null │ null                   │
│ ports   │ seq  │ […] 2 items            │
│         │      │ ├── [0]: 80            │
│         │      │ └── [1]: 443           │
│ owner   │ map  │ {…} 2 keys             │
│         │      │ ├── team: \"infra\"      │
│         │      │ └── oncall: {…} 2 keys │
│ tags    │ seq  │ []                     │
└─────────┴──────┴────────────────────────┘
"
    );
}

#[test]
fn depth_zero_folds_every_nested_value() {
    let record = json(RECORD);
    let out = plain(50, &RecordView::new(&record).depth(0).show_types(false));
    assert_eq!(
        out,
        "\
┏━━━━━━━━━┳━━━━━━━━━━━━━┓
┃ field   ┃ value       ┃
┡━━━━━━━━━╇━━━━━━━━━━━━━┩
│ id      │ 7           │
│ name    │ web         │
│ enabled │ true        │
│ note    │ null        │
│ ports   │ […] 2 items │
│ owner   │ {…} 2 keys  │
│ tags    │ []          │
└─────────┴─────────────┘
"
    );
}

#[test]
fn expand_opens_one_branch_at_any_depth() {
    let record = json(RECORD);
    let view = RecordView::new(&record)
        .depth(0)
        .expand(path("owner.oncall"));
    assert!(view.is_open(&path("owner")));
    assert!(view.is_open(&path("owner.oncall")));
    assert!(!view.is_open(&path("ports")));
    let out = plain(60, &view);
    assert!(out.contains("│ ports   │ seq  │ […] 2 items"), "{out}");
    assert!(out.contains("└── oncall"), "{out}");
    assert!(out.contains("├── primary: \"ana\""), "{out}");
    assert!(out.contains("└── backup: \"bo\""), "{out}");
}

#[test]
fn collapse_folds_within_the_depth() {
    let record = json(RECORD);
    let view = RecordView::new(&record).depth(3).collapse(path("owner"));
    assert!(!view.is_open(&path("owner")));
    assert!(view.is_open(&path("ports")));
    let out = plain(60, &view);
    assert!(out.contains("│ owner   │ map  │ {…} 2 keys"), "{out}");
    assert!(!out.contains("team"), "{out}");
    // Expanding again reopens it.
    let view = view.expand(path("owner.oncall"));
    assert!(view.is_open(&path("owner")));
}

#[test]
fn the_branch_hook_opens_and_closes_by_path() {
    let record = json(RECORD);
    let mut view = RecordView::new(&record).depth(0);
    assert_eq!(view.branches(), [path("ports"), path("owner")]);
    assert!(view.open_branch(&path("owner.oncall")));
    assert!(view.is_open(&path("owner.oncall")));
    // Scalars and missing paths are not branches.
    assert!(!view.open_branch(&path("id")));
    assert!(!view.open_branch(&path("missing")));
    assert!(view.close_branch(&path("owner")));
    assert!(!view.is_open(&path("owner")));
    assert!(!view.is_open(&path("owner.oncall")));

    // Any `OpenBranch` works the same way.
    fn drill(target: &mut dyn OpenBranch, to: &Path) -> bool {
        target.open_branch(to)
    }
    assert!(drill(&mut view, &path("ports")));
    assert!(view.is_open(&path("ports")));
}

#[test]
fn a_branch_renders_on_its_own() {
    let record = json(RECORD);
    let view = RecordView::new(&record).expand(path("owner.oncall"));
    let tree = view.branch(&path("owner")).unwrap();
    assert_eq!(
        plain(40, &tree),
        "{…} 2 keys\n├── team: \"infra\"\n└── oncall\n    ├── primary: \"ana\"\n    └── backup: \"bo\"\n"
    );
    assert!(view.branch(&path("id")).is_none());
    assert!(view.branch(&path("nope")).is_none());
}

#[test]
fn long_strings_and_arrays_are_cut_with_counts() {
    let record = json(&format!(
        r#"{{"bio": "{}", "items": [{}]}}"#,
        "x".repeat(30),
        (0..12).map(|i| i.to_string()).collect::<Vec<_>>().join(",")
    ));
    let out = plain(60, &RecordView::new(&record).max_string(10).max_items(3));
    assert!(
        out.contains("│ bio   │ str  │ xxxxxxxxxx… (30 chars)"),
        "{out}"
    );
    assert!(out.contains("├── [2]: 2"), "{out}");
    assert!(out.contains("└── … 9 more"), "{out}");
    assert!(!out.contains("[3]"), "{out}");

    // Unlimited keeps everything.
    let out = plain(
        200,
        &RecordView::new(&record).max_string(None).max_items(None),
    );
    assert!(out.contains(&"x".repeat(30)), "{out}");
    assert!(out.contains("[11]: 11"), "{out}");
}

#[test]
fn redaction_masks_before_display() {
    let record = json(r#"{"user": "u", "password": "hunter2", "auth": {"token": "abc"}}"#);
    let view = RecordView::new(&record).redact(&Redaction::secrets());
    let out = plain(60, &view);
    assert!(!out.contains("hunter2"), "{out}");
    assert!(!out.contains("abc"), "{out}");
    assert!(out.contains("********"), "{out}");
    assert!(out.contains("│ user     │ str  │ u"), "{out}");
    assert_eq!(
        view.node().get("password").unwrap().value,
        Value::String("********".into())
    );
}

#[test]
fn sequences_and_scalars_are_records_too() {
    let items = json("[1, \"two\", [3], {}]");
    let out = plain(40, &RecordView::new(&items).max_items(3));
    assert!(out.contains("│ [0]   │ int  │ 1"), "{out}");
    assert!(out.contains("│ [1]   │ str  │ two"), "{out}");
    assert!(out.contains("└── [0]: 3"), "{out}");
    assert!(!out.contains("[3]"), "{out}");
    assert!(out.contains("… 1 more"), "{out}");

    let scalar = json("\"  \"");
    let out = plain(40, &RecordView::new(&scalar).title("one"));
    assert!(out.contains("one"), "{out}");
    assert!(out.contains("│ (value) │ str  │ \"  \""), "{out}");
}

#[test]
fn control_characters_never_reach_the_terminal() {
    let record = json(r#"{"k\u001b[31m": "v\u001b[2Jx\nnext"}"#);
    let out = plain(60, &RecordView::new(&record));
    assert!(!out.contains('\u{1b}'), "{out:?}");
    assert!(out.contains("\\u001b"), "{out}");
}

#[test]
fn the_table_fits_narrow_widths() {
    let record = json(RECORD);
    let view = RecordView::new(&record).depth(5);
    for width in [20, 32, 80] {
        let out = plain(width, &view);
        for line in out.lines() {
            assert!(rich::cells::cell_len(line) <= width, "{line:?} at {width}");
        }
    }
}
