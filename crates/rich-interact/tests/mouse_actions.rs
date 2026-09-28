//! Mouse support (#476) and custom actions (#491), headless: scripted
//! clicks, drags and wheels, action menus on lists, tables, trees and
//! files, and actions plugins register.

use std::sync::Arc;

use rich::{Segment, Style};
use rich_interact::headless::{self, Script};
use rich_interact::{
    Action, ActionTarget, Actions, Choice, Component, Confirm, Context, Event, FilePicker, Flow,
    Form, Item, Key, MouseKind, Outcome, Preview, Select, TableSelect, TargetKind, TreeSelect,
    View,
};

fn before_answer(record: &headless::Record) -> &str {
    &record.frames[record.frames.len() - 2]
}

fn fruit() -> Vec<Item<&'static str>> {
    ["apple", "banana", "cherry", "damson"]
        .into_iter()
        .map(Item::from)
        .collect()
}

// ---- Mouse ----

#[test]
fn a_click_focuses_a_row_and_a_second_click_picks_it() {
    // Row 0 is the question; rows 1 to 4 the items.
    let script = Script::new().click(4, 3).click(4, 3);
    let (outcome, record) = headless::run(
        Select::new("Fruit", fruit()).with_mouse(true),
        script,
        40,
        10,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("cherry"));
    assert!(before_answer(&record).contains("❯ cherry"));
    // The wheel moves the focus.
    let script = Script::new()
        .scroll(true, 0, 1)
        .scroll(true, 0, 1)
        .keys("enter");
    let (outcome, _) = headless::run(
        Select::new("Fruit", fruit()).with_mouse(true),
        script,
        40,
        10,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("cherry"));
}

#[test]
fn mouse_reporting_is_opt_in() {
    assert!(!Component::mouse(&Select::new("Fruit", fruit())));
    assert!(Component::mouse(
        &Select::new("Fruit", fruit()).with_mouse(true)
    ));
    assert!(!Component::mouse(&Confirm::new("Sure?")));
    assert!(!Component::mouse(&Form::new("Form")));
}

#[test]
fn the_border_beside_a_preview_drags_within_limits() {
    let items: Vec<Item<u8>> = (0..4)
        .map(|i| Item::new(i, format!("item {i}")).preview(Preview::Text("preview text".into())))
        .collect();
    // At 80 columns the list starts 36 wide: the border is at 36..39.
    let script = Script::new().drag((37, 1), (50, 1)).keys("esc");
    let mut select = Select::new("Pick", items.clone()).with_mouse(true);
    let (_, record) = headless::run(&mut select, script, 80, 10);
    assert_eq!(select.split_width(), Some(50));
    let dragged = &record.frames[record.frames.len() - 2];
    let line = dragged.lines().nth(1).unwrap();
    assert_eq!(
        line.find('│').map(|at| line[..at].chars().count()),
        Some(51),
        "{dragged}"
    );
    // Dragged far left, the list keeps its minimum width.
    let mut select = Select::new("Pick", items).with_mouse(true);
    let (_, _) = headless::run(
        &mut select,
        Script::new().drag((37, 1), (2, 1)).keys("esc"),
        80,
        10,
    );
    assert_eq!(select.split_width(), Some(12));
    // A press elsewhere does not start a drag.
    let mut select = Select::new("Pick", fruit()).with_mouse(true);
    let script = Script::new()
        .mouse(MouseKind::Drag(rich_interact::Button::Left), 50, 1)
        .keys("esc");
    let _ = headless::run(&mut select, script, 80, 10);
    assert_eq!(select.split_width(), None);
}

/// A component that shows a link and records what it was sent.
struct Linked(Vec<Event>);

impl Component for Linked {
    type Output = Vec<Event>;
    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<Vec<Event>> {
        if event.key().is_some() {
            return Flow::Done(std::mem::take(&mut self.0));
        }
        self.0.push(event.clone());
        Flow::Continue
    }
    fn render(&self, _: &Context<'_>) -> View {
        let link = Style::parse("link https://example.com/docs").unwrap();
        View::new(vec![
            vec![Segment::new("title", None)],
            vec![
                Segment::new("see ", None),
                Segment::new("the docs", Some(link)),
            ],
        ])
    }
    fn mouse(&self) -> bool {
        true
    }
}

#[test]
fn a_click_on_a_hyperlink_is_a_link_event() {
    let script = Script::new().click(6, 1).click(1, 1).click(6, 0).keys("q");
    let (outcome, _) = headless::run(Linked(Vec::new()), script, 40, 5);
    let events = outcome.unwrap().value().unwrap();
    assert_eq!(events[0], Event::Link("https://example.com/docs".into()));
    // The button's release, a click off the link, and a click on row 0.
    assert!(
        matches!(events[1], Event::Mouse(m) if m.kind == MouseKind::Up(rich_interact::Button::Left))
    );
    assert!(matches!(events[2], Event::Mouse(m) if m.row == 1 && m.column == 1));
    assert!(matches!(events[4], Event::Mouse(m) if m.row == 0));
}

