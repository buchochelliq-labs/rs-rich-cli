//! Composition (0.0.14 workstream 1), headless: containers, focus,
//! routing and bubbling, the keymap, and a component written here, outside
//! the crate, composed with the built-ins in a split, tabs and a modal.

#[allow(dead_code)]
#[path = "../examples/custom_component.rs"]
mod example;

use std::cell::RefCell;
use std::rc::Rc;

use example::{app, checklist_keymap, App, Checklist};
use rich_interact::compose::{
    Column, ComponentExt, Label, Layer, LayerKind, Layers, Placement, Rect, Row, Size, Split, Tabs,
    FOCUS_NEXT,
};
use rich_interact::headless::{self, Script};
use rich_interact::keymap::{self, keys, Keymap, Overrides};
use rich_interact::{
    Component, Context, Event, Flow, Input, Key, Mouse, MouseKind, Outcome, Select, View,
};

fn frame(record: &headless::Record) -> String {
    record.last_frame().to_string()
}

#[test]
fn the_example_app_ticks_switches_tabs_and_keeps_state() {
    // Tick the second item, look at the other tab, come back: still ticked.
    let script = Script::new()
        .keys("down space alt+right alt+left")
        .keys("enter");
    let (outcome, record) = headless::run(app(), script, 72, 16);
    assert_eq!(
        outcome.unwrap(),
        Outcome::Done(App::Checked(vec!["update the changelog".into()]))
    );
    let frames = &record.frames;
    assert!(
        frames.iter().any(|f| f.contains("Where to?")),
        "the Deploy tab showed:\n{}",
        frames.join("\n---\n")
    );
    let last = frame(&record);
    assert!(last.contains("◉ update the changelog"), "{last}");
    assert!(
        last.contains("• update the changelog"),
        "the summary: {last}"
    );
}

#[test]
fn the_example_app_deploys_from_built_ins_side_by_side() {
    let script = Script::new()
        .keys("alt+2")
        .text("v2")
        .keys("enter down enter");
    let (outcome, record) = headless::run(app(), script, 72, 16);
    assert_eq!(
        outcome.unwrap(),
        Outcome::Done(App::Deploy {
            region: "us-east".into(),
            tag: "v2".into()
        }),
        "{}",
        record.frames.join("\n---\n")
    );
}

#[test]
fn the_example_app_opens_help_and_quit_modals() {
    let script = Script::new()
        .keys("f1")
        .keys("escape")
        .keys("ctrl+q n")
        .keys("ctrl+q y");
    let (outcome, record) = headless::run(app(), script, 72, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done(App::Quit));
    let help = record
        .frames
        .iter()
        .find(|f| f.contains("Keys"))
        .unwrap_or_else(|| panic!("no help dialog:\n{}", record.frames.join("\n---\n")));
    assert!(help.contains("tick or untick"), "{help}");
    if std::env::var_os("SHOW").is_some() {
        eprintln!("{help}");
    }
    assert!(help.contains("╭"), "{help}");
    // The modal dims the base: SGR 2 is written for it.
    assert!(record.output().contains("\x1b[2"), "a backdrop");
    assert!(record.frames.iter().any(|f| f.contains("Quit?")));
}

#[test]
fn the_example_app_split_border_drags() {
    // 72 columns at 60%: the border is at column 43.
    let script = Script::new().drag((43, 3), (30, 3)).keys("enter");
    let (outcome, record) = headless::run(app(), script, 72, 16);
    assert!(matches!(outcome.unwrap(), Outcome::Done(App::Checked(_))));
    let border = |frame: &str| {
        frame
            .lines()
            .nth(2)
            .and_then(|line| line.chars().position(|c| c == '│'))
    };
    let first = border(&record.frames[0]);
    let last = border(frame(&record).as_str());
    assert_eq!(first, Some(43), "{}", record.frames[0]);
    assert_eq!(last, Some(30), "{}", frame(&record));
}

