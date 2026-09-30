//! Explorers and utilities (0.0.14 workstream 3): the data explorer, tree
//! filtering and breadcrumbs, copying through the clipboard, selector
//! reload and the theme picker, all headless.

use std::sync::mpsc;
use std::time::Duration;

use rich::{Style, Theme};
use rich_ext::data::{parse, Format};
use rich_interact::clipboard::{self, ClipboardError, CopyFormat};
use rich_interact::components::json_path;
use rich_interact::headless::{self, Headless, Script};
use rich_interact::{
    DataExplorer, EventLoop, Item, Key, LoopOptions, MultiSelect, Outcome, Select, TableSelect,
    ThemePicker, TreeSelect,
};

const CONFIG: &str = r#"{"server": {"host": "localhost", "port": 8080}, "users": [{"name": "ada"}, {"name": "grace"}], "debug": true}"#;

fn explorer() -> DataExplorer {
    DataExplorer::new("config.json", parse(Format::Json, CONFIG).unwrap())
}

#[test]
fn the_explorer_starts_folded_below_the_root() {
    let (outcome, record) = headless::run(explorer(), Script::new().keys("escape"), 60, 14);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    let first = &record.frames[0];
    assert!(first.contains("$ {…} 3 keys"), "{first}");
    assert!(first.contains("▸ server: {…} 2 keys"), "{first}");
    assert!(first.contains("debug: true"), "{first}");
    assert!(!first.contains("host"), "folded: {first}");
    // The breadcrumbs line shows the focused node: the root.
    assert_eq!(first.lines().nth(1).unwrap().trim(), "$");
}

#[test]
fn right_expands_and_the_breadcrumbs_follow_the_cursor() {
    let script = Script::new().keys("down right down down enter");
    let (outcome, record) = headless::run(explorer(), script, 60, 14);
    let path = outcome.unwrap().value().unwrap();
    assert_eq!(json_path(&path), "$.server.port");
    let before_enter = &record.frames[record.frames.len() - 2];
    assert!(before_enter.contains("$ › server › port"), "{before_enter}");
    assert!(before_enter.contains("port: 8080"), "{before_enter}");
}

#[test]
fn searching_keeps_the_ancestors_of_what_matches() {
    let script = Script::new().text("grace").keys("enter");
    let (outcome, record) = headless::run(explorer(), script, 60, 14);
    let path = outcome.unwrap().value().unwrap();
    assert_eq!(json_path(&path), "$.users[1].name");
    let found = &record.frames[record.frames.len() - 2];
    // The match, under the ancestors that place it, even though `users`
    // was folded; nothing unrelated.
    for row in [
        "$ {…} 3 keys",
        "users: […] 2 items",
        "[1]: {…} 1 key",
        "name: \"grace\"",
    ] {
        assert!(found.contains(row), "{row} in {found}");
    }
    assert!(!found.contains("ada"), "{found}");
    assert!(!found.contains("debug"), "{found}");
    // The guides join what is listed, and a listed ancestor is open.
    assert!(found.contains("└── ▾ users"), "{found}");
    assert!(found.contains("        └── name"), "{found}");
}

#[test]
fn ctrl_y_copies_the_path_and_alt_y_the_value() {
    let script = Script::new().keys("down ctrl+y alt+y escape");
    let (_, record) = headless::run(explorer(), script, 60, 14);
    assert_eq!(
        record.copies,
        [
            "$.server",
            "{\n  \"host\": \"localhost\",\n  \"port\": 8080\n}"
        ]
    );
    assert!(record
        .frames
        .iter()
        .any(|frame| frame.contains("copied path")));
    assert!(record
        .frames
        .iter()
        .any(|frame| frame.contains("copied value")));
}

#[test]
fn without_a_terminal_clipboard_a_copy_says_why() {
    let mut backend = Headless::new(Script::new().keys("down ctrl+y escape"), 60, 14);
    backend.clipboard = false;
    let record = backend.record();
    let mut event_loop = EventLoop::new(backend, LoopOptions::default());
    let handle = event_loop.mount(explorer());
    event_loop.run().unwrap();
    assert_eq!(handle.take(), Some(Outcome::Cancelled));
    let record = record.borrow();
    assert!(record.copies.is_empty());
    assert!(record
        .frames
        .iter()
        .any(|frame| frame.contains("cannot copy path: no terminal clipboard")));
}

