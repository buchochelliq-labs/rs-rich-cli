//! Overlays and chrome (0.0.14 workstream 2), headless: the palette, the
//! help overlay and the shortcut sheet over every built-in component and
//! over the custom component of workstream 1's example; region actions in
//! a modal menu; the status bar and breadcrumbs round a component.

#[allow(dead_code)]
#[path = "../examples/custom_component.rs"]
mod custom;
#[allow(dead_code)]
#[path = "../examples/overlays.rs"]
mod example;

use std::cell::RefCell;
use std::fmt::Debug;
use std::rc::Rc;
use std::time::Duration;

use rich_interact::chrome::{Breadcrumbs, StatusBar, StatusItem};
use rich_interact::compose::{Column, ComponentExt, Label};
use rich_interact::headless::{self, Script};
use rich_interact::keymap::keys;
use rich_interact::overlay::{Command, Overlays};
use rich_interact::{
    Action, ActionTarget, Actions, AssetKind, AssetPicker, ColorPicker, Component, Confirm,
    FilePicker, Flow, Form, Input, Item, MultiSelect, Outcome, Pager, Select, TableSelect,
    TargetKind, TextArea, TreeSelect,
};

fn opened(record: &headless::Record, title: &str) -> bool {
    let top = format!("╭─ {title} ");
    record.frames.iter().any(|frame| frame.contains(&top))
}

/// Open each overlay over what `make` makes, and close it again: the
/// palette lists the component's keys, the help its first binding, and the
/// sheet opens and closes on any key.
fn every_overlay_over<C>(name: &str, make: impl Fn() -> C)
where
    C: Component,
    C::Output: Debug,
{
    let first = make()
        .keymap()
        .bindings()
        .into_iter()
        .find(|binding| !binding.keys.is_empty())
        .map(|binding| binding.description);
    for (keys, title) in [("ctrl+o", "Commands"), ("f1", "Keys"), ("f2", "Shortcuts")] {
        let script = Script::new().keys(keys).keys("escape");
        let (_, record) = headless::run(Overlays::new(make()), script, 100, 30);
        assert!(
            opened(&record, title),
            "{title} did not open over {name}:\n{}",
            record.frames.join("\n----\n")
        );
        if let (Some(first), "Keys") = (&first, title) {
            assert!(
                record
                    .frames
                    .iter()
                    .any(|frame| frame.contains(first.as_str())),
                "help over {name} lacks {first:?}:\n{}",
                record.frames.join("\n----\n")
            );
        }
        // Escape closed it: the last frame has no box.
        assert!(
            !record.last_frame().contains(&format!("╭─ {title} ")),
            "{title} stayed open over {name}:\n{}",
            record.last_frame()
        );
    }
}

fn files() -> Vec<Item<&'static str>> {
    ["src/lib.rs", "src/main.rs", "README.md"]
        .into_iter()
        .map(Item::from)
        .collect()
}

#[test]
fn every_overlay_opens_over_every_built_in() {
    every_overlay_over("select", || Select::new("File", files()));
    every_overlay_over("multi-select", || MultiSelect::new("Files", files()));
    every_overlay_over("table", || {
        TableSelect::new(
            "Service",
            ["Name", "State"],
            vec![
                (Item::new(1, "web"), vec!["web".to_string(), "up".into()]),
                (Item::new(2, "db"), vec!["db".to_string(), "down".into()]),
            ],
        )
    });
    every_overlay_over("tree", || {
        TreeSelect::new(
            "Files",
            vec![
                (0, Item::new("src", "src")),
                (1, Item::new("lib", "lib.rs")),
            ],
        )
    });
    every_overlay_over("input", || Input::new("Name"));
    every_overlay_over("text area", || TextArea::new("Notes"));
    every_overlay_over("confirm", || Confirm::new("Deploy?"));
    every_overlay_over("form", || {
        Form::new("Service")
            .text("name", "Name")
            .toggle("tls", "TLS", true)
    });
    every_overlay_over("pager", || {
        Pager::new(rich::Text::new("first line\nsecond line"))
    });
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    every_overlay_over("file picker", || FilePicker::new("File", dir.path()));
    every_overlay_over("colour picker", || ColorPicker::new("Colour"));
    every_overlay_over("asset picker", || {
        AssetPicker::new("Emoji", AssetKind::Emoji)
    });
}