/// Records the events it gets, in its own coordinates.
struct Probe {
    name: &'static str,
    seen: Rc<RefCell<Vec<String>>>,
    takes: Vec<Key>,
}

impl Probe {
    fn new(name: &'static str, seen: &Rc<RefCell<Vec<String>>>) -> Probe {
        Probe {
            name,
            seen: Rc::clone(seen),
            takes: Vec::new(),
        }
    }

    fn takes(mut self, names: &str) -> Probe {
        self.takes = keys(names);
        self
    }
}

impl Component for Probe {
    type Output = String;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<String> {
        match event {
            Event::Key(key) if self.takes.contains(key) => {
                self.seen.borrow_mut().push(format!("{} {key}", self.name));
                Flow::Continue
            }
            Event::Key(key) if key.code == rich_interact::KeyCode::Enter => {
                Flow::Done(self.name.to_string())
            }
            Event::Mouse(mouse) => {
                self.seen.borrow_mut().push(format!(
                    "{} mouse {},{}",
                    self.name, mouse.column, mouse.row
                ));
                Flow::Continue
            }
            _ => Flow::Ignored,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        View::new(context.markup(&format!(
            "{} {}x{}",
            self.name, context.width, context.height
        )))
    }

    fn mouse(&self) -> bool {
        true
    }

    fn keymap(&self) -> Keymap {
        Keymap::new(self.name).bind("take", self.takes.clone(), "take a key")
    }
}

#[test]
fn tab_moves_focus_through_nested_containers_and_wraps() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let root = Column::new()
        .child(Label::new("title"))
        .child(Probe::new("a", &seen).takes("x"))
        .child(
            Row::new()
                .child(Probe::new("b", &seen).takes("x"))
                .child(Probe::new("c", &seen).takes("x")),
        );
    // x goes to the focused probe; Tab moves a → b → c → a; Shift+Tab back.
    let script = Script::new().keys("x tab x tab x tab x shift+tab x enter");
    let (outcome, _) = headless::run(root, script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done("c".to_string()));
    assert_eq!(*seen.borrow(), ["a x", "b x", "c x", "a x", "c x"]);
}

#[test]
fn an_unused_key_bubbles_to_the_container() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&seen);
    let root = Column::new()
        .child(Probe::new("a", &seen).takes("x"))
        .on("save", keys("ctrl+s"), "save", move || {
            log.borrow_mut().push("column save".into());
            Flow::Continue
        })
        .shortcut("first", keys("x"), "before the child", || {
            Flow::Done("column".into())
        });
    // `ctrl+s` bubbles past the probe; `x` never reaches it: a shortcut.
    let script = Script::new().keys("ctrl+s x");
    let (outcome, _) = headless::run(root, script, 40, 4);
    assert_eq!(outcome.unwrap(), Outcome::Done("column".to_string()));
    assert_eq!(*seen.borrow(), ["column save"]);
}

#[test]
fn mouse_events_arrive_in_the_childs_coordinates_and_focus_it() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let root = Row::new()
        .sized(Size::Fixed(10), Probe::new("a", &seen).takes("x"))
        .child(Probe::new("b", &seen).takes("x"))
        .gap(2);
    let script = Script::new().click(15, 0).keys("x enter");
    let (outcome, record) = headless::run(root, script, 30, 3);
    assert_eq!(outcome.unwrap(), Outcome::Done("b".to_string()));
    assert_eq!(*seen.borrow(), ["b mouse 3,0", "b mouse 3,0", "b x"]);
    assert!(
        record.frames[0].starts_with("a 10x3      b 18x3"),
        "{}",
        record.frames[0]
    );
}