#[test]
fn mouse_rows_are_relative_to_each_components_view() {
    use rich_interact::{EventLoop, LoopOptions};
    // Two components stacked: the second's rows start below the first's.
    let script = Script::new()
        .keys("enter")
        .click(0, 2)
        .click(0, 9)
        .keys("x");
    let backend = headless::Headless::new(script, 40, 10);
    let mut event_loop = EventLoop::new(backend, LoopOptions::default());
    let first = event_loop.mount(Select::new("First", ["a", "b"]));
    let second = event_loop.mount(Linked(Vec::new()));
    event_loop.run().unwrap();
    assert_eq!(first.take(), Some(Outcome::Done("a")));
    let events = second.take().unwrap().value().unwrap();
    // Answered, the first is one row ("? First › a"), so terminal row 2 is
    // the second's row 1.
    assert!(
        matches!(events[0], Event::Mouse(m) if m.row == 1 && m.kind == MouseKind::Down(rich_interact::Button::Left)),
        "{events:?}"
    );
    // A press below every view reaches no one; its release still does.
    assert_eq!(events.len(), 3, "{events:?}");
}

/// Two `Linked` views mounted and running at once: rows 0-1 are the
/// first's, rows 2-3 the second's. `q` finishes the first running one.
fn two_linked(script: Script) -> (Vec<Event>, Vec<Event>) {
    use rich_interact::{EventLoop, LoopOptions};
    let backend = headless::Headless::new(script.keys("q").keys("q"), 40, 10);
    let mut event_loop = EventLoop::new(backend, LoopOptions::default());
    let first = event_loop.mount(Linked(Vec::new()));
    let second = event_loop.mount(Linked(Vec::new()));
    event_loop.run().unwrap();
    (
        first.take().unwrap().value().unwrap(),
        second.take().unwrap().value().unwrap(),
    )
}

#[test]
fn a_click_in_a_later_running_view_reaches_that_view() {
    use rich_interact::Button;
    let (first, second) = two_linked(Script::new().click(1, 3));
    assert!(first.is_empty(), "{first:?}");
    assert_eq!(
        second,
        vec![
            Event::Mouse(rich_interact::Mouse::new(
                MouseKind::Down(Button::Left),
                1,
                1
            )),
            Event::Mouse(rich_interact::Mouse::new(MouseKind::Up(Button::Left), 1, 1)),
        ]
    );
    // A click in the first still reaches the first.
    let (first, second) = two_linked(Script::new().click(1, 0));
    assert_eq!(first.len(), 2, "{first:?}");
    assert!(second.is_empty(), "{second:?}");
}

#[test]
fn a_hyperlink_in_a_later_running_view_is_its_link_event() {
    let (first, second) = two_linked(Script::new().click(6, 3));
    assert!(first.is_empty(), "{first:?}");
    assert_eq!(second[0], Event::Link("https://example.com/docs".into()));
}

#[test]
fn a_drag_stays_with_the_view_it_started_in() {
    use rich_interact::Button;
    // Pressed in the first, dragged and released over the second.
    let (first, second) = two_linked(Script::new().drag((1, 1), (3, 3)));
    assert!(second.is_empty(), "{second:?}");
    assert_eq!(
        first.first(),
        Some(&Event::Mouse(rich_interact::Mouse::new(
            MouseKind::Down(Button::Left),
            1,
            1
        )))
    );
    // Moves and the release are in the first's coordinates.
    assert_eq!(
        first.last(),
        Some(&Event::Mouse(rich_interact::Mouse::new(
            MouseKind::Up(Button::Left),
            3,
            3
        )))
    );
    // A press outside every view reaches no one.
    let (first, second) = two_linked(Script::new().mouse(MouseKind::Down(Button::Left), 1, 8));
    assert!(
        first.is_empty() && second.is_empty(),
        "{first:?} {second:?}"
    );
}

#[test]
fn a_select_below_another_picks_on_its_own_click() {
    use rich_interact::{EventLoop, LoopOptions};
    // Each shows 4 rows (question, two items, help): the second's "y" is
    // on row 6. Enter goes to the first.
    let script = Script::new().click(4, 6).click(4, 6).keys("enter");
    let backend = headless::Headless::new(script, 40, 12);
    let mut event_loop = EventLoop::new(backend, LoopOptions::default());
    let first = event_loop.mount(Select::new("First", ["a", "b"]).with_mouse(true));
    let second = event_loop.mount(Select::new("Second", ["x", "y"]).with_mouse(true));
    event_loop.run().unwrap();
    assert_eq!(second.take(), Some(Outcome::Done("y")));
    assert_eq!(first.take(), Some(Outcome::Done("a")));
}