#[test]
fn every_overlay_opens_over_the_custom_component() {
    every_overlay_over("checklist", || {
        custom::Checklist::new("Todo", ["write", "test", "ship"])
    });
    // The whole example app has an F1 dialog of its own, which wins: the
    // component comes first. The palette and the sheet still open.
    let script = Script::new().keys("ctrl+o escape f2 escape");
    let (_, record) = headless::run(Overlays::new(custom::app()), script, 90, 24);
    assert!(opened(&record, "Commands"));
    assert!(opened(&record, "Shortcuts"));
    let script = Script::new().keys("f1");
    let (_, record) = headless::run(Overlays::new(custom::app()), script, 90, 24);
    assert!(opened(&record, "Keys"));
    assert!(record.last_frame().contains("tick or untick"));
}

#[test]
fn a_palette_command_runs_on_the_custom_component() {
    // "tick" finds the checklist's binding; Enter sends its key (Space).
    let checklist = custom::Checklist::new("Todo", ["write", "test", "ship"]);
    let script = Script::new()
        .keys("down ctrl+o")
        .text("tick")
        .keys("enter enter");
    let (outcome, record) = headless::run(Overlays::new(checklist), script, 80, 20);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["test".to_string()]));
    let palette = record
        .frames
        .iter()
        .find(|frame| frame.contains("tick or untick"))
        .expect("the palette listed the binding");
    // Its shortcut, read from the keymap, and its category.
    assert!(palette.contains("checklist"), "{palette}");
    assert!(palette.contains("space"), "{palette}");
    // The same, composed in the example's tabs.
    let script = Script::new()
        .keys("ctrl+o")
        .text("tick")
        .keys("enter enter");
    let (outcome, _) = headless::run(Overlays::new(custom::app()), script, 90, 24);
    assert_eq!(
        outcome.unwrap(),
        Outcome::Done(custom::App::Checked(vec!["bump versions".into()]))
    );
}

#[test]
fn the_palette_lists_only_active_contexts_and_runs_commands() {
    let ran = Rc::new(RefCell::new(0));
    let count = Rc::clone(&ran);
    let app = Overlays::new(Select::new("File", files()))
        .command(
            Command::new("app.count", "count something").category("app"),
            move || {
                *count.borrow_mut() += 1;
                Flow::Continue
            },
        )
        .command(
            Command::new("editor.save", "save the buffer").context("editor"),
            || Flow::Cancel,
        );
    let palette = app.palette();
    let ids: Vec<&str> = palette.matches().iter().map(|c| c.id.as_str()).collect();
    assert!(ids.contains(&"select.down"), "{ids:?}");
    assert!(ids.contains(&"app.count"), "{ids:?}");
    assert!(ids.contains(&"overlays.help"), "{ids:?}");
    // Not the palette itself, and nothing for a context not active here.
    assert!(!ids.contains(&"overlays.palette"), "{ids:?}");
    assert!(!ids.contains(&"editor.save"), "{ids:?}");
    // Fuzzy: "cnt" finds "count something"; Enter runs its handler.
    let script = Script::new()
        .keys("ctrl+o")
        .text("cnt")
        .keys("enter ctrl+o")
        .text("cnt")
        .keys("enter enter");
    let (outcome, record) = headless::run(app, script, 80, 20);
    assert_eq!(outcome.unwrap(), Outcome::Done("src/lib.rs"));
    assert_eq!(*ran.borrow(), 2);
    assert!(record
        .frames
        .iter()
        .any(|f| f.contains("app") && f.contains("count something")));
}

