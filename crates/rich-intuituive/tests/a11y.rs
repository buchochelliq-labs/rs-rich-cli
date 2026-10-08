//! Accessibility: roles and names in the accessibility tree, the cursor on
//! what has the focus, text mode, and announcements.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use intuituive::a11y::{Announcement, Role};
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