#[test]
fn confirm_choices_are_buttons() {
    let sheet = Confirm::new("Deploy?")
        .choices([
            Choice::new("yes", "Deploy", 'y'),
            Choice::new("later", "Later", 'l'),
            Choice::new("no", "Cancel", 'n'),
        ])
        .with_mouse(true);
    // Row 1 holds the buttons: "  " then " Deploy " " Later " …
    let (outcome, _) = headless::run(sheet, Script::new().click(13, 1), 60, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done("later".into()));
    // A click beside them does nothing.
    let sheet = Confirm::new("Deploy?").with_mouse(true);
    let (outcome, _) = headless::run(sheet, Script::new().click(40, 1).keys("n"), 60, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done("no".into()));
}

#[test]
fn form_fields_and_buttons_click() {
    let form = || {
        Form::new("Service")
            .text("name", "Name")
            .toggle("public", "Public", false)
            .choice("tier", "Tier", ["free", "pro"])
            .with_mouse(true)
    };
    // Click the toggle (row 2), the choice twice (row 3), then Submit.
    let script = Script::new()
        .click(3, 2)
        .click(3, 3)
        .click(3, 3)
        .click(3, 4);
    let (outcome, record) = headless::run(form(), script, 60, 12);
    let answers = outcome.unwrap().value().unwrap();
    assert_eq!(answers.flag("public"), Some(true));
    assert_eq!(answers.text("tier"), Some("pro"));
    assert!(before_answer(&record).contains(" Submit    Cancel "));
    let (outcome, _) = headless::run(form(), Script::new().click(13, 4), 60, 12);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
}

// ---- Actions ----

#[test]
fn the_action_menu_lists_item_and_view_actions() {
    let items = fruit()
        .into_iter()
        .map(|item| item.action(Action::new("eat", "Eat", Key::ctrl('e'))));
    let actions = Actions::new()
        .action(Action::menu("share", "Share"))
        .action_if(Action::menu("peel", "Peel"), |target| {
            target.label == "banana"
        });
    let mut select = Select::new("Fruit", items).actions(actions);
    let script = Script::new().keys("down ctrl+k down down enter");
    let (outcome, record) = headless::run(&mut select, script, 60, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("banana"));
    assert_eq!(select.action(), Some("peel"));
    let menu = &record.frames[2];
    assert!(
        menu.contains("❯ Eat    ctrl+e\n    Share\n    Peel"),
        "{menu}"
    );
    assert!(
        record.frames[1].contains("ctrl+k actions"),
        "{}",
        record.frames[1]
    );
    // Escape closes the menu without leaving the list.
    let mut select =
        Select::new("Fruit", fruit()).actions(Actions::new().action(Action::menu("x", "X")));
    let (outcome, _) = headless::run(&mut select, Script::new().keys("ctrl+k esc enter"), 60, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("apple"));
    assert_eq!(select.action(), None);
}

#[test]
fn a_view_action_key_picks_directly_and_the_menu_opens_on_a_chosen_key() {
    let actions = Actions::new().action(Action::new("copy", "Copy", Key::ctrl('y')));
    let mut select = Select::new("Fruit", fruit())
        .actions(actions.clone())
        .menu_key(Key::parse("f2").unwrap());
    let (outcome, _) = headless::run(&mut select, Script::new().keys("down ctrl+y"), 60, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("banana"));
    assert_eq!(select.action(), Some("copy"));
    let mut select = Select::new("Fruit", fruit())
        .actions(actions)
        .menu_key(Key::parse("f2").unwrap());
    let (_, record) = headless::run(&mut select, Script::new().keys("f2 esc esc"), 60, 12);
    assert!(record
        .frames
        .iter()
        .any(|frame| frame.contains("enter run")));
}

