//! Trees by key (round 6): selection and open items that follow their
//! items, reveal, a prefix before the indent, a highlight symbol, the keys
//! a tree takes, and reloading a lazy tree's level.

use std::collections::HashSet;

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::widgets::{tree_keyed, TreeItem, TreeKeys};

fn plain(driver: &intuituive::Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

/// Folders of files, by name, with keys `folder/file`.
fn folders(names: &[(&'static str, Vec<&'static str>)]) -> Vec<TreeItem> {
    names
        .iter()
        .map(|(folder, files)| {
            let kids = files
                .iter()
                .map(|f| TreeItem::keyed(format!("{folder}/{f}"), *f));
            TreeItem::keyed(*folder, *folder).children(kids)
        })
        .collect()
}

#[test]
fn open_items_and_the_selection_stay_with_their_keys_when_rows_move() {
    let app = App::new(|| {
        let added = signal(false);
        let selected = signal(Some("src/lib.rs".to_string()));
        let expanded = signal(HashSet::from(["src".to_string()]));
        let items = move || {
            let mut all = vec![("src", vec!["lib.rs", "main.rs"]), ("tests", vec!["a.rs"])];
            if added.get() {
                all.insert(0, ("docs", vec!["guide.md"]));
            }
            folders(&all)
        };
        column([
            tree_keyed(items, selected, expanded).fixed(5),
            text(move || format!("{:?}", selected.get())),
        ])
        .on_key("a", move |_| added.set(true))
    });
    let mut driver = app.driver(24, 6);
    driver.settle();
    assert_eq!(
        plain(&driver)[..4],
        ["▾ src", "    lib.rs", "    main.rs", "▸ tests"]
    );
    // A folder before `src`: by path, `src`'s place would now be `docs`.
    driver.press("a");
    assert_eq!(
        plain(&driver)[..5],
        ["▸ docs", "▾ src", "    lib.rs", "    main.rs", "▸ tests"]
    );
    let styled = intuituive::testing::styled(&driver);
    assert!(
        styled.lines().nth(2).unwrap().starts_with("[reverse]"),
        "{styled}"
    );
    assert_eq!(plain(&driver)[5], "Some(\"src/lib.rs\")");
}

#[test]
fn collapsing_moves_a_selection_inside_up_to_the_item() {
    let app = App::new(|| {
        let selected = signal(Some("src/main.rs".to_string()));
        let expanded = signal(HashSet::from(["src".to_string()]));
        let items = || folders(&[("src", vec!["lib.rs", "main.rs"])]);
        column([
            tree_keyed(items, selected, expanded).fixed(3),
            text(move || format!("{:?}", selected.get())),
        ])
    });
    let mut driver = app.driver(24, 4);
    driver.press("left left");
    assert_eq!(plain(&driver)[0], "▸ src");
    assert_eq!(plain(&driver)[3], "Some(\"src\")");
}

#[test]
fn a_prefix_and_a_symbol_come_before_the_indent_and_the_arrow_still_toggles() {
    let app = App::new(|| {
        let items = || {
            vec![TreeItem::keyed("src", "src")
                .prefix("  ")
                .child(TreeItem::keyed("src/lib.rs", "lib.rs").prefix("M "))]
        };
        tree_keyed(items, signal(None), signal(HashSet::new())).symbol("> ")
    });
    let mut driver = app.driver(20, 2);
    driver.settle();
    assert_eq!(plain(&driver)[0], ">   ▸ src");
    // The arrow is after the symbol and the prefix: four cells in.
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        4,
        0,
    )));
    driver.settle();
    assert_eq!(plain(&driver), [">   ▾ src", "  M     lib.rs"]);
}

#[test]
fn keys_the_tree_does_not_take_reach_its_ancestors() {
    let app = App::new(|| {
        let heard = signal(String::new());
        let items = || folders(&[("src", vec!["lib.rs"])]);
        let keys = TreeKeys::default().no_open_close().no_toggle();
        column([
            tree_keyed(items, signal(None), signal(HashSet::new()))
                .tree_keys(keys)
                .fixed(2),
            text!("{heard}"),
        ])
        .on_key("right", move |_| heard.update(|h| h.push('>')))
        .on_key("space", move |_| heard.update(|h| h.push('_')))
    });
    let mut driver = app.driver(16, 3);
    driver.press("right space");
    assert_eq!(plain(&driver)[0], "▸ src");
    assert_eq!(plain(&driver)[2], ">_");
}

#[test]
fn a_reloaded_lazy_level_shows_what_it_holds_now() {
    use std::sync::{Arc, Mutex};

    use intuituive::widgets::{tree_lazy_with, LazyItem, LazyTree};

    let files = Arc::new(Mutex::new(vec!["a.txt", "b.txt"]));
    let disk = files.clone();
    let app = App::new(move || {
        let loads = LazyTree::new();
        let disk = disk.clone();
        let children = move |key: String| -> Result<Vec<LazyItem>, String> {
            let files = disk.lock().unwrap().clone();
            Ok(files
                .into_iter()
                .map(|f| LazyItem::leaf(format!("{key}/{f}"), f))
                .collect())
        };
        let roots = || vec![LazyItem::branch("/d", "d")];
        tree_lazy_with(roots, children, signal(None), loads)
            .on_key("r", move |_| loads.reload("/d"))
    });
    let mut driver = app.wait_for_tasks(true).driver(16, 3);
    driver.press("right");
    assert_eq!(plain(&driver), ["▾ d", "    a.txt", "    b.txt"]);
    files.lock().unwrap().remove(0);
    driver.press("r");
    assert_eq!(plain(&driver), ["▾ d", "    b.txt", ""]);
}
