//! Structured data: the document model, parsers, detection and views.
#![cfg(feature = "data")]

use rich::{ColorSystem, Console, Renderable, Style};
use rich_ext::data::*;

fn plain(width: usize, renderable: &dyn Renderable) -> String {
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

/// Every segment's text with its style, for checking highlights.
fn segments(renderable: &dyn Renderable) -> Vec<(String, Option<Style>)> {
    let console = Console::builder().width(80).build();
    renderable
        .rich_render(&console, &console.options())
        .into_iter()
        .map(|s| (s.text, s.style))
        .collect()
}

// --- model ------------------------------------------------------------------

#[test]
fn paths_display_quote_and_parse_back() {
    let p = Path::root()
        .child_key("servers")
        .child_index(0)
        .child_key("weird key")
        .child_key("name");
    assert_eq!(p.to_string(), r#"servers[0]["weird key"].name"#);
    assert_eq!(path(&p.to_string()), p);
    assert_eq!(path("$.a[2]"), Path::root().child_key("a").child_index(2));
    assert_eq!(Path::root().child_index(3).to_string(), "[3]");
    assert_eq!(Path::root().to_string(), "");
    assert!("a[x]".parse::<Path>().is_err());
    assert_eq!(p.last_key(), Some("name"));
    assert_eq!(
        p.parent().unwrap().to_string(),
        r#"servers[0]["weird key"]"#
    );
}

#[test]
fn serialize_keeps_u64_and_stringifies_keys() {
    use std::collections::BTreeMap;

    #[derive(serde::Serialize)]
    enum Kind {
        Unit,
        Tuple(u8, u8),
        Named { x: i32 },
    }
    #[derive(serde::Serialize)]
    struct Record {
        big: u64,
        small: i64,
        maybe: Option<u8>,
        kinds: Vec<Kind>,
        by_id: BTreeMap<u32, bool>,
    }
    let record = Record {
        big: u64::MAX,
        small: -3,
        maybe: None,
        kinds: vec![Kind::Unit, Kind::Tuple(1, 2), Kind::Named { x: 5 }],
        by_id: BTreeMap::from([(7, true)]),
    };
    let node = from_serialize(&record).unwrap();
    assert_eq!(node.get("big").unwrap().value, Value::UInt(u64::MAX));
    assert_eq!(node.get("small").unwrap().value, Value::Int(-3));
    assert_eq!(node.get("maybe").unwrap().value, Value::Null);
    assert_eq!(
        node.get("by_id").unwrap().get("7").unwrap().value,
        Value::Bool(true)
    );
    assert_eq!(
        node.get("kinds").unwrap().to_json(),
        serde_json::json!(["Unit", {"Tuple": [1, 2]}, {"Named": {"x": 5}}])
    );
    let keys: Vec<&str> = match &node.value {
        Value::Map(entries) => entries.iter().map(|(k, _)| k.as_str()).collect(),
        _ => unreachable!(),
    };
    assert_eq!(keys, ["big", "small", "maybe", "kinds", "by_id"]);
}

#[test]
fn serde_json_values_convert() {
    let value = serde_json::json!({"u": u64::MAX, "f": 1.5, "s": "x", "n": null});
    let node = Node::from(&value);
    assert_eq!(node.get("u").unwrap().value, Value::UInt(u64::MAX));
    assert_eq!(node.get("f").unwrap().value, Value::Float(1.5));
    assert_eq!(node.to_json(), value);
}

// --- explorer ---------------------------------------------------------------

#[test]
fn explorer_folds_paths_and_depths() {
    let node = json(r#"{"a": {"b": 1, "c": [1, 2]}, "d": "x", "e": {}}"#);
    assert_eq!(
        plain(40, &Explorer::new(&node).fold(path("a.c"))),
        concat!(
            "{…} 3 keys\n",
            "├── a\n",
            "│   ├── b: 1\n",
            "│   └── c: […] 2 items\n",
            "├── d: \"x\"\n",
            "└── e: {}\n",
        )
    );
    assert_eq!(
        plain(40, &Explorer::new(&node).max_depth(0)),
        "{…} 3 keys\n"
    );
    assert_eq!(
        plain(40, &Explorer::new(&node).max_depth(1).root_label("doc")),
        "doc\n├── a: {…} 2 keys\n├── d: \"x\"\n└── e: {}\n"
    );
}

#[test]
fn explorer_limits_children() {
    let node = json("[1, 2, 3, 4, 5]");
    assert_eq!(
        plain(40, &Explorer::new(&node).max_length(2)),
        "[…] 5 items\n├── [0]: 1\n├── [1]: 2\n└── … 3 more\n"
    );
}

#[test]
fn explorer_cuts_strings_to_the_width() {
    let node = json(r#"{"description": "abcdefghijklmnopqrstuvwxyz", "n": 1}"#);
    // 4 cells of guide, 13 of `description: `, 9 left for the string.
    assert_eq!(
        plain(26, &Explorer::new(&node)),
        "{…} 2 keys\n├── description: \"abcdef…\"\n└── n: 1\n"
    );
    assert_eq!(
        plain(80, &Explorer::new(&node).max_string(5)),
        "{…} 2 keys\n├── description: \"abcde…\"\n└── n: 1\n"
    );
    // Too narrow even for the key: the line itself ends in an ellipsis.
    let out = plain(10, &Explorer::new(&node));
    assert_eq!(out, "{…} 2 keys\n├── descr…\n└── n: 1\n");
    for line in out.lines() {
        assert!(rich::cells::cell_len(line) <= 10, "{line:?}");
    }
}

#[test]
fn explorer_shows_paths_and_types() {
    let node = json(r#"{"a": {"b": 1}, "c": [true]}"#);
    assert_eq!(
        plain(60, &Explorer::new(&node).show_paths(true)),
        "{…} 2 keys\n├── a\n│   └── b: 1  a.b\n└── c\n    └── [0]: true  c[0]\n"
    );
    assert_eq!(
        plain(60, &Explorer::new(&node).show_types(true)),
        "{…} 2 keys (map)\n├── a (map)\n│   └── b: 1 (int)\n└── c (seq)\n    └── [0]: true (bool)\n"
    );
}

#[test]
fn explorer_escapes_control_characters() {
    let node = json(r#"{"k\u001b": "line\nbreak"}"#);
    assert_eq!(
        plain(40, &Explorer::new(&node)),
        "{…} 1 key\n└── k\\u001b: \"line\\nbreak\"\n"
    );
}

#[test]
fn explorer_measures_its_widest_line() {
    let node = json(r#"{"name": "demo"}"#);
    let console = Console::builder().width(80).build();
    let measurement = Explorer::new(&node).measure(&console, &console.options());
    // `└── name: "demo"` is 16 cells; `└── name` plus an ellipsis 9.
    assert_eq!((measurement.minimum, measurement.maximum), (9, 16));
}

#[test]
fn explorer_table_view_of_records() {
    let node =
        json(r#"[{"name": "web", "port": 80}, {"name": "db", "port": 5432, "tags": ["a"]}]"#);
    assert_eq!(
        plain(60, &Explorer::new(&node).view(View::Table)),
        concat!(
            "┏━━━━━━┳━━━━━━┳━━━━━━━┓\n",
            "┃ name ┃ port ┃ tags  ┃\n",
            "┡━━━━━━╇━━━━━━╇━━━━━━━┩\n",
            "│ web  │   80 │       │\n",
            "│ db   │ 5432 │ [\"a\"] │\n",
            "└──────┴──────┴───────┘\n",
        )
    );
}

#[test]
fn explorer_table_view_of_other_documents_lists_leaves() {
    let node = json(r#"{"a": {"b": [1, 2, 3]}, "c": null}"#);
    assert_eq!(
        plain(60, &Explorer::new(&node).view(View::Table).max_length(2)),
        concat!(
            "┏━━━━━━━━┳━━━━━━━━━━┓\n",
            "┃ path   ┃ value    ┃\n",
            "┡━━━━━━━━╇━━━━━━━━━━┩\n",
            "│ a.b[0] │ 1        │\n",
            "│ a.b[1] │ 2        │\n",
            "│ a.b    │ … 1 more │\n",
            "│ c      │ null     │\n",
            "└────────┴──────────┘\n",
        )
    );
}

// --- tables and helpers -----------------------------------------------------

#[derive(serde::Serialize)]
struct Server {
    name: &'static str,
    port: u16,
    region: Option<&'static str>,
}

fn servers() -> Vec<Server> {
    vec![
        Server {
            name: "web",
            port: 80,
            region: Some("eu"),
        },
        Server {
            name: "db",
            port: 5432,
            region: None,
        },
        Server {
            name: "cache",
            port: 6379,
            region: Some("us"),
        },
    ]
}

#[test]
fn table_helper_with_overrides() {
    let view = table(&servers())
        .unwrap()
        .columns(["port", "name"])
        .header("port", "Port")
        .justify("name", rich::Justify::Right)
        .title("Servers")
        .max_rows(2);
    assert_eq!(
        plain(40, &view),
        concat!(
            "    Servers    \n",
            "┏━━━━━━┳━━━━━━┓\n",
            "┃ Port ┃ name ┃\n",
            "┡━━━━━━╇━━━━━━┩\n",
            "│   80 │  web │\n",
            "│ 5432 │   db │\n",
            "└──────┴──────┘\n",
            "   … 1 more    \n",
        )
    );
}

#[test]
fn table_nulls_are_dim_and_numbers_styled() {
    let view = table(&servers()).unwrap();
    let segments = segments(&view);
    let null = segments.iter().find(|(t, _)| t.trim() == "null").unwrap();
    assert_eq!(null.1, Some(Style::parse("dim").unwrap()));
    let port = segments.iter().find(|(t, _)| t.trim() == "5432").unwrap();
    assert_eq!(port.1, Some(Style::parse("bold not italic cyan").unwrap()));
}

#[test]
fn print_helpers_write_to_the_console() {
    let console = Console::builder().width(40).build();
    let out = console.capture(|c| print_json_to(c, &serde_json::json!({"a": [1]})).unwrap());
    assert_eq!(out, "{\n  \"a\": [\n    1\n  ]\n}\n");
    let out = console.capture(|c| print_tree_to(c, &serde_json::json!({"a": 1})).unwrap());
    assert_eq!(out, "{…} 1 key\n└── a: 1\n");
    let out = console.capture(|c| print_table_to(c, &servers()).unwrap());
    assert!(out.contains("│ cache │ 6379 │ us     │"), "{out}");
}

// --- flatten ----------------------------------------------------------------

#[test]
fn flatten_round_trips() {
    let node = json(r#"{"a": [1, {"b": null, "c": []}], "d": {}, "e": "x"}"#);
    let leaves = flatten(&node);
    let shown: Vec<String> = leaves.iter().map(|(p, _)| p.to_string()).collect();
    assert_eq!(shown, ["a[0]", "a[1].b", "a[1].c", "d", "e"]);
    assert_eq!(unflatten(leaves.clone()).unwrap().to_json(), node.to_json());
    // Any order works; keys keep first-seen order, indexes sort.
    let mut reversed = leaves;
    reversed.reverse();
    assert_eq!(
        unflatten(reversed).unwrap().to_json(),
        serde_json::json!({"e": "x", "d": {}, "a": [1, {"c": [], "b": null}]})
    );
    // A scalar root is one leaf at the root.
    let scalar = json("3");
    assert_eq!(flatten(&scalar), vec![(Path::root(), scalar.clone())]);
    assert_eq!(unflatten(flatten(&scalar)).unwrap(), scalar);
}

#[test]
fn unflatten_rejects_inconsistent_leaves() {
    let n = |v: i64| Node::new(Value::Int(v));
    assert_eq!(unflatten(vec![]), Err(UnflattenError::Empty));
    assert_eq!(
        unflatten(vec![(path("a"), n(1)), (path("a.b"), n(2))]),
        Err(UnflattenError::LeafAndContainer(path("a")))
    );
    assert_eq!(
        unflatten(vec![(path("a.b"), n(2)), (path("a"), n(1))]),
        Err(UnflattenError::LeafAndContainer(path("a")))
    );
    assert_eq!(
        unflatten(vec![(path("a"), n(1)), (path("a"), n(2))]),
        Err(UnflattenError::Duplicate(path("a")))
    );
    assert_eq!(
        unflatten(vec![(path("a.b"), n(1)), (path("a[0]"), n(2))]),
        Err(UnflattenError::MixedSegments(path("a")))
    );
    let sparse = unflatten(vec![(path("a[0]"), n(1)), (path("a[2]"), n(2))]).unwrap_err();
    assert_eq!(
        sparse,
        UnflattenError::SparseIndex {
            path: path("a"),
            missing: 1
        }
    );
    assert_eq!(sparse.to_string(), "`a` is missing index 1");
}

#[test]
fn flat_view_lists_leaves_with_types() {
    let node = json(r#"{"a": {"b": 1}, "c": []}"#);
    assert_eq!(
        plain(60, &FlatView::new(&node).show_types(true)),
        concat!(
            "┏━━━━━━┳━━━━━━━┳━━━━━━┓\n",
            "┃ path ┃ value ┃ type ┃\n",
            "┡━━━━━━╇━━━━━━━╇━━━━━━┩\n",
            "│ a.b  │ 1     │ int  │\n",
            "│ c    │ []    │ seq  │\n",
            "└──────┴───────┴──────┘\n",
        )
    );
}

// --- search -----------------------------------------------------------------

fn servers_doc() -> Node {
    json(
        r#"{"servers": [{"name": "web", "port": 80}, {"name": "db", "port": 5432}],
            "db_main": {"host": "db.local", "Port": 5433}}"#,
    )
}

fn hit_paths(node: &Node, query: &SearchQuery) -> Vec<String> {
    search(node, query)
        .into_iter()
        .map(|m| m.path.to_string())
        .collect()
}

#[test]
fn search_by_key_path_and_value() {
    let node = servers_doc();
    assert_eq!(
        hit_paths(&node, &SearchQuery::key("port")),
        ["servers[0].port", "servers[1].port"]
    );
    assert_eq!(
        hit_paths(&node, &SearchQuery::key("port").case_insensitive(true)),
        ["servers[0].port", "servers[1].port", "db_main.Port"]
    );
    assert_eq!(
        hit_paths(&node, &SearchQuery::path("servers[*].name")),
        ["servers[0].name", "servers[1].name"]
    );
    assert_eq!(
        hit_paths(&node, &SearchQuery::path("**.port")),
        ["servers[0].port", "servers[1].port"]
    );
    assert_eq!(
        hit_paths(&node, &SearchQuery::path("db_*.host")),
        ["db_main.host"]
    );
    assert_eq!(
        hit_paths(&node, &SearchQuery::path("servers[1]")),
        ["servers[1]"]
    );
    assert_eq!(
        hit_paths(&node, &SearchQuery::value("db")),
        ["servers[1].name", "db_main.host"]
    );
    // Criteria combine: a value inside a path.
    assert_eq!(
        hit_paths(&node, &SearchQuery::value("db").and_path("db_main.*")),
        ["db_main.host"]
    );
    assert_eq!(
        search(&node, &SearchQuery::value("54"))[0].matched_on,
        MatchKind::Value
    );
    assert!(search(&node, &SearchQuery::default()).is_empty());
    // `text` is the one OR criterion: a key or a value.
    let hits = search(&node, &SearchQuery::text("db").case_insensitive(true));
    let found: Vec<_> = hits
        .iter()
        .map(|hit| (hit.path.to_string(), hit.matched_on))
        .collect();
    assert_eq!(
        found,
        [
            ("servers[1].name".to_string(), MatchKind::Value),
            ("db_main".to_string(), MatchKind::Key),
            ("db_main.host".to_string(), MatchKind::Value),
        ]
    );
}

#[test]
fn search_results_show_context() {
    let node = servers_doc();
    let results = SearchResults::new(&node, &SearchQuery::value("db")).context(1);
    assert_eq!(
        plain(60, &results),
        concat!(
            "servers[1].name: \"db\"\n",
            "    port: 5432\n",
            "db_main.host: \"db.local\"\n",
            "    Port: 5433\n",
            "2 matches\n",
        )
    );
    assert_eq!(
        plain(60, &SearchResults::new(&node, &SearchQuery::key("nothing"))),
        "No matches\n"
    );
}

#[test]
fn search_results_highlight_the_match() {
    let node = servers_doc();
    let highlight = Style::parse("bold reverse yellow").unwrap();
    let key = Style::parse("bold blue").unwrap();
    let hit = key.combine(&highlight);
    let found = segments(&SearchResults::new(&node, &SearchQuery::key("or")));
    // `servers[0].port`: only `or` is highlighted.
    let first_line: Vec<&(String, Option<Style>)> =
        found.iter().take_while(|(t, _)| t != "\n").collect();
    let line: String = first_line.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(line, "servers[0].port: 80");
    let lit: Vec<&str> = first_line
        .iter()
        .filter(|(_, s)| s.as_ref() == Some(&hit))
        .map(|(t, _)| t.as_str())
        .collect();
    assert_eq!(lit, ["or"]);
    // And with colour on, as escape codes.
    let console = Console::builder()
        .width(60)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Standard))
        .build();
    let out = console.render_export(&SearchResults::new(&node, &SearchQuery::value("loc")));
    assert!(out.contains("\u{1b}[1;7;33mloc\u{1b}[0m"), "{out:?}");
}

// --- diff -------------------------------------------------------------------

#[test]
fn diff_reports_leaf_changes_in_document_order() {
    let old = json(r#"{"a": 1, "list": [1, 2, 3], "gone": {"x": true}, "same": "s"}"#);
    let new = json(r#"{"a": 1.5, "list": [1, 3], "same": "s", "new": null}"#);
    let changes = diff(&old, &new);
    let shown: Vec<(String, ChangeKind)> = changes
        .iter()
        .map(|c| (c.path.to_string(), c.kind))
        .collect();
    assert_eq!(
        shown,
        [
            ("a".to_string(), ChangeKind::Changed),
            ("list[1]".to_string(), ChangeKind::Changed),
            ("list[2]".to_string(), ChangeKind::Removed),
            ("gone.x".to_string(), ChangeKind::Removed),
            ("new".to_string(), ChangeKind::Added),
        ]
    );
    assert_eq!(
        plain(40, &DiffView::new(&old, &new)),
        "~ a: 1 → 1.5\n~ list[1]: 2 → 3\n- list[2]: 3\n- gone.x: true\n+ new: null\n"
    );
    assert_eq!(plain(40, &DiffView::new(&old, &old)), "No differences\n");
    // Metadata does not count as a change.
    let with_meta = parse_dotenv("# note\nA=1\n").unwrap();
    let without = parse_dotenv("A=1\n").unwrap();
    assert!(diff(&with_meta, &without).is_empty());
}

#[test]
fn diff_lines_are_coloured() {
    let old = json(r#"{"a": 1}"#);
    let new = json(r#"{"b": 1}"#);
    let found = segments(&DiffView::new(&old, &new));
    assert_eq!(
        found[0],
        ("- a: 1".into(), Some(Style::parse("red").unwrap()))
    );
    assert_eq!(
        found[2],
        ("+ b: 1".into(), Some(Style::parse("green").unwrap()))
    );
}

// --- redaction --------------------------------------------------------------

#[test]
fn redaction_masks_matching_leaves_only() {
    let node = json(
        r#"{"db": {"user": "u", "password": "p", "port": 5432},
            "API_KEY": "k", "tokens": ["a", "b"], "auth": {"enabled": true, "secret_id": 7}, "author": "Ada"}"#,
    );
    let safe = node.redacted(&Redaction::secrets());
    assert_eq!(
        safe.to_json(),
        serde_json::json!({
            "db": {"user": "u", "password": "********", "port": 5432},
            "API_KEY": "********", "tokens": ["********", "********"],
            "auth": {"enabled": true, "secret_id": "********"}, "author": "Ada"
        })
    );
    let custom = Redaction::new().pattern("*_id").mask("<hidden>");
    let masked = node.redacted(&custom);
    assert_eq!(
        masked.at(&path("auth.secret_id")).unwrap().as_str(),
        Some("<hidden>")
    );
    assert_eq!(masked.at(&path("db.password")).unwrap().as_str(), Some("p"));
    // A custom redactor: anything under `db`.
    let by_path = |p: &Path, n: &Node| {
        (p.segments().first() == Some(&PathSegment::Key("db".into())) && !n.is_container())
            .then(|| Node::new(Value::String("x".into())))
    };
    assert_eq!(
        node.redacted(&by_path).get("db").unwrap().to_json(),
        serde_json::json!({"user": "x", "password": "x", "port": "x"})
    );
}

// --- dotenv and INI ---------------------------------------------------------

#[test]
fn dotenv_quotes_escapes_export_and_comments() {
    let source = concat!(
        "# database\n",
        "# settings\n",
        "export DB_HOST=localhost\n",
        "DB_PASS='a\\nb $HOME'\n",
        "\n",
        "# dropped: a blank line follows\n",
        "\n",
        "GREETING=\"hello\\n\\\"world\\\" \\\\\"\n",
        "URL=http://x/#anchor # the url\n",
        "MULTI=\"one\n",
        "two\"\n",
        "EMPTY=\n",
        "DB_HOST=override\n",
    );
    let node = parse_dotenv(source).unwrap();
    let get = |k: &str| node.get(k).unwrap();
    assert_eq!(get("DB_HOST").as_str(), Some("override"));
    assert_eq!(get("DB_HOST").meta.position, Some(Position::new(13, 1)));
    assert_eq!(get("DB_PASS").as_str(), Some("a\\nb $HOME"));
    assert_eq!(get("DB_PASS").meta.comment, None);
    assert_eq!(get("GREETING").as_str(), Some("hello\n\"world\" \\"));
    assert_eq!(get("GREETING").meta.comment, None);
    assert_eq!(get("URL").as_str(), Some("http://x/#anchor"));
    assert_eq!(get("URL").meta.comment.as_deref(), Some("the url"));
    assert_eq!(get("MULTI").as_str(), Some("one\ntwo"));
    assert_eq!(get("EMPTY").as_str(), Some(""));
    let keys: Vec<&str> = match &node.value {
        Value::Map(e) => e.iter().map(|(k, _)| k.as_str()).collect(),
        _ => unreachable!(),
    };
    assert_eq!(
        keys,
        ["DB_HOST", "DB_PASS", "GREETING", "URL", "MULTI", "EMPTY"]
    );
    // The comment block directly above the first entry stays with it, even
    // after the value is overridden? No: the override replaces the node.
    let first = parse_dotenv("# database\n# settings\nexport DB_HOST=x\n").unwrap();
    assert_eq!(
        first.get("DB_HOST").unwrap().meta.comment.as_deref(),
        Some("database\nsettings")
    );
    assert_eq!(
        first.get("DB_HOST").unwrap().meta.position,
        Some(Position::new(3, 8))
    );
}

#[test]
fn dotenv_errors_have_positions() {
    let cases = [
        ("1A=x\n", 1, 1, "expected a variable name"),
        ("A=1\nB x\n", 2, 3, "expected `=` after `B`"),
        (
            "A=\"x\" y\n",
            1,
            7,
            "unexpected text after the closing quote",
        ),
        ("A='open\n", 1, 3, "unterminated ' quote"),
    ];
    for (source, line, column, message) in cases {
        let error = parse_dotenv(source).unwrap_err();
        assert_eq!(
            error.position,
            Some(Position::new(line, column)),
            "{source:?}"
        );
        assert!(error.message.starts_with(message), "{error}");
    }
}

#[test]
fn config_file_view_of_dotenv_has_no_section_column() {
    let node = parse_dotenv("# where\nHOST=example.com\nSECRET_KEY=abc\n").unwrap();
    assert_eq!(
        plain(
            60,
            &ConfigFileView::new(&node).redactor(Redaction::secrets())
        ),
        concat!(
            "┏━━━━━━━━━━━━┳━━━━━━━━━━━━━┳━━━━━━━━━┓\n",
            "┃ key        ┃ value       ┃ comment ┃\n",
            "┡━━━━━━━━━━━━╇━━━━━━━━━━━━━╇━━━━━━━━━┩\n",
            "│ HOST       │ example.com │ where   │\n",
            "│ SECRET_KEY │ ********    │         │\n",
            "└────────────┴─────────────┴─────────┘\n",
        )
    );
}

const INI: &str = "; global\nname = demo\n\n; the server\n[server]\n# listen here\nhost = 0.0.0.0\nport: 8080\nhost = 127.0.0.1\n[a.b]\npath = /x\n  /y\n";

#[test]
fn ini_sections_comments_and_duplicates() {
    let node = parse_ini(INI).unwrap();
    assert_eq!(
        node.to_json(),
        serde_json::json!({
            "name": "demo",
            "server": {"host": "127.0.0.1", "port": "8080"},
            "a.b": {"path": "/x\n/y"}
        })
    );
    let server = node.get("server").unwrap();
    assert_eq!(server.meta.comment.as_deref(), Some("the server"));
    assert_eq!(server.meta.position, Some(Position::new(5, 1)));
    let host = server.get("host").unwrap();
    // The last value wins, with its position; the first one's comment went
    // with the first node.
    assert_eq!(host.meta.position, Some(Position::new(9, 1)));
    assert_eq!(host.meta.comment, None);
    assert_eq!(
        node.get("name").unwrap().meta.comment.as_deref(),
        Some("global")
    );
    assert_eq!(
        plain(60, &ConfigFileView::new(&node)),
        concat!(
            "┏━━━━━━━━━┳━━━━━━┳━━━━━━━━━━━┳━━━━━━━━━┓\n",
            "┃ section ┃ key  ┃ value     ┃ comment ┃\n",
            "┡━━━━━━━━━╇━━━━━━╇━━━━━━━━━━━╇━━━━━━━━━┩\n",
            "│         │ name │ demo      │ global  │\n",
            "│ server  │ host │ 127.0.0.1 │         │\n",
            "│         │ port │ 8080      │         │\n",
            "│ a.b     │ path │ /x\\n/y    │         │\n",
            "└─────────┴──────┴───────────┴─────────┘\n",
        )
    );
}

#[test]
fn ini_errors_have_positions() {
    let error = parse_ini("[open\nk=v\n").unwrap_err();
    assert_eq!(error.position, Some(Position::new(1, 1)));
    let error = parse_ini("[s]\n  = v\n").unwrap_err();
    assert_eq!(error.message, "empty key");
    let error = parse_ini("[s]\nk = v\njust words\n").unwrap_err();
    assert_eq!(error.position, Some(Position::new(3, 1)));
}

#[test]
fn explorer_shows_comments_dim() {
    let node = parse_dotenv("# the host\nHOST=x\n").unwrap();
    assert_eq!(
        plain(40, &Explorer::new(&node)),
        "{…} 1 key\n└── HOST: \"x\"  # the host\n"
    );
    let found = segments(&Explorer::new(&node));
    assert!(found.contains(&("  # the host".into(), Some(Style::parse("dim").unwrap()))));
}

// --- JSON errors and diagnostics --------------------------------------------

fn diagnostic(format: Format, source: &str) -> (DataError, String) {
    let error = parse(format, source).unwrap_err();
    let out = plain(80, &error.to_diagnostic(source, "input"));
    (error, out)
}

#[test]
fn json_errors_render_as_diagnostics() {
    let (error, out) = diagnostic(Format::Json, "{\n  \"a\" 1\n}");
    assert_eq!(error.position, Some(Position::new(2, 7)));
    assert_eq!(
        error.to_string(),
        "invalid JSON at line 2, column 7: expected `:`"
    );
    assert_eq!(
        out,
        concat!(
            "error: invalid JSON: expected `:`\n",
            "  --> input:2:7\n",
            "--> input\n",
            "1 | {\n",
            "2 |   \"a\" 1\n",
            "  |       ^ expected `:`\n",
            "3 | }\n",
        )
    );
    // Columns count characters, not bytes.
    let error = parse(Format::Json, "{\"é\" 1}").unwrap_err();
    assert_eq!(error.position, Some(Position::new(1, 6)));
}

#[test]
fn dotenv_and_ini_errors_render_as_diagnostics() {
    let (_, out) = diagnostic(Format::Dotenv, "A=1\nB x\n");
    assert!(out.contains("  --> input:2:3\n"), "{out}");
    assert!(
        out.contains("2 | B x\n  |   ^ expected `=` after `B`\n"),
        "{out}"
    );
    let (_, out) = diagnostic(Format::Ini, "[s]\nnovalue\n");
    assert!(out.contains("  --> input:2:1\n"), "{out}");
    assert!(out.contains("2 | novalue\n  | ^ expected"), "{out}");
}

// --- detection --------------------------------------------------------------

const PROSE: &str = "This is just a paragraph of text.\nIt has two lines, and a colon: here.";
const WORD: &str = "hello";
const MARKDOWN: &str =
    "# Setup\n\nRun the installer, then set:\n\nport: 8080\n\n- step one\n- step two\n";
const CSV: &str = "name,age,city\nbob,3,paris\nalice,4,rome\n";
const CSV_WITH_COLONS: &str = "time,message\n10:00,started\n10:05,done: ok\n";

#[test]
fn detection_names_and_extensions() {
    assert_eq!(Format::from_name("YML"), Some(Format::Yaml));
    assert_eq!(Format::from_name("env"), Some(Format::Dotenv));
    assert_eq!(Format::from_extension(".cfg"), Some(Format::Ini));
    assert_eq!(
        Format::from_file_name("dir/.env.local"),
        Some(Format::Dotenv)
    );
    assert_eq!(Format::from_file_name("app.JSON"), Some(Format::Json));
    assert_eq!(Format::from_file_name(".bashrc"), None);
    assert_eq!(Format::detect("A=1", Some("x/.env")), Some(Format::Dotenv));
    // A hint for a format this build lacks is ignored, not trusted.
    if !Format::Yaml.is_enabled() {
        assert_eq!(Format::detect("key: value\nother: 2", Some("a.yaml")), None);
    }
}

#[test]
fn detection_positive_cases() {
    assert_eq!(Format::detect(" {\"a\": 1} ", None), Some(Format::Json));
    assert_eq!(Format::detect("[1, 2]", None), Some(Format::Json));
    assert_eq!(
        Format::detect("# comment\nexport A=1\nB='two words'\n", None),
        Some(Format::Dotenv)
    );
    let ini = "[server]\nhost = example.com\nname: web\n";
    assert_eq!(Format::detect(ini, None), Some(Format::Ini));
}

#[test]
fn detection_negative_cases() {
    for text in [
        PROSE,
        WORD,
        MARKDOWN,
        CSV,
        CSV_WITH_COLONS,
        "",
        "  \n",
        "key: value",
        "- a\n- b\n",
        "a = b is the formula",
    ] {
        assert_eq!(Format::detect(text, None), None, "{text:?}");
    }
    // `KEY = VALUE` with spaces is not strict dotenv; without a section it
    // is not INI either.
    assert_eq!(Format::detect("A = 1 2\nB = x y\n", None), None);
    // A JSON scalar is not a document.
    assert_eq!(Format::detect("\"text\"", None), None);
}

// --- YAML -------------------------------------------------------------------

#[cfg(feature = "yaml")]
mod yaml {
    use super::*;

    #[test]
    fn scalars_resolve_per_the_core_schema() {
        let node = parse_yaml(concat!(
            "n: ~\nn2: null\nb: True\ni: -12\nh: 0x1F\no: 0o17\nf: 1.5e3\n",
            "inf: -.inf\nnan: .NaN\ns: yes\nq: \"12\"\nt: !!str 3\nbig: 18446744073709551615\n",
        ))
        .unwrap();
        let get = |k: &str| node.get(k).unwrap().value.clone();
        assert_eq!(get("n"), Value::Null);
        assert_eq!(get("n2"), Value::Null);
        assert_eq!(get("b"), Value::Bool(true));
        assert_eq!(get("i"), Value::Int(-12));
        assert_eq!(get("h"), Value::Int(31));
        assert_eq!(get("o"), Value::Int(15));
        assert_eq!(get("f"), Value::Float(1500.0));
        assert_eq!(get("inf"), Value::Float(f64::NEG_INFINITY));
        assert!(matches!(get("nan"), Value::Float(f) if f.is_nan()));
        assert_eq!(get("s"), Value::String("yes".into()));
        assert_eq!(get("q"), Value::String("12".into()));
        assert_eq!(get("t"), Value::String("3".into()));
        assert_eq!(get("big"), Value::UInt(u64::MAX));
        assert_eq!(
            node.get("i").unwrap().meta.position,
            Some(Position::new(4, 1))
        );
    }

    #[test]
    fn anchors_and_aliases_are_recorded_and_shown() {
        let source = "base: &base\n  host: localhost\ncopy: *base\nx: &v 1\ny: *v\n";
        let node = parse_yaml(source).unwrap();
        assert_eq!(
            node.get("base").unwrap().meta.anchor.as_deref(),
            Some("base")
        );
        let copy = node.get("copy").unwrap();
        assert_eq!(copy.meta.alias.as_deref(), Some("base"));
        assert_eq!(copy.meta.anchor, None);
        assert_eq!(copy.get("host").unwrap().as_str(), Some("localhost"));
        assert_eq!(
            plain(40, &Explorer::new(&node)),
            concat!(
                "{…} 4 keys\n",
                "├── base &base\n",
                "│   └── host: \"localhost\"\n",
                "├── copy *base\n",
                "│   └── host: \"localhost\"\n",
                "├── x: 1 &v\n",
                "└── y: 1 *v\n",
            )
        );
        let found = segments(&Explorer::new(&node));
        assert!(found.contains(&(" &base".into(), Some(Style::parse("dim cyan").unwrap()))));
    }

    #[test]
    fn multiple_documents_become_a_sequence() {
        let node = parse_yaml("a: 1\n---\nb: 2\n").unwrap();
        assert_eq!(node.to_json(), serde_json::json!([{"a": 1}, {"b": 2}]));
        assert_eq!(parse_yaml("").unwrap().value, Value::Null);
        assert_eq!(
            parse_yaml("- 1\n").unwrap().to_json(),
            serde_json::json!([1])
        );
    }

    #[test]
    fn alias_bombs_are_refused() {
        let mut source = String::from("a0: &a0 [x, x, x, x, x, x, x, x, x, x]\n");
        for i in 1..8 {
            let prev = format!("*a{}", i - 1);
            let items = [prev.as_str(); 10].join(", ");
            source.push_str(&format!("a{i}: &a{i} [{items}]\n"));
        }
        let error = parse_yaml(&source).unwrap_err();
        assert!(error.message.contains("more than 1000000 nodes"), "{error}");
    }

    #[test]
    fn deep_nesting_is_refused() {
        // Block nesting is ours to limit...
        let block: String = (0..600).map(|i| format!("{}a:\n", " ".repeat(i))).collect();
        let error = parse_yaml(&block).unwrap_err();
        assert_eq!(error.message, "nesting deeper than 512 levels");
        assert_eq!(error.position, Some(Position::new(513, 513)));
        // ...flow nesting stops earlier, in the parser.
        let flow = format!("{}{}", "[".repeat(600), "]".repeat(600));
        assert_eq!(
            parse_yaml(&flow).unwrap_err().message,
            "recursion limit exceeded"
        );
        let fine = format!("{}{}", "[".repeat(100), "]".repeat(100));
        assert!(parse_yaml(&fine).is_ok());
    }

    #[test]
    fn yaml_errors_render_as_diagnostics() {
        let (error, out) = diagnostic(Format::Yaml, "a: 1\nb: [1, 2\nc: 3\n");
        let position = error.position.unwrap();
        assert!(
            out.contains(&format!(
                "  --> input:{}:{}",
                position.line, position.column
            )),
            "{out}"
        );
        assert!(out.contains(&format!("{} | ", position.line)), "{out}");
        let (error, _) = diagnostic(Format::Yaml, "key: [a\nb: c: d");
        assert!(error.position.is_some());
        let error = parse_yaml("a: *nope\n").unwrap_err();
        assert_eq!(error.position, Some(Position::new(1, 4)));
    }

    #[test]
    fn detection() {
        let yaml = "name: demo\nservers:\n  - host: a\n    port: 80\n";
        assert_eq!(Format::detect(yaml, None), Some(Format::Yaml));
        assert_eq!(
            Format::detect("---\na: 1\nb: 2\n", None),
            Some(Format::Yaml)
        );
        assert_eq!(
            Format::detect("- name: a\n- name: b\n", None),
            Some(Format::Yaml)
        );
        assert_eq!(
            Format::detect("key: value", Some("a.yml")),
            Some(Format::Yaml)
        );
        // Front matter followed by prose is Markdown.
        let front = "---\ntitle: x\ndate: y\n---\n# Heading\n\nSome prose here.\n";
        assert_eq!(Format::detect(front, None), None);
        for text in [
            PROSE,
            WORD,
            MARKDOWN,
            CSV,
            CSV_WITH_COLONS,
            "key: value",
            "- a\n- b\n",
        ] {
            assert_eq!(Format::detect(text, None), None, "{text:?}");
        }
    }
}

// --- TOML -------------------------------------------------------------------

#[cfg(feature = "toml")]
mod toml {
    use super::*;

    const DOC: &str = "title = \"x\"\n\n[owner]\nname = \"Tom\"\ndob = 1979-05-27 07:32:00Z\n\n[[fruit]]\nname = \"apple\"\ntags = [\"red\", { inline = true }]\n\n[[fruit]]\nname = \"banana\"\n";

    #[test]
    fn tables_arrays_and_datetimes() {
        let node = parse_toml(DOC).unwrap();
        assert_eq!(
            plain(50, &Explorer::new(&node)),
            concat!(
                "{…} 3 keys\n",
                "├── title: \"x\"\n",
                "├── owner\n",
                "│   ├── name: \"Tom\"\n",
                "│   └── dob: 1979-05-27 07:32:00Z\n",
                "└── fruit\n",
                "    ├── [0]\n",
                "    │   ├── name: \"apple\"\n",
                "    │   └── tags\n",
                "    │       ├── [0]: \"red\"\n",
                "    │       └── [1]\n",
                "    │           └── inline: true\n",
                "    └── [1]\n",
                "        └── name: \"banana\"\n",
            )
        );
        let dob = node.at(&path("owner.dob")).unwrap();
        assert_eq!(dob.value, Value::DateTime("1979-05-27 07:32:00Z".into()));
        assert_eq!(dob.meta.position, Some(Position::new(5, 1)));
        // Document order, not alphabetical.
        let node = parse_toml("z = 1\na = 2\n").unwrap();
        assert_eq!(node.to_json().to_string(), r#"{"z":1,"a":2}"#);
    }

    #[test]
    fn toml_errors_render_as_diagnostics() {
        let (error, out) = diagnostic(Format::Toml, "a = 1\nb = \n");
        assert_eq!(error.position, Some(Position::new(2, 5)));
        assert!(out.contains("  --> input:2:5\n"), "{out}");
        assert!(out.contains("2 | b = \n  |     ^"), "{out}");
    }

    #[test]
    fn detection() {
        assert_eq!(Format::detect(DOC, None), Some(Format::Toml));
        assert_eq!(
            Format::detect("[s]\nport = 8080\n", None),
            Some(Format::Toml)
        );
        // Not valid TOML (a bare value): INI.
        assert_eq!(
            Format::detect("[s]\nhost = example.com\n", None),
            Some(Format::Ini)
        );
        // Valid as both TOML and dotenv: TOML, which types the values.
        assert_eq!(Format::detect("A=1\nB=\"x\"\n", None), Some(Format::Toml));
        for text in [PROSE, WORD, MARKDOWN, CSV, CSV_WITH_COLONS] {
            assert_eq!(Format::detect(text, None), None, "{text:?}");
        }
    }
}

// --- XML --------------------------------------------------------------------

#[cfg(feature = "xml")]
mod xml {
    use super::*;

    const DOC: &str = "<?xml version=\"1.0\"?>\n<!-- servers -->\n<server id=\"7\" x:ns=\"u\">\n  <name>web &amp; api</name>\n  <port>80</port>\n  <port>81</port>\n  <empty/>\n  <note lang=\"en\">hi</note>\n  mixed <![CDATA[<text>]]>\n</server>\n";

    #[test]
    fn attributes_repeats_and_text() {
        let node = parse_xml(DOC).unwrap();
        assert_eq!(
            plain(50, &Explorer::new(&node)),
            concat!(
                "{…} 1 key\n",
                "└── server\n",
                "    ├── @id: \"7\"\n",
                "    ├── @x:ns: \"u\"\n",
                "    ├── name: \"web & api\"\n",
                "    ├── port\n",
                "    │   ├── [0]: \"80\"\n",
                "    │   └── [1]: \"81\"\n",
                "    ├── empty: null\n",
                "    ├── note\n",
                "    │   ├── @lang: \"en\"\n",
                "    │   └── #text: \"hi\"\n",
                "    └── #text: \"mixed <text>\"\n",
            )
        );
        let server = node.get("server").unwrap();
        assert_eq!(server.meta.xml, Some(XmlKind::Element));
        assert_eq!(server.meta.position, Some(Position::new(3, 1)));
        assert_eq!(
            server.get("@id").unwrap().meta.xml,
            Some(XmlKind::Attribute)
        );
        assert_eq!(server.get("#text").unwrap().meta.xml, Some(XmlKind::Text));
        let found = segments(&Explorer::new(&node));
        assert!(found.contains(&(
            "@id".into(),
            Some(Style::parse("not italic yellow").unwrap())
        )));
    }

    #[test]
    fn large_documents_stay_linear() {
        let mut doc = String::from("<items>");
        for i in 0..50_000 {
            doc.push_str(&format!("<item n=\"{i}\">{i}</item>"));
        }
        doc.push_str("</items>");
        let started = std::time::Instant::now();
        let node = parse_xml(&doc).unwrap();
        assert_eq!(node.at(&path("items.item")).unwrap().len(), 50_000);
        assert!(started.elapsed().as_secs() < 10);
    }

    #[test]
    fn deep_nesting_is_refused() {
        let deep = format!("{}{}", "<a>".repeat(600), "</a>".repeat(600));
        let error = parse_xml(&deep).unwrap_err();
        assert_eq!(error.message, "nesting deeper than 512 levels");
        assert_eq!(error.position, Some(Position::new(1, 512 * 3 + 1)));
    }

    #[test]
    fn xml_errors_render_as_diagnostics() {
        let (error, out) = diagnostic(Format::Xml, "<a>\n  <b></c>\n</a>");
        assert_eq!(error.position, Some(Position::new(2, 6)));
        assert!(out.contains("2 |   <b></c>\n  |      ^"), "{out}");
        let error = parse_xml("<a>\n  <b>\n").unwrap_err();
        assert_eq!(error.message, "unclosed element <b>");
        assert_eq!(error.position, Some(Position::new(2, 3)));
        let error = parse_xml("<a/><b/>").unwrap_err();
        assert_eq!(error.message, "more than one root element");
    }

    #[test]
    fn detection() {
        assert_eq!(Format::detect(DOC, None), Some(Format::Xml));
        assert_eq!(Format::detect("<a><b>1</b></a>", None), Some(Format::Xml));
        assert_eq!(Format::detect("<not closed", None), None);
        assert_eq!(Format::detect("<3 you", None), None);
    }
}

// --- selection --------------------------------------------------------------

#[test]
fn selectors_registry() {
    struct Keys;
    struct KeysSelector(String);
    impl Selector for KeysSelector {
        fn select<'a>(&self, root: &'a Node) -> Result<Vec<(Path, &'a Node)>, SelectError> {
            let mut out = Vec::new();
            root.walk(|p, n| {
                if p.last_key() == Some(self.0.as_str()) {
                    out.push((p.clone(), n));
                }
            });
            Ok(out)
        }
    }
    impl SelectorBackend for Keys {
        fn name(&self) -> &str {
            "keys"
        }
        fn compile(&self, expr: &str) -> Result<Box<dyn Selector>, SelectError> {
            Ok(Box::new(KeysSelector(expr.to_string())))
        }
    }
    let mut selectors = Selectors::default();
    assert_eq!(
        selectors.names(),
        if cfg!(feature = "jsonpath") {
            vec!["jsonpath"]
        } else {
            vec![]
        }
    );
    selectors.register(Box::new(Keys));
    let node = servers_doc();
    let found = selectors
        .compile("keys", "name")
        .unwrap()
        .select(&node)
        .unwrap();
    assert_eq!(found.len(), 2);
    let error = selectors.compile("xpath", "/a").err().unwrap();
    assert_eq!(error.to_string(), "no selector backend named `xpath`");
}

#[cfg(feature = "jsonpath")]
mod jsonpath {
    use super::*;

    fn store() -> Node {
        json(
            r#"{"store": {
                "book": [
                    {"title": "A", "price": 8.95, "cat": "ref"},
                    {"title": "B", "price": 12.99, "cat": "fic"},
                    {"title": "C", "price": 8.99, "cat": "fic", "isbn": "x"}
                ],
                "bicycle": {"color": "red", "price": 19.95}
            }, "n": null, "flag": true, "count": 3}"#,
        )
    }

    fn select(expr: &str) -> Vec<String> {
        let node = store();
        JsonPathSelector::parse(expr)
            .unwrap_or_else(|e| panic!("{expr}: {e}"))
            .select(&node)
            .unwrap()
            .into_iter()
            .map(|(p, _)| p.to_string())
            .collect()
    }

    #[test]
    fn paths_names_and_indexes() {
        assert_eq!(select("$"), [""]);
        assert_eq!(select("$.store.bicycle.color"), ["store.bicycle.color"]);
        assert_eq!(select("$['store'][\"bicycle\"]"), ["store.bicycle"]);
        assert_eq!(select("store.book[0].title"), ["store.book[0].title"]);
        assert_eq!(select("$.store.book[-1].title"), ["store.book[2].title"]);
        assert_eq!(select("$.store.book[5]"), Vec::<String>::new());
        assert_eq!(select("$.missing.key"), Vec::<String>::new());
    }

    #[test]
    fn wildcards_and_recursive_descent() {
        assert_eq!(
            select("$.store.book[*].title"),
            [
                "store.book[0].title",
                "store.book[1].title",
                "store.book[2].title"
            ]
        );
        assert_eq!(select("$.store.*"), ["store.book", "store.bicycle"]);
        assert_eq!(
            select("$..price"),
            [
                "store.book[0].price",
                "store.book[1].price",
                "store.book[2].price",
                "store.bicycle.price"
            ]
        );
        assert_eq!(select("$..book[1].title"), ["store.book[1].title"]);
        assert_eq!(
            select("$.store.bicycle..*"),
            ["store.bicycle.color", "store.bicycle.price"]
        );
    }

    #[test]
    fn slices_and_unions() {
        let titles = |expr: &str| {
            select(expr)
                .into_iter()
                .map(|p| p.replace("store.book", "").replace(".title", ""))
                .collect::<Vec<_>>()
        };
        assert_eq!(titles("$.store.book[0:2].title"), ["[0]", "[1]"]);
        assert_eq!(titles("$.store.book[1:].title"), ["[1]", "[2]"]);
        assert_eq!(titles("$.store.book[:1].title"), ["[0]"]);
        assert_eq!(titles("$.store.book[-2:].title"), ["[1]", "[2]"]);
        assert_eq!(titles("$.store.book[::2].title"), ["[0]", "[2]"]);
        assert_eq!(titles("$.store.book[::-1].title"), ["[2]", "[1]", "[0]"]);
        assert_eq!(titles("$.store.book[0, 2].title"), ["[0]", "[2]"]);
        assert_eq!(select("$['n','flag']"), ["n", "flag"]);
    }

    #[test]
    fn filters() {
        let books = |expr: &str| {
            select(expr)
                .into_iter()
                .map(|p| p.replace("store.book", ""))
                .collect::<Vec<_>>()
        };
        assert_eq!(books("$.store.book[?(@.price < 9)]"), ["[0]", "[2]"]);
        assert_eq!(books("$.store.book[?(@.price <= 8.99)]"), ["[0]", "[2]"]);
        assert_eq!(
            books("$.store.book[?(@.price > 12.99)]"),
            Vec::<String>::new()
        );
        assert_eq!(books("$.store.book[?(@.price >= 12.99)]"), ["[1]"]);
        assert_eq!(books("$.store.book[?(@.cat == 'fic')]"), ["[1]", "[2]"]);
        assert_eq!(books("$.store.book[?(@.cat != \"fic\")]"), ["[0]"]);
        assert_eq!(books("$.store.book[?(@.isbn)]"), ["[2]"]);
        assert_eq!(books("$.store.book[?(!@.isbn)]"), ["[0]", "[1]"]);
        assert_eq!(books("$.store.book[?@.title > 'A']"), ["[1]", "[2]"]);
        assert_eq!(
            books("$.store.book[?(@.price < 9 && @.cat == 'fic' || @.title == 'A')]"),
            ["[0]", "[2]"]
        );
        assert_eq!(
            books("$.store.book[?(@.price < 9 && (@.cat == 'fic' || @.title == 'Z'))]"),
            ["[2]"]
        );
        assert_eq!(select("$[?(@ == true)]"), ["flag"]);
        assert_eq!(select("$[?(@ == null)]"), ["n"]);
        assert_eq!(select("$[?(@ == 3)]"), ["count"]);
        assert_eq!(select("$[?(@ == false)]"), Vec::<String>::new());
        assert_eq!(
            books("$.store.book[?(@.price > $.count)]"),
            ["[0]", "[1]", "[2]"]
        );
    }

    #[test]
    fn errors_point_at_the_column() {
        let error = |expr: &str| JsonPathSelector::parse(expr).unwrap_err().to_string();
        assert_eq!(error("$.store.book[1"), "expected `]` at column 15");
        assert_eq!(error("$.store.."), "expected a key at column 10");
        assert_eq!(error("$[?(@.a = 1)]"), "expected `==` at column 9");
        assert_eq!(error("$[1:2:0]"), "slice step cannot be 0 at column 7");
        assert_eq!(error("$.a b"), "unexpected text after the path at column 5");
        assert_eq!(
            error("$[?(@.a == foo)]"),
            "unexpected `foo`; expected `@`, `$` or a literal at column 12"
        );
        assert_eq!(error("$['a"), "unterminated string at column 3");
        assert_eq!(error("  "), "empty expression at column 1");
        assert_eq!(error("$[x]"), "unexpected `x` in brackets at column 3");
        assert_eq!(error("$.a)"), "unexpected `)` at column 4");
    }
}