#[test]
fn table_rows_take_row_actions() {
    let rows = vec![
        (
            Item::new(1, "web"),
            vec!["web".to_string(), "running".into(), "3".into()],
        ),
        (
            Item::new(2, "db"),
            vec!["db".to_string(), "stopped".into(), "1".into()],
        ),
    ];
    let seen = Arc::new(std::sync::Mutex::new(Vec::<ActionTarget>::new()));
    let log = Arc::clone(&seen);
    let actions = Actions::new().action_if(
        Action::new("restart", "Restart", Key::ctrl('r')),
        move |target| {
            log.lock().unwrap().push(target.clone());
            target.kind == TargetKind::Row
        },
    );
    let mut table =
        TableSelect::new("Service", ["Name", "State", "Replicas"], rows).actions(actions);
    let (outcome, record) = headless::run(
        &mut table,
        Script::new().text("stop").keys("ctrl+r"),
        60,
        10,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(2));
    assert_eq!(table.action(), Some("restart"));
    let first = &record.frames[0];
    assert!(
        first.contains("  Name  State    Replicas\n❯ web   running  3"),
        "{first}"
    );
    let target = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(target.value, "db\tstopped\t1");
}

#[test]
fn tree_nodes_fold_and_take_node_actions() {
    let nodes = vec![
        (0, Item::new("src", "src")),
        (1, Item::new("src/lib.rs", "lib.rs")),
        (1, Item::new("src/bin", "bin")),
        (2, Item::new("src/bin/main.rs", "main.rs")),
        (0, Item::new("Cargo.toml", "Cargo.toml")),
    ];
    let actions = Actions::new().action_if(Action::menu("open", "Open"), |target| {
        target.kind == TargetKind::Node && target.value.ends_with(".rs")
    });
    let mut tree = TreeSelect::new("Files", nodes.clone()).actions(actions);
    let script = Script::new().keys("down down down ctrl+k enter");
    let (outcome, record) = headless::run(&mut tree, script, 60, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("src/bin/main.rs"));
    assert_eq!(tree.action(), Some("open"));
    let first = &record.frames[0];
    assert!(
        first.contains("❯ ▾ src\n  ├── lib.rs\n  └── ▾ bin\n      └── main.rs\n  Cargo.toml"),
        "{first}"
    );
    // Left folds a node; Right unfolds it; Left on a leaf goes to its parent.
    let mut tree = TreeSelect::new("Files", nodes.clone());
    let script = Script::new().keys("left down enter");
    let (outcome, record) = headless::run(&mut tree, script, 60, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("Cargo.toml"));
    assert!(
        record.frames[1].contains("▸ src\n  Cargo.toml"),
        "{}",
        record.frames[1]
    );
    assert!(tree.is_collapsed(0));
    let tree = TreeSelect::new("Files", nodes.clone()).collapsed(true);
    let (outcome, _) = headless::run(
        tree,
        Script::new().keys("right down down left enter"),
        60,
        12,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("src"));
    // A search finds nodes inside folded ones.
    let tree = TreeSelect::new("Files", nodes).collapsed(true);
    let (outcome, _) = headless::run(tree, Script::new().text("main").keys("enter"), 60, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("src/bin/main.rs"));
}

#[test]
fn file_entries_take_file_actions() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.md"), "# notes").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let actions = Actions::new().action_for(
        TargetKind::File,
        Action::new("edit", "Edit", Key::ctrl('e')),
    );
    let mut picker = FilePicker::new("File", dir.path()).actions(actions);
    // An action on a directory picks it, where Enter would open it.
    let (outcome, _) = headless::run(&mut picker, Script::new().keys("ctrl+e"), 80, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done(dir.path().join("sub")));
    assert_eq!(picker.action(), Some("edit"));
}

#[test]
fn plugin_actions_reach_a_view() {
    use rich_plugin_api::{CustomAction, Plugin, PluginError, PluginMetadata, PluginRegistrar};
    struct Upper;
    impl CustomAction for Upper {
        fn label(&self) -> String {
            "Shout".into()
        }
        fn key(&self) -> Option<String> {
            Some("ctrl+u".into())
        }
        fn run(&self, _: &str, value: &str) -> Result<Option<String>, PluginError> {
            Ok(Some(value.to_uppercase()))
        }
    }
    struct Shout;
    impl Plugin for Shout {
        fn metadata(&self) -> PluginMetadata {
            PluginMetadata::new("shout", "Shout", "0.0.0")
        }
        fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
            registrar.action("shout", Arc::new(Upper));
            Ok(())
        }
    }
    let mut registry = rich_ext::registry::ExtensionRegistry::new();
    registry.add_plugin(&Shout).unwrap();
    let mut select = Select::new("Fruit", fruit()).actions(Actions::from_registry(&registry));
    // The plugin's key wins over Ctrl+U's clearing, as an action's key does.
    let (outcome, _) = headless::run(&mut select, Script::new().keys("down ctrl+u"), 60, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("banana"));
    let id = select.action().unwrap();
    let (_, _, action) = registry
        .actions()
        .into_iter()
        .find(|(name, _, _)| *name == id)
        .unwrap();
    assert_eq!(action.run("item", "banana"), Ok(Some("BANANA".into())));
}
