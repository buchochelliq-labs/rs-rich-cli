//! Accessibility: roles, names and states in the accessibility tree, hidden
//! nodes, the cursor on what has the focus, text mode, and announcements.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use intuituive::a11y::{AccessNode, AccessState, Announcement, Role};
use intuituive::interact::{Event, Key};
use intuituive::prelude::*;
use intuituive::widgets::{table, tabs, tree, Column, TreeItem};

fn frame(driver: &mut intuituive::Driver, event: Option<Event>) -> String {
    if let Some(event) = event {
        driver.event(event);
    }
    driver.update(Duration::ZERO);
    driver.render().unwrap_or_default()
}

fn press(key: &str) -> Option<Event> {
    Some(Event::Key(Key::parse(key).expect("a key")))
}

fn rows(driver: &intuituive::Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .into_iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn app() -> App {
    App::new(|| {
        let picked = signal(1usize);
        let tab = signal(0usize);
        column([
            label("Report").label("Title").fixed(1),
            tabs(|| vec!["Files".into(), "Log".into()], tab).fixed(1),
            list(
                || vec!["alpha".into(), "beta".into(), "gamma".into()],
                picked,
            )
            .panel("Files")
            .fixed(5),
            label("Save").on_click(|cx| cx.toast("saved")).fixed(1),
        ])
    })
}

#[test]
fn the_tree_has_roles_names_and_what_is_selected() {
    let mut driver = app().driver(30, 8);
    frame(&mut driver, None);
    let tree = driver.accessibility();
    let summary: Vec<(usize, Role, String, Option<String>, bool)> = tree
        .iter()
        .map(|n| (n.depth, n.role, n.name.clone(), n.value.clone(), n.focused))
        .collect();
    assert_eq!(
        summary,
        vec![
            (0, Role::Text, "Title".into(), None, false),
            (0, Role::TabList, String::new(), Some("Files".into()), true),
            (0, Role::Region, "Files".into(), None, false),
            (1, Role::List, String::new(), Some("beta".into()), false),
            (0, Role::Button, "Save".into(), None, false),
        ],
        "{tree:#?}"
    );
}

#[test]
fn accessible_mode_draws_text_and_puts_the_cursor_on_the_selection() {
    let mut driver = app().accessible(true).driver(30, 8);
    let out = frame(&mut driver, None);
    let screen = rows(&driver);
    // No box: the title stays, the lines go.
    assert_eq!(screen[2].trim(), "Files", "{screen:?}");
    assert!(!screen.iter().any(|r| r.contains('─') || r.contains('│')));
    // Markers on the selected tab and row.
    assert!(screen[1].starts_with(">Files"), "{screen:?}");
    assert_eq!(screen[4].trim(), "> beta", "{screen:?}");
    assert!(screen[3].trim() == "alpha", "{screen:?}");
    // No colour at all: the only style sequence is a reset.
    let styles: Vec<&str> = out
        .split("\x1b[")
        .skip(1)
        .filter_map(|s| s.find('m').map(|end| &s[..end]))
        .filter(|s| s.chars().all(|c| c.is_ascii_digit() || c == ';'))
        .collect();
    assert!(styles.iter().all(|s| *s == "0"), "{styles:?}");
    // The focused tab strip: the cursor on its selected title, shown.
    assert!(out.ends_with("\x1b[2;1H\x1b[?25h"), "{out:?}");
    // Tab to the list: the cursor goes to its selected row.
    let out = frame(&mut driver, press("tab"));
    assert!(out.ends_with("\x1b[5;2H\x1b[?25h"), "{out:?}");
    let out = frame(&mut driver, press("down"));
    assert!(out.ends_with("\x1b[6;2H\x1b[?25h"), "{out:?}");
    assert_eq!(rows(&driver)[5].trim(), "> gamma");
}

#[test]
fn tables_and_trees_mark_the_selected_row_in_text_mode() {
    let app = App::new(|| {
        column([
            table(
                vec![
                    Column::new("Name", Size::Auto),
                    Column::new("Size", Size::Auto),
                ],
                || vec![vec!["a".into(), "1".into()], vec!["b".into(), "2".into()]],
                signal(1usize),
            )
            .fixed(3),
            tree(
                || vec![TreeItem::new("src"), TreeItem::new("docs")],
                signal(vec![0usize]),
            ),
        ])
    })
    .accessible(true);
    let mut driver = app.driver(20, 6);
    frame(&mut driver, None);
    let screen = rows(&driver);
    assert_eq!(screen[0], "  Name Size", "{screen:?}");
    assert_eq!(screen[1], "  a    1", "{screen:?}");
    assert_eq!(screen[2], "> b    2", "{screen:?}");
    assert_eq!(screen[3], ">   src", "{screen:?}");
    assert_eq!(screen[4], "    docs", "{screen:?}");
}

#[test]
fn toasts_dialogs_live_nodes_and_handlers_are_announced() {
    let heard = Rc::new(RefCell::new(Vec::<Announcement>::new()));
    let sink = heard.clone();
    let app = App::new(|| {
        let count = signal(0);
        column([text!("count {count}").live(), label("x")])
            .on_key("t", |cx| cx.toast("[bold]Saved[/] the file"))
            .on_key("+", move |_| count.update(|c| *c += 1))
            .on_key("a", |cx| cx.announce("quietly", false))
            .on_key("m", |cx| {
                cx.modal(Size::Fixed(20), Size::Fixed(3), || label("Delete it?"))
            })
    })
    .announcer(move |a: &Announcement| sink.borrow_mut().push(a.clone()));
    let mut driver = app.driver(40, 8);
    frame(&mut driver, None);
    assert!(driver.take_announcements().is_empty(), "nothing yet");
    frame(&mut driver, press("t"));
    frame(&mut driver, press("+"));
    frame(&mut driver, press("a"));
    frame(&mut driver, press("m"));
    let said: Vec<(String, bool)> = driver
        .take_announcements()
        .into_iter()
        .map(|a| (a.text, a.urgent))
        .collect();
    assert_eq!(
        said,
        vec![
            ("Saved the file".into(), false),
            ("count 1".into(), false),
            ("quietly".into(), false),
            ("Dialog: Delete it?".into(), true),
        ]
    );
    // The announcer heard the same, as they happened.
    assert_eq!(heard.borrow().len(), 4);
    assert!(driver.take_announcements().is_empty());
}

#[test]
fn animations_jump_to_their_end_when_accessible() {
    let app = App::new(|| {
        let x = signal(0.0f64);
        text(move || format!("{:.0}", x.get())).on_key("g", move |cx| {
            cx.animate(x, 10.0, Duration::from_secs(5), intuituive::Easing::Linear)
        })
    })
    .accessible(true);
    let mut driver = app.driver(10, 1);
    frame(&mut driver, None);
    driver.event(Event::Key(Key::char('g')));
    frame(&mut driver, None);
    assert_eq!(rows(&driver)[0], "10", "at its end at once");
}

#[test]
fn roles_and_labels_set_in_code_win() {
    let app = App::new(|| {
        column([
            row([label("A"), label("B")])
                .role(Role::MenuBar)
                .label("Tools"),
            label("status").live(),
        ])
    });
    let mut driver = app.driver(10, 2);
    frame(&mut driver, None);
    let tree = driver.accessibility();
    assert_eq!(
        (tree[0].role, tree[0].name.as_str()),
        (Role::MenuBar, "Tools")
    );
    assert_eq!(
        (tree[3].role, tree[3].name.as_str()),
        (Role::Status, "status")
    );
}

#[test]
fn names_after_a_wide_character_keep_to_their_cells() {
    let app = App::new(|| row([label("界").fixed(2), label("Save").fixed(4)]));
    let mut driver = app.driver(6, 1);
    frame(&mut driver, None);
    let names: Vec<String> = driver
        .accessibility()
        .iter()
        .map(|n| n.name.clone())
        .collect();
    assert_eq!(names, ["界", "Save"], "{names:?}");
}

/// The first node of the accessibility tree with `role`.
fn node_of(driver: &intuituive::Driver, role: Role) -> AccessNode {
    let tree = driver.accessibility();
    tree.iter()
        .find(|n| n.role == role)
        .cloned()
        .unwrap_or_else(|| panic!("a {role:?} in {tree:#?}"))
}

#[test]
fn lists_tables_and_tabs_say_which_item_is_selected_and_where() {
    let app = App::new(|| {
        column([
            tabs(|| vec!["A".into(), "B".into(), "C".into()], signal(1)).fixed(1),
            list(|| vec!["x".into(), "y".into()], signal(0)).fixed(2),
            table(
                vec![Column::new("N", Size::Auto)],
                || vec![vec!["1".into()], vec!["2".into()], vec!["3".into()]],
                signal(2usize),
            )
            .fixed(4),
        ])
    });
    let mut driver = app.driver(20, 8);
    frame(&mut driver, None);
    let tabs = node_of(&driver, Role::TabList);
    assert_eq!(tabs.state, AccessState::item(1, 3));
    assert!(tabs.state.selected);
    assert_eq!(tabs.state.position, Some((2, 3)));
    assert_eq!(tabs.describe(), "tab list, 2 of 3: B, selected");
    let list = node_of(&driver, Role::List);
    assert_eq!(list.describe(), "list, 1 of 2: x, selected");
    let table = node_of(&driver, Role::Table);
    assert_eq!(table.state.position, Some((3, 3)));
    // The list's selection moves: its place follows.
    frame(&mut driver, press("tab"));
    frame(&mut driver, press("down"));
    assert_eq!(node_of(&driver, Role::List).state.position, Some((2, 2)));
}

#[test]
fn tree_items_are_expanded_or_collapsed() {
    let app = App::new(|| {
        let items = || {
            vec![
                TreeItem::new("src").child(TreeItem::new("main.rs")),
                TreeItem::new("README"),
            ]
        };
        tree(items, signal(vec![0usize])).label("Files")
    });
    let mut driver = app.driver(20, 4);
    frame(&mut driver, None);
    let tree = node_of(&driver, Role::Tree);
    assert_eq!(tree.state.expanded, Some(false));
    assert_eq!(
        tree.describe(),
        "Files, tree, 1 of 2: src, selected, collapsed"
    );
    frame(&mut driver, press("right"));
    let tree = node_of(&driver, Role::Tree);
    assert_eq!(tree.state.expanded, Some(true));
    assert_eq!(tree.state.position, Some((1, 3)));
    // A leaf neither opens nor closes.
    frame(&mut driver, press("down"));
    let tree = node_of(&driver, Role::Tree);
    assert_eq!(tree.state.expanded, None);
    assert_eq!(tree.describe(), "Files, tree, 2 of 3: main.rs, selected");
}

#[test]
fn a_lazy_tree_is_busy_while_a_level_loads() {
    use std::sync::{mpsc, Arc, Mutex};

    use intuituive::widgets::{tree_lazy, LazyItem};

    let (send, wait) = mpsc::channel::<()>();
    let wait = Arc::new(Mutex::new(wait));
    let app = App::new(move || {
        let children = move |key: String| -> Result<Vec<LazyItem>, String> {
            let _ = wait.lock().expect("the lock").recv();
            Ok(vec![LazyItem::leaf(format!("{key}/a"), "a")])
        };
        let roots = || vec![LazyItem::branch("/src", "src")];
        tree_lazy(roots, children, signal(None))
    });
    let mut driver = app.driver(20, 4);
    frame(&mut driver, None);
    assert!(!node_of(&driver, Role::Tree).state.busy);
    frame(&mut driver, press("right"));
    assert!(node_of(&driver, Role::Tree).state.busy, "loading");
    send.send(()).expect("the loader waits");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while node_of(&driver, Role::Tree).state.busy {
        assert!(std::time::Instant::now() < deadline, "the level loads");
        std::thread::sleep(Duration::from_millis(2));
        frame(&mut driver, None);
    }
    assert_eq!(node_of(&driver, Role::Tree).state.expanded, Some(true));
}

#[test]
fn menus_count_their_items_without_separators() {
    use intuituive::menu::{menu_bar, Menu, MenuItem};

    let app = App::new(|| {
        let item = |name: &str| MenuItem::new(name, |_| {});
        menu_bar(vec![
            Menu::new(
                "File",
                vec![item("New"), MenuItem::separator(), item("Open")],
            ),
            Menu::new("Edit", vec![item("Copy")]),
        ])
    })
    .accessible(true);
    let mut driver = app.driver(30, 8);
    frame(&mut driver, None);
    let bar = node_of(&driver, Role::MenuBar);
    assert_eq!(bar.describe(), "menu bar, 1 of 2: File, selected");
    frame(&mut driver, press("enter"));
    frame(&mut driver, press("down"));
    let menu = node_of(&driver, Role::Menu);
    assert_eq!(menu.state.position, Some((2, 2)), "{menu:?}");
    assert_eq!(menu.value.as_deref(), Some("Open"));
    // In text mode the separator is blank.
    let screen = rows(&driver);
    assert!(!screen.iter().any(|r| r.contains('─')), "{screen:?}");
}

#[test]
fn a_calendar_puts_the_cursor_on_the_selected_day() {
    use intuituive::widgets::{calendar, Date};

    let app = App::new(|| calendar(signal(Date::new(2026, 10, 8))).label("Due")).accessible(true);
    let mut driver = app.driver(24, 9);
    let out = frame(&mut driver, None);
    assert_eq!(
        node_of(&driver, Role::Grid).describe(),
        "Due, grid: 8, selected"
    );
    // October 2026 starts on a Thursday: the 8th is the second Thursday.
    assert!(out.ends_with("\x1b[4;10H\x1b[?25h"), "{out:?}");
}

#[test]
fn an_input_is_a_text_box() {
    use intuituive::interact::Input;

    let app = App::new(|| component(Input::new("Find"), |_: String, _| {}).label("Search"));
    let mut driver = app.driver(20, 2);
    frame(&mut driver, None);
    for key in ["f", "o", "o"] {
        frame(&mut driver, press(key));
    }
    let input = node_of(&driver, Role::TextBox);
    let value = input.value.clone().unwrap_or_default();
    assert!(value.ends_with("foo"), "{input:?}");
    assert!(
        input.describe().starts_with("Search, text box: "),
        "{input:?}"
    );
}

#[test]
fn states_set_in_code_reach_the_tree() {
    let app = App::new(|| {
        let on = signal(false);
        column([
            text(move || format!("[{}] Wrap", if on.get() { "x" } else { " " }))
                .label("Wrap")
                .checked_when(move || on.get())
                .focusable()
                .on_key("space", move |_| on.update(|on| *on = !*on)),
            label("Details").role(Role::Button).expanded_when(|| true),
            label("Sync").live().busy_when(|| true),
            label("Dark").role(Role::Switch).checked_when(|| true),
            label("Today").selected_when(|| true),
        ])
    });
    let mut driver = app.driver(20, 5);
    frame(&mut driver, None);
    let lines: Vec<String> = driver
        .accessibility()
        .iter()
        .map(|n| n.describe())
        .collect();
    assert_eq!(
        lines,
        [
            "Wrap, check box, not checked",
            "Details, button, expanded",
            "Sync, busy",
            "Dark, switch, checked",
            "Today, selected",
        ]
    );
    frame(&mut driver, press("space"));
    let wrap = &driver.accessibility()[0];
    assert_eq!(wrap.state.checked, Some(true));
    assert_eq!(
        wrap.aria_attributes(),
        [
            ("role", "checkbox".to_string()),
            ("aria-label", "Wrap".to_string()),
            ("aria-checked", "true".to_string()),
        ]
    );
}

#[test]
fn a_hidden_node_and_its_subtree_leave_the_tree_but_are_drawn() {
    let heard = Rc::new(RefCell::new(Vec::<Announcement>::new()));
    let sink = heard.clone();
    let app = App::new(|| {
        let n = signal(0);
        column([
            label("*** LOGO ***").access_hidden(true).fixed(1),
            row([label("a").fixed(1), label("b").fixed(1)])
                .label("Decor")
                .access_hidden(true)
                .fixed(1),
            row([label("@").access_hidden(true).fixed(2), text!("{n} loaded")])
                .live()
                .fixed(1),
            text!("ghost {n}").live().access_hidden(true).fixed(1),
            label("Save").on_click(|_| {}).fixed(1),
        ])
        .on_key("+", move |_| n.update(|n| *n += 1))
        .on_key("m", |cx| {
            cx.modal(Size::Fixed(20), Size::Fixed(3), || {
                row([label("!").access_hidden(true).fixed(2), label("Sure?")])
            })
        })
    })
    .announcer(move |a: &Announcement| sink.borrow_mut().push(a.clone()));
    let mut driver = app.driver(20, 8);
    frame(&mut driver, None);
    // Drawn as ever.
    let screen = rows(&driver);
    assert_eq!(
        screen[..3],
        ["*** LOGO ***", "ab", "@ 0 loaded"],
        "{screen:?}"
    );
    let tree: Vec<(Role, String)> = driver
        .accessibility()
        .into_iter()
        .map(|n| (n.role, n.name))
        .collect();
    assert_eq!(
        tree,
        [
            (Role::Status, "0 loaded".into()),
            (Role::Text, "0 loaded".into()),
            (Role::Button, "Save".into()),
        ],
        "the logo, the decor and what is in it, and the hidden live line are left out"
    );
    frame(&mut driver, press("+"));
    frame(&mut driver, press("m"));
    let said: Vec<String> = heard.borrow().iter().map(|a| a.text.clone()).collect();
    assert_eq!(said, ["1 loaded", "Dialog: Sure?"]);
}

#[test]
fn accessible_frames_hide_the_cursor_while_they_are_written() {
    let app = App::new(|| {
        let picked = signal(0usize);
        let ticks = signal(0);
        column([
            text!("tick {ticks}").fixed(1),
            list(|| vec!["a".into(), "b".into(), "c".into()], picked).fixed(3),
        ])
        .on_key("t", move |_| ticks.update(|t| *t += 1))
        // Draws again, the same.
        .on_key("x", move |_| ticks.update(|_| {}))
    })
    .accessible(true);
    let mut driver = app.driver(20, 4);
    let out = frame(&mut driver, None);
    assert!(out.starts_with("\x1b[?25l"), "{out:?}");
    assert!(out.ends_with("\x1b[2;1H\x1b[?25h"), "{out:?}");
    // A change away from the focus: the cursor is hidden before any cell
    // is written, and shown once it is back on the focus.
    let out = frame(&mut driver, press("t"));
    assert!(out.starts_with("\x1b[?25l"), "hidden first: {out:?}");
    assert!(out.contains("\x1b[1;6H\x1b[0m1"), "{out:?}");
    assert!(out.ends_with("\x1b[2;1H\x1b[?25h"), "{out:?}");
    assert_eq!(out.matches("\x1b[?25h").count(), 1, "{out:?}");
    // The selection moves: hidden, the rows written, parked on the new one.
    let out = frame(&mut driver, press("down"));
    assert!(out.starts_with("\x1b[?25l"), "{out:?}");
    assert!(out.ends_with("\x1b[3;1H\x1b[?25h"), "{out:?}");
    // A frame that changes nothing on the screen sends nothing: the
    // cursor stays where it is.
    driver.event(Event::Key(Key::char('x')));
    driver.update(Duration::ZERO);
    assert_eq!(driver.render(), Some(String::new()));
}

#[test]
fn frames_that_are_not_accessible_leave_the_cursor_alone() {
    let app = App::new(|| {
        let ticks = signal(0);
        text!("tick {ticks}").on_key("t", move |_| ticks.update(|t| *t += 1))
    });
    let mut driver = app.driver(10, 1);
    frame(&mut driver, None);
    let out = frame(&mut driver, press("t"));
    assert!(!out.contains("\x1b[?25"), "{out:?}");
}