#[test]
fn a_column_sizes_fixed_content_and_flexible_children() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let root = Column::new()
        .sized(Size::Fixed(2), Probe::new("fixed", &seen))
        .child(Label::new("auto"))
        .sized(Size::Flex(1), Probe::new("one", &seen))
        .sized(Size::Flex(2), Probe::new("two", &seen));
    let script = Script::new().keys("enter");
    let (_, record) = headless::run(root, script, 20, 12);
    let lines: Vec<&str> = record.frames[0].split('\n').collect();
    // 12 rows: 2 fixed, 1 auto, and 9 shared 3 : 6.
    assert_eq!(lines[0], "fixed 20x2");
    assert_eq!(lines[2], "auto");
    assert_eq!(lines[3], "one 20x3");
    assert_eq!(lines[6], "two 20x6");
    assert_eq!(lines.len(), 12);
}

#[test]
fn a_split_keeps_its_minimums_and_moves_with_the_keyboard() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let split = Split::horizontal(Probe::new("l", &seen), Probe::new("r", &seen))
        .at(4)
        .min(8);
    let script = Script::new().keys("alt+l alt+l enter");
    let (_, record) = headless::run(split, script, 40, 3);
    assert!(
        record.frames[0].starts_with("l 8x3   │r 31x3"),
        "{}",
        record.frames[0]
    );
    assert!(frame(&record).starts_with("l 12x3"), "{}", frame(&record));

    let vertical = Split::vertical(Probe::new("t", &seen), Probe::new("b", &seen)).ratio(25);
    let (_, record) = headless::run(vertical, Script::new().keys("enter"), 20, 9);
    let lines: Vec<&str> = record.frames[0].lines().collect();
    assert_eq!(lines[0], "t 20x2");
    assert_eq!(lines[2], "─".repeat(20));
    assert_eq!(lines[3], "b 20x6");
}

#[test]
fn tabs_switch_by_key_number_and_click_and_keep_state() {
    let first = Input::new("First").map(Flow::Done);
    let second = Input::new("Second").map(Flow::Done);
    let tabs = Tabs::new()
        .tab("one", first)
        .tab("two", second)
        .with_mouse(true);
    // Type in one, go to two by Alt+2, type, click "one", finish it.
    let script = Script::new()
        .text("ab")
        .keys("alt+2")
        .text("zz")
        .click(1, 0)
        .text("c")
        .keys("enter");
    let (outcome, record) = headless::run(tabs, script, 40, 5);
    assert_eq!(outcome.unwrap(), Outcome::Done("abc".to_string()));
    assert!(record.frames.iter().any(|f| f.contains("Second › zz")));
    assert!(
        record.frames[0].starts_with(" one   two "),
        "{}",
        record.frames[0]
    );
}

#[test]
fn a_modal_traps_focus_and_escape_dismisses_it() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let base = Probe::new("base", &seen).takes("x");
    let mut layers = Layers::new(base);
    let modal = Column::new()
        .child(Probe::new("m1", &seen).takes("x"))
        .child(Probe::new("m2", &seen).takes("x"));
    layers.open(Layer::modal(modal).size(20, 6).title("Dialog"));
    // Tab cycles inside the modal (m1 → m2 → m1); Escape closes it; the
    // base has the keys again.
    let script = Script::new().keys("x tab x tab x escape x enter");
    let (outcome, record) = headless::run(layers, script, 40, 8);
    assert_eq!(outcome.unwrap(), Outcome::Done("base".to_string()));
    assert_eq!(*seen.borrow(), ["m1 x", "m2 x", "m1 x", "base x"]);
    assert!(
        record.frames[0].contains("╭─ Dialog ─"),
        "{}",
        record.frames[0]
    );
}