#[test]
fn outside_an_event_loop_nothing_is_copied() {
    assert!(!clipboard::available());
    assert!(matches!(
        clipboard::copy("x"),
        Err(ClipboardError::Unsupported(_))
    ));
}

/// Every format `rich inspect` reads explores, headless.
#[test]
fn every_data_format_explores() {
    let documents = [
        (Format::Json, r#"{"a": {"b": 1}}"#),
        (Format::Yaml, "a:\n  b: 1\n"),
        (Format::Toml, "[a]\nb = 1\n"),
        (Format::Xml, "<a><b>1</b></a>"),
        (Format::Ini, "[a]\nb = 1\n"),
        (Format::Dotenv, "A_B=1\n"),
    ];
    for (format, text) in documents {
        let node = parse(format, text).unwrap_or_else(|e| panic!("{format:?}: {}", e.message));
        let explorer = DataExplorer::new(format!("doc.{}", format.name()), node);
        let script = Script::new().keys("down right down enter");
        let (outcome, record) = headless::run(explorer, script, 60, 12);
        let path = outcome
            .unwrap()
            .value()
            .unwrap_or_else(|| panic!("{format:?}"));
        assert!(!path.is_root(), "{format:?}: {}", record.last_frame());
    }
}

#[test]
fn the_preview_draws_the_focused_subtree() {
    let script = Script::new().keys("down escape");
    let (_, record) = headless::run(explorer(), script, 90, 14);
    let frame = &record.frames[1];
    assert!(frame.contains("│ server"), "{frame}");
    assert!(frame.contains("├── host: \"localhost\""), "{frame}");
}

fn tree() -> TreeSelect<&'static str> {
    TreeSelect::new(
        "Pick",
        [
            (0, "src"),
            (1, "lib.rs"),
            (1, "bin"),
            (2, "main.rs"),
            (0, "docs"),
            (1, "guide.md"),
        ],
    )
}

#[test]
fn a_tree_filter_keeps_ancestors_and_picks_the_best_match() {
    let script = Script::new().text("main").keys("enter");
    let (outcome, record) = headless::run(tree(), script, 50, 12);
    assert_eq!(outcome.unwrap().value(), Some("main.rs"));
    let found = &record.frames[record.frames.len() - 2];
    assert!(found.contains("src"), "{found}");
    assert!(found.contains("bin"), "{found}");
    assert!(!found.contains("docs"), "{found}");
    // The cursor is on the match, not on its first ancestor.
    assert!(found.contains("❯     └── main.rs"), "{found}");
}

#[test]
fn breadcrumbs_are_opt_in_and_cut_from_the_left() {
    let (_, plain) = headless::run(tree(), Script::new().keys("escape"), 50, 12);
    assert!(!plain.frames[0].lines().nth(1).unwrap().contains('›'));
    let tree = tree().breadcrumbs(true);
    let (_, record) = headless::run(tree, Script::new().keys("down down down escape"), 20, 12);
    let frame = &record.frames[record.frames.len() - 2];
    let crumbs = frame.lines().nth(1).unwrap();
    assert_eq!(crumbs.trim(), "… › bin › main.rs");
}

#[test]
fn a_tree_copies_the_focused_path() {
    let script = Script::new().keys("down down down ctrl+y escape");
    let (_, record) = headless::run(tree(), script, 50, 12);
    assert_eq!(record.copies, ["src/bin/main.rs"]);
}

fn table() -> TableSelect<usize> {
    TableSelect::new(
        "Host",
        ["name", "address"],
        [
            (0, vec!["web".to_string(), "10.0.0.1".to_string()]),
            (1, vec!["db, primary".to_string(), "10.0.0.2".to_string()]),
        ],
    )
}

#[test]
fn a_table_copies_rows_and_cells_in_each_format() {
    let script =
        Script::new().keys("down ctrl+y alt+f ctrl+y alt+f ctrl+y ctrl+right alt+y escape");
    let (_, record) = headless::run(table(), script, 50, 10);
    assert_eq!(
        record.copies,
        [
            "db, primary\t10.0.0.2",
            "\"db, primary\",10.0.0.2",
            r#"{"name": "db, primary", "address": "10.0.0.2"}"#,
            r#""10.0.0.2""#,
        ]
    );
    let focused = record.writes.concat();
    // The focused column's heading is underlined once moved to.
    assert!(focused.contains("\u{1b}[1;4maddress"), "{focused:?}");
}