#[test]
fn shortcuts_follow_rebinding() {
    // The palette shows the key the select has now, and running it sends
    // that key.
    let select = Select::new("File", files()).rebind("down", keys("ctrl+j"));
    let script = Script::new()
        .keys("ctrl+o")
        .text("move down")
        .keys("enter enter");
    let (outcome, record) = headless::run(Overlays::new(select), script, 80, 20);
    assert_eq!(outcome.unwrap(), Outcome::Done("src/main.rs"));
    assert!(record
        .frames
        .iter()
        .any(|f| f.contains("move down") && f.contains("ctrl+j")));
    // The overlays' own keys rebind too.
    let app = Overlays::new(Select::new("File", files())).rebind("help", keys("f5"));
    let (_, record) = headless::run(app, Script::new().keys("f1 f5"), 80, 20);
    assert!(opened(&record, "Keys"));
    assert_eq!(
        record
            .frames
            .iter()
            .filter(|f| f.contains("╭─ Keys "))
            .count(),
        1
    );
}

#[test]
fn the_help_overlay_searches() {
    let script = Script::new().keys("f1").text("mark");
    let (_, record) = headless::run(
        Overlays::new(MultiSelect::new("Files", files())),
        script,
        90,
        24,
    );
    let last = record.last_frame();
    assert!(last.contains("Search keys › mark"), "{last}");
    assert!(last.contains("mark and move down"), "{last}");
    assert!(!last.contains("cancel"), "{last}");
    // Escape clears the search first, then closes.
    let script = Script::new().keys("f1").text("mark").keys("escape");
    let (_, record) = headless::run(
        Overlays::new(MultiSelect::new("Files", files())),
        script,
        90,
        24,
    );
    assert!(record.last_frame().contains("cancel"));
    assert!(record.last_frame().contains("╭─ Keys "));
}

#[test]
fn a_question_mark_types_where_it_is_used_and_opens_the_sheet_where_not() {
    // The select filters with it...
    let (_, record) = headless::run(
        Overlays::new(Select::new("File", files())),
        Script::new().keys("?"),
        80,
        20,
    );
    assert!(!opened(&record, "Shortcuts"));
    assert!(record.last_frame().contains("File › ?"));
    // ... the confirmation does not use it.
    let (_, record) = headless::run(
        Overlays::new(Confirm::new("Deploy?")),
        Script::new().keys("?"),
        80,
        20,
    );
    assert!(opened(&record, "Shortcuts"));
}

#[test]
fn region_actions_open_in_a_modal() {
    let seen = Rc::new(RefCell::new(Vec::<(String, ActionTarget)>::new()));
    let log = Rc::clone(&seen);
    let actions = Actions::new()
        .action(Action::menu("refresh", "Refresh"))
        .action_if(Action::menu("files-only", "Only on files"), |target| {
            target.kind == TargetKind::File
        });
    let app = Overlays::new(Select::new("File", files()))
        .region("Files", "file-list")
        .actions(actions)
        .on_action(move |action, target| {
            log.borrow_mut().push((action.id.clone(), target.clone()));
            Flow::Continue
        });
    let (outcome, record) = headless::run(app, Script::new().keys("ctrl+k enter enter"), 80, 20);
    assert_eq!(outcome.unwrap(), Outcome::Done("src/lib.rs"));
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "refresh");
    assert_eq!(seen[0].1.kind, TargetKind::Region);
    assert_eq!(seen[0].1.value, "file-list");
    let menu = record
        .frames
        .iter()
        .find(|f| f.contains("╭─ Actions · Files "))
        .expect("the region's menu opened");
    assert!(menu.contains("Refresh"), "{menu}");
    assert!(!menu.contains("Only on files"), "{menu}");
    // An item with actions of its own keeps Ctrl+K: its menu opens instead.
    let items = files()
        .into_iter()
        .map(|item| item.action(Action::menu("open", "Open")));
    let mut region = Overlays::new(Select::new("File", items))
        .region("Files", "file-list")
        .actions(Actions::new().action(Action::menu("refresh", "Refresh")));
    let (_, record) = headless::run(&mut region, Script::new().keys("ctrl+k"), 80, 20);
    assert!(record.last_frame().contains("╭─ Actions · src/lib.rs "));
    assert_eq!(region.action(), None);
}