#[test]
fn a_popover_closes_on_a_click_outside() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let base = Probe::new("base", &seen).takes("x");
    let layers = Layers::new(base).with_mouse(true);
    let handle = layers.handle();
    handle.open(
        Layer::popover(Probe::new("pop", &seen).takes("x"))
            .placement(Placement::At(Rect::new(10, 1, 12, 3))),
    );
    let script = Script::new()
        .keys("x")
        .click(14, 2)
        .click(0, 0)
        .keys("x enter");
    let (outcome, _) = headless::run(layers, script, 40, 8);
    assert_eq!(outcome.unwrap(), Outcome::Done("base".to_string()));
    assert_eq!(handle.open_count(), 0);
    assert_eq!(
        *seen.borrow(),
        ["pop x", "pop mouse 3,0", "pop mouse 3,0", "base x"]
    );
    assert_eq!(
        Layer::<String>::popover(Label::new("")).kind(),
        LayerKind::Popover
    );
}

#[test]
fn a_layer_answer_finishes_the_host_and_a_cancel_only_closes_it() {
    let layers = Layers::new(Label::new("base")).open_on("ask", keys("a"), "ask", || {
        Layer::modal(Select::new("Pick", ["one", "two"]).map(|s| Flow::Done(s.to_string())))
    });
    let script = Script::new().keys("a escape a down enter");
    let (outcome, _) = headless::run(layers, script, 40, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("two".to_string()));
}

#[test]
fn the_keymap_lists_the_focus_path_and_rebinds() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let split = Split::horizontal(
        Checklist::new("Todo", ["a"]).map(|_| Flow::Done(String::new())),
        Probe::new("r", &seen),
    )
    .rebind(FOCUS_NEXT, keys("f6"));
    let listed: Vec<String> = split.keymap().bindings().iter().map(|b| b.id()).collect();
    assert_eq!(listed[0], "checklist.up");
    assert!(listed.contains(&"split.focus-next".to_string()));
    assert!(listed.contains(&"split.grow".to_string()));
    assert_eq!(split.keymap().keys(FOCUS_NEXT), keys("f6"));
    assert_eq!(checklist_keymap().key("tick"), Key::parse("space"));

    // F6 moves focus now; Tab no longer does.
    let (outcome, _) = headless::run(split, Script::new().keys("tab f6 enter"), 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done("r".to_string()));
}

#[test]
fn a_select_rebinds_its_keys() {
    let select = Select::new("Pick", ["one", "two", "three"]).rebind("down", keys("ctrl+j"));
    let (outcome, _) = headless::run(select, Script::new().keys("ctrl+j ctrl+j enter"), 40, 8);
    assert_eq!(outcome.unwrap(), Outcome::Done("three"));
}

#[test]
fn installed_overrides_rebind_a_custom_component() {
    // A context of its own, so no other test sees the override.
    let mut overrides = Overrides::new();
    overrides.set("checklist", "tick", keys("t"));
    let keymap = checklist_keymap();
    keymap::install(Overrides::parse("compose-test.never = f12").unwrap());
    assert_eq!(keymap.action(Key::parse("space").unwrap()), Some("tick"));
    let mut local = checklist_keymap();
    local.apply(&overrides);
    assert_eq!(local.action(Key::char('t')), Some("tick"));
    assert_eq!(local.action(Key::parse("space").unwrap()), None);
    keymap::install(Overrides::new());
}

#[test]
fn a_mouse_press_holds_its_child_until_the_release() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let root = Row::new()
        .child(Probe::new("a", &seen))
        .child(Probe::new("b", &seen));
    let script = Script::new()
        .mouse(MouseKind::Down(rich_interact::Button::Left), 2, 0)
        .mouse(MouseKind::Drag(rich_interact::Button::Left), 25, 0)
        .mouse(MouseKind::Up(rich_interact::Button::Left), 25, 0)
        .keys("enter");
    let (outcome, _) = headless::run(root, script, 40, 3);
    assert_eq!(outcome.unwrap(), Outcome::Done("a".to_string()));
    // The drag and release, past a's right edge, still went to a.
    assert_eq!(
        *seen.borrow(),
        ["a mouse 2,0", "a mouse 25,0", "a mouse 25,0"]
    );
    let _ = Mouse::new(MouseKind::Moved, 0, 0);
}