#[test]
fn a_table_starts_in_the_format_asked_for() {
    let table = table().copy_format(CopyFormat::Csv);
    let (_, record) = headless::run(table, Script::new().keys("alt+y escape"), 50, 10);
    assert_eq!(record.copies, ["web"]);
}

#[test]
fn reload_keeps_the_query_the_cursor_and_the_marks() {
    let select = MultiSelect::new("Pick", ["alpha", "beta", "gamma", "delta"]).reload_on(
        Key::ctrl('r'),
        || {
            vec![
                Item::new("zeta", "zeta"),
                Item::new("delta", "delta"),
                Item::new("beta", "beta"),
            ]
        },
    );
    // "ta" matches beta and delta: mark beta (Tab moves on to delta), then
    // reload.
    let script = Script::new().text("ta").keys("tab ctrl+r enter");
    let (outcome, record) = headless::run(select, script, 40, 10);
    let last = &record.frames[record.frames.len() - 2];
    // The query is still typed and matches the new item; the mark and the
    // focus stayed with their items.
    assert!(last.contains("Pick › ta"), "{last}");
    assert!(last.contains("zeta"), "{last}");
    assert!(last.contains("❯ ○ delta"), "{last}");
    assert!(last.contains("◉ beta"), "{last}");
    assert_eq!(outcome.unwrap().value(), Some(vec!["beta"]));
}

#[test]
fn fed_items_arrive_on_the_next_tick() {
    let (tx, rx) = mpsc::channel();
    let select = Select::new("Pick", ["waiting"]).reload_from(rx);
    tx.send(vec![Item::new("ready", "ready")]).unwrap();
    let script = Script::new().wait(Duration::from_millis(150)).keys("enter");
    let (outcome, record) = headless::run(select, script, 40, 10);
    assert_eq!(outcome.unwrap().value(), Some("ready"));
    assert!(record.frames.iter().any(|frame| frame.contains("❯ ready")));
}

#[test]
fn a_reload_key_reloads_from_its_source() {
    let mut generation = 0;
    let select = Select::new("Pick", ["one"]).reload_on(Key::ctrl('r'), move || {
        generation += 1;
        vec![
            Item::new("one", "one"),
            Item::new("new", format!("new {generation}")),
        ]
    });
    let script = Script::new().keys("ctrl+r down enter");
    let (outcome, record) = headless::run(select, script, 40, 10);
    assert_eq!(outcome.unwrap().value(), Some("new"));
    assert!(record.frames.iter().any(|frame| frame.contains("new 1")));
}

#[test]
fn the_theme_picker_previews_each_theme() {
    let mut night = Theme::new();
    night.insert("info", Style::parse("#ff00ff").unwrap());
    let picker = ThemePicker::new("Theme", [("default", Theme::new()), ("night", night)]);
    let script = Script::new().keys("down enter");
    let (outcome, record) = headless::run(picker, script, 100, 14);
    assert_eq!(outcome.unwrap().value().as_deref(), Some("night"));
    // The default preview draws `info` cyan; night's is magenta, in its
    // own colour.
    assert!(
        record.writes[0].contains("\u{1b}[36minfo"),
        "{:?}",
        record.writes[0]
    );
    assert!(
        record.output().contains("\u{1b}[38;2;255;0;255minfo"),
        "{:?}",
        record.output()
    );
}

#[test]
fn the_theme_picker_lists_plugin_themes() {
    use rich_ext::plugin::{Plugin, PluginError, PluginMetadata, PluginRegistrar};
    struct Pack;
    impl Plugin for Pack {
        fn metadata(&self) -> PluginMetadata {
            PluginMetadata::new("pack", "Theme pack", "0.0.1")
        }
        fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
            registrar.theme("solar", Theme::new());
            registrar.theme("dusk", Theme::new());
            Ok(())
        }
    }
    let mut registry = rich_ext::ExtensionRegistry::new();
    registry.add_plugin(&Pack).unwrap();
    let picker = ThemePicker::from_registry("Theme", &registry);
    assert_eq!(picker.names(), ["default", "dusk", "solar"]);
}