#[test]
fn a_status_bar_and_breadcrumbs_round_a_component() {
    let bar = StatusBar::new()
        .left("mode", StatusItem::badge("FILES", "bold"))
        .left("work", StatusItem::spinner("line", "indexing"))
        .right("keys", StatusItem::hints(2))
        .clock(|| Duration::from_millis(130));
    let status = bar.handle();
    let crumbs = Breadcrumbs::new(["home", "project"]).with_mouse(true);
    let path = crumbs.crumbs();
    let app = Overlays::new(Select::new("File", files()).height(3))
        .status_bar(bar)
        .breadcrumbs(crumbs)
        .command(Command::new("app.done", "finish indexing"), move || {
            status.set("work", StatusItem::text("[green]indexed[/]"));
            Flow::Continue
        });
    let script = Script::new()
        .click(1, 0)
        .keys("ctrl+o")
        .text("finish")
        .keys("enter escape");
    let (outcome, record) = headless::run(app, script, 60, 12);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    let first = &record.frames[0];
    let rows: Vec<&str> = first.lines().collect();
    assert_eq!(rows[0], "home › project");
    assert!(rows[1].starts_with("? File ›"), "{first}");
    let bottom = rows.last().unwrap().trim_end();
    // The spinner at 130ms, and the select's first keys as hints.
    assert!(bottom.starts_with(" FILES  │ \\ indexing"), "{first}");
    assert!(bottom.ends_with("enter pick · esc cancel"), "{first}");
    // A click on `home` went back to it.
    assert_eq!(path.get(), ["home"]);
    // While the palette is open the hints are the palette's.
    let open = record
        .frames
        .iter()
        .find(|f| f.contains("╭─ Commands "))
        .unwrap();
    assert!(
        open.lines()
            .last()
            .unwrap()
            .trim_end()
            .ends_with("↑ move up · ↓ move down"),
        "{open}"
    );
    // The command changed the bar.
    assert!(record.frames.iter().any(|f| f.contains("indexed")));
}

#[test]
fn chrome_sits_in_containers_and_the_status_bar_ticks() {
    // A clock a frame on at every look, as if time passed between paints.
    let calls = std::cell::Cell::new(0u64);
    let bar: StatusBar<()> = StatusBar::new()
        .left("work", StatusItem::spinner("line", "busy"))
        .clock(move || {
            calls.set(calls.get() + 1);
            Duration::from_millis(130 * calls.get())
        });
    assert_eq!(bar.tick(), Some(Duration::from_millis(130)));
    let column = Column::new()
        .child(Breadcrumbs::new(["a", "b"]))
        .child(Label::new("body"))
        .child(bar);
    let (_, record) = headless::run(
        column,
        Script::new().wait(Duration::from_millis(300)),
        40,
        6,
    );
    assert!(record.frames.len() >= 2, "{:?}", record.frames);
    assert!(
        record.frames[0].starts_with("a › b\nbody\n"),
        "{}",
        record.frames[0]
    );
    // Each tick painted the spinner's next frame.
    let spinners: Vec<&str> = record
        .frames
        .iter()
        .filter_map(|f| f.lines().nth(2))
        .collect();
    assert!(spinners.len() >= 2, "{spinners:?}");
    assert!(spinners.iter().all(|line| line.contains(" busy")));
    assert_ne!(spinners[0], spinners[1]);
}

#[test]
fn the_example_opens_every_overlay() {
    let script = Script::new()
        .keys("ctrl+o")
        .text("refresh")
        .keys("enter ctrl+k down enter f1 escape f2 escape down enter");
    let (outcome, record) = headless::run(example::app(), script, 100, 26);
    assert_eq!(outcome.unwrap(), Outcome::Done("src/overlay.rs"));
    for title in ["Commands", "Actions · Files", "Keys", "Shortcuts"] {
        assert!(opened(&record, title), "{title}");
    }
    assert!(record.frames.iter().any(|f| f.contains("refreshed")));
    assert!(record
        .frames
        .iter()
        .any(|f| f.contains("Show hidden files · Files")));
    // The quit command cancels.
    let script = Script::new().keys("ctrl+o").text("quit").keys("enter");
    let (outcome, _) = headless::run(example::app(), script, 100, 26);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
}

#[test]
fn with_overlays_wraps_any_component() {
    let app = Confirm::new("Deploy?").with_overlays();
    let (outcome, record) = headless::run(app, Script::new().keys("f1 escape y"), 60, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done("yes".to_string()));
    assert!(opened(&record, "Keys"));
}
