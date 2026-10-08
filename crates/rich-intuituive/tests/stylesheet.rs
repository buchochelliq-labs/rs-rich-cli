//! Stylesheets: what they match, what they set, what code keeps, and
//! reading one live from a file.

mod common;

use std::time::Duration;

use common::row_of;
use intuituive::interact::{Button, Event, Key, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::rich::Style;

fn frame(driver: &mut intuituive::Driver, event: Option<Event>) -> Vec<String> {
    if let Some(event) = event {
        driver.event(event);
    }
    driver.update(Duration::ZERO);
    let _ = driver.render();
    driver.screen().plain()
}

fn press(key: &str) -> Option<Event> {
    Some(Event::Key(Key::parse(key).expect("a key")))
}

/// The style of the cell at `x`, `y`.
fn style_at(driver: &intuituive::Driver, x: u16, y: u16) -> Style {
    let screen = driver.screen();
    screen
        .style(screen.cell(x, y).style)
        .cloned()
        .unwrap_or_default()
}

fn style(definition: &str) -> Style {
    Style::parse(definition).unwrap()
}

#[test]
fn kinds_names_and_classes_colour_nodes_under_their_own_colours() {
    let app = App::new(|| {
        column([
            label("plain"),
            label("[green]own[/] rest").name("named"),
            label("classy").class("warn big"),
        ])
    })
    .stylesheet(
        "label { color: red; } #named { background: blue; } .warn.big { text-style: bold; }",
    );
    let mut driver = app.driver(20, 3);
    frame(&mut driver, None);
    assert_eq!(style_at(&driver, 0, 0), style("red"));
    // The markup's own green stays; the sheet fills in what it left unset.
    assert_eq!(style_at(&driver, 0, 1), style("green on blue"));
    assert_eq!(style_at(&driver, 5, 1), style("red on blue"));
    assert_eq!(style_at(&driver, 0, 2), style("bold red"));
}

#[test]
fn states_restyle_and_code_styles_go_over_the_sheet() {
    let app = App::new(|| {
        column([
            label("one").focusable(),
            label("two").focusable().focus_style("underline"),
            label("three"),
        ])
    })
    .stylesheet("label:focus { color: red; } label:hover { background: blue; }");
    let mut driver = app.driver(10, 3);
    frame(&mut driver, None);
    // The first focusable node has the focus.
    assert_eq!(style_at(&driver, 0, 0), style("red"));
    assert_eq!(style_at(&driver, 0, 1), Style::default());
    frame(&mut driver, press("tab"));
    assert_eq!(style_at(&driver, 0, 0), Style::default());
    assert_eq!(style_at(&driver, 0, 1), style("red underline"));
    frame(
        &mut driver,
        Some(Event::Mouse(Mouse::new(MouseKind::Moved, 1, 2))),
    );
    assert_eq!(style_at(&driver, 0, 2), style("on blue"));
    frame(
        &mut driver,
        Some(Event::Mouse(Mouse::new(MouseKind::Moved, 1, 0))),
    );
    assert_eq!(style_at(&driver, 0, 2), Style::default());
    assert_eq!(style_at(&driver, 0, 0), style("on blue"));
}

#[test]
fn a_class_that_follows_a_signal_restyles_the_node_and_what_is_inside() {
    let app = App::new(|| {
        let load = signal(10);
        column([
            row([label("cpu "), text!("{load}")]).class_when("hot", move || load.get() > 90),
            label("+").on_key("+", move |_| load.update(|l| *l += 50)),
        ])
        .on_key("+", move |_| load.update(|l| *l += 50))
    })
    .stylesheet(".hot label { color: red; } .hot { background: yellow; }");
    let mut driver = app.driver(10, 2);
    frame(&mut driver, None);
    assert_eq!(style_at(&driver, 0, 0), Style::default());
    frame(&mut driver, press("+"));
    assert_eq!(style_at(&driver, 0, 0), Style::default(), "60 is not hot");
    frame(&mut driver, press("+"));
    assert_eq!(style_at(&driver, 0, 0), style("red on yellow"));
    // The gap after the text takes the row's background too.
    assert_eq!(style_at(&driver, 9, 0), style("on yellow"));
}

#[test]
fn sizes_come_from_the_sheet_unless_code_set_them() {
    let app = App::new(|| {
        column([
            label("a").name("a"),
            label("b").name("b").fixed(1),
            label("rest"),
        ])
    })
    .stylesheet("#a { size: 3; } #b { size: 4; } label { size: 1; }");
    let mut driver = app.driver(10, 8);
    let rows = frame(&mut driver, None);
    assert_eq!(row_of(&rows, "a"), Some(0));
    assert_eq!(row_of(&rows, "b"), Some(3), "{rows:?}");
    assert_eq!(row_of(&rows, "rest"), Some(4), "{rows:?}");
}

#[test]
fn display_none_hides_a_node_and_takes_it_out_of_the_tab_order() {
    let app = App::new(|| {
        let picked = signal("");
        column([
            label("gone")
                .class("secret")
                .focusable()
                .on_key("enter", move |_| picked.set("gone")),
            label("shown")
                .focusable()
                .on_key("enter", move |_| picked.set("shown")),
            text!("picked {picked}"),
        ])
    })
    .stylesheet(".secret { display: none; }");
    let mut driver = app.driver(20, 3);
    let rows = frame(&mut driver, None);
    assert_eq!(rows[0].trim_end(), "shown");
    let rows = frame(&mut driver, press("enter"));
    assert!(row_of(&rows, "picked shown").is_some(), "{rows:?}");
}

#[test]
fn docked_children_keep_to_their_edge() {
    let app = App::new(|| {
        column([
            label("body"),
            label("status").name("status"),
            label("title").name("title"),
        ])
    })
    .stylesheet(
        "#status { dock: bottom; size: 1; } #title { dock: top; size: 1; } label { size: 1; }",
    );
    let mut driver = app.driver(10, 5);
    let rows = frame(&mut driver, None);
    assert_eq!(rows[0].trim_end(), "title", "{rows:?}");
    assert_eq!(rows[1].trim_end(), "body", "{rows:?}");
    assert_eq!(rows[4].trim_end(), "status", "{rows:?}");
}

#[test]
fn a_border_and_padding_go_round_any_node_and_clicks_land_inside() {
    let app = App::new(|| {
        let clicked = signal(String::new());
        column([
            list(
                || vec!["alpha".to_string(), "beta".to_string()],
                signal(0usize),
            )
            .name("files")
            .on_mouse(move |_, mouse| {
                clicked.set(format!("{},{}", mouse.column, mouse.row));
                true
            }),
            text!("at {clicked}").fixed(1),
        ])
    })
    .stylesheet("#files { border: square $accent; border-title: Files; padding: 0 1; }");
    let mut driver = app.driver(16, 6);
    let rows = frame(&mut driver, None);
    assert!(rows[0].starts_with("┌─ Files ─"), "{rows:?}");
    assert!(rows[1].starts_with("│ alpha"), "{rows:?}");
    assert!(rows[4].starts_with("└"), "{rows:?}");
    // The accent's colour on the border.
    assert_eq!(
        style_at(&driver, 0, 0).color(),
        style("bright_cyan").color()
    );
    // Row 2 on screen is the list's second row, at its first column.
    let rows = frame(
        &mut driver,
        Some(Event::Mouse(Mouse::new(
            MouseKind::Down(Button::Left),
            2,
            2,
        ))),
    );
    assert!(row_of(&rows, "at 0,1").is_some(), "{rows:?}");
}

#[test]
fn a_panel_takes_the_sheets_box_title_and_padding() {
    let app = App::new(|| label("inside").panel("").name("box"))
        .stylesheet("#box { border: double; border-title: Box; padding: 1 2; }");
    let mut driver = app.driver(16, 5);
    let rows = frame(&mut driver, None);
    assert!(rows[0].starts_with("╔═ Box ═"), "{rows:?}");
    assert!(rows[2].starts_with("║  inside"), "{rows:?}");
    assert!(rows[4].starts_with("╚"), "{rows:?}");
}

#[test]
fn gaps_and_grid_tracks_come_from_the_sheet() {
    let app = App::new(|| {
        column([
            row([label("a"), label("b")]).name("pair").fixed(1),
            grid([], [label("1"), label("2"), label("3"), label("4")]).name("cells"),
        ])
    })
    .stylesheet(
        "#pair { gap: 3; } #pair label { size: 1; } #cells { grid-columns: 2 2; grid-rows: 1; }",
    );
    let mut driver = app.driver(10, 4);
    let rows = frame(&mut driver, None);
    assert_eq!(rows[0].trim_end(), "a   b", "{rows:?}");
    assert_eq!(rows[1].trim_end(), "1 2", "{rows:?}");
    assert_eq!(rows[2].trim_end(), "3 4", "{rows:?}");
}

#[test]
fn a_disabled_node_takes_no_focus_and_no_clicks() {
    let app = App::new(|| {
        let off = signal(true);
        let clicks = signal(0);
        column([
            label("button")
                .on_click(move |_| clicks.update(|n| *n += 1))
                .disabled_when(move || off.get()),
            label("toggle").on_click(move |_| off.update(|o| *o = !*o)),
            text!("clicks {clicks}"),
        ])
    })
    .stylesheet("label:disabled { text-style: dim; }");
    let mut driver = app.driver(20, 3);
    frame(&mut driver, None);
    assert_eq!(style_at(&driver, 0, 0), style("dim"));
    let click = |row| {
        Some(Event::Mouse(Mouse::new(
            MouseKind::Down(Button::Left),
            1,
            row,
        )))
    };
    let rows = frame(&mut driver, click(0));
    assert!(row_of(&rows, "clicks 0").is_some(), "{rows:?}");
    frame(&mut driver, click(1));
    assert_eq!(style_at(&driver, 0, 0), Style::default());
    let rows = frame(&mut driver, click(0));
    assert!(row_of(&rows, "clicks 1").is_some(), "{rows:?}");
}

#[test]
fn a_sheet_that_does_not_parse_says_so_in_a_toast() {
    let app = App::new(|| label("hi")).stylesheet("label { colour: red; }");
    let mut driver = app.driver(60, 6);
    let rows = frame(&mut driver, None);
    assert!(
        row_of(&rows, "stylesheet: line 1:9: unknown property `colour`").is_some(),
        "{rows:?}"
    );
}

#[test]
fn a_sheet_file_is_read_again_when_it_changes_and_a_bad_edit_keeps_the_last() {
    let dir = std::env::temp_dir().join(format!("intuituive-sheet-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("app.tcss");
    std::fs::write(&path, "label { color: red; }").unwrap();
    let app = App::new(|| label("hi")).stylesheet_file(&path);
    let mut driver = app.driver(60, 6);
    frame(&mut driver, None);
    assert_eq!(style_at(&driver, 0, 0), style("red"));
    std::fs::write(&path, "label { color: green;  }").unwrap();
    frame(&mut driver, None);
    assert_eq!(style_at(&driver, 0, 0), style("green"));
    std::fs::write(&path, "label { color: nope; }").unwrap();
    let rows = frame(&mut driver, None);
    assert_eq!(
        style_at(&driver, 0, 0),
        style("green"),
        "the last good sheet"
    );
    assert!(
        row_of(&rows, "`nope` is not a colour").is_some(),
        "{rows:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn without_a_sheet_nothing_changes() {
    // The same app with and without an empty sheet sends the same bytes.
    let build = || {
        App::new(|| {
            column([label("one").focus_style("reverse"), label("two")]).on_key("q", |cx| cx.quit())
        })
    };
    let plain = build().render_with(&["tab", "q"], 10, 2).unwrap();
    let sheet = build()
        .stylesheet("/* nothing */")
        .render_with(&["tab", "q"], 10, 2)
        .unwrap();
    assert_eq!(plain, sheet);
}

/// A widget that takes `x`, counting them.
struct Counter(std::rc::Rc<std::cell::Cell<u32>>);

impl intuituive::widget::Widget for Counter {
    fn draw(
        &mut self,
        _cx: &mut intuituive::widget::DrawCx,
        _canvas: &mut intuituive::widget::Canvas,
    ) {
    }

    fn event(
        &mut self,
        _cx: &mut intuituive::widget::EventCx,
        event: &intuituive::widget::WidgetEvent,
    ) -> intuituive::widget::Used {
        if matches!(event, intuituive::widget::WidgetEvent::Key(key) if *key == Key::char('x')) {
            self.0.set(self.0.get() + 1);
            return intuituive::widget::Used::Yes;
        }
        intuituive::widget::Used::No
    }

    fn focusable(&self) -> bool {
        true
    }
}

#[test]
fn a_focused_node_that_is_disabled_lets_go_of_the_focus_and_its_keys() {
    let keys = std::rc::Rc::new(std::cell::Cell::new(0));
    let counted = keys.clone();
    let app = App::new(move || {
        let off = signal(false);
        let picked = signal("");
        column([
            widget(Counter(counted.clone()))
                .fixed(1)
                .disabled_when(move || off.get()),
            label("other")
                .focusable()
                .on_key("x", move |_| picked.set("other")),
            text!("picked {picked}"),
        ])
        .on_key("ctrl+d", move |_| off.set(true))
    });
    let mut driver = app.driver(20, 3);
    frame(&mut driver, None);
    frame(&mut driver, press("x"));
    assert_eq!(keys.get(), 1);
    // Disabled while focused: the widget sees no more keys, and the focus
    // moves on to the next node.
    frame(&mut driver, press("ctrl+d"));
    let rows = frame(&mut driver, press("x"));
    assert_eq!(keys.get(), 1);
    assert!(row_of(&rows, "picked other").is_some(), "{rows:?}");
}

#[test]
fn a_hidden_grid_child_takes_no_cell() {
    let app = App::new(|| {
        grid(
            [Size::Fixed(3), Size::Fixed(3)],
            [
                label("aa"),
                label("bb").class("gone"),
                label("cc"),
                label("dd"),
            ],
        )
    })
    .stylesheet(".gone { display: none; }");
    let mut driver = app.driver(6, 2);
    let rows = frame(&mut driver, None);
    assert_eq!(rows[0].trim_end(), "aa cc");
    assert_eq!(rows[1].trim_end(), "dd");
}

#[test]
fn border_none_takes_a_panels_border_away() {
    let app = App::new(|| label("inside").panel("title")).stylesheet("panel { border: none; }");
    let mut driver = app.driver(10, 3);
    let rows = frame(&mut driver, None);
    assert_eq!(rows[0].trim_end(), "inside", "{rows:?}");
}

#[test]
fn a_later_text_style_replaces_an_earlier_one() {
    let app = App::new(|| label("x").class("base").name("item"))
        .stylesheet(".base { text-style: bold; } #item { text-style: italic; }");
    let mut driver = app.driver(4, 1);
    frame(&mut driver, None);
    assert_eq!(style_at(&driver, 0, 0), style("italic"));
    let app = App::new(|| label("x").class("base").name("item"))
        .stylesheet(".base { text-style: bold; } #item { text-style: none; }");
    let mut driver = app.driver(4, 1);
    frame(&mut driver, None);
    assert_eq!(
        style_at(&driver, 0, 0),
        style("not bold not dim not italic not underline not reverse not strike not blink")
    );
}

#[test]
fn a_disabled_autofocus_node_does_not_take_the_focus() {
    let app = App::new(|| {
        let picked = signal("");
        column([
            label("first")
                .focusable()
                .on_key("x", move |_| picked.set("first")),
            label("chosen")
                .autofocus()
                .disabled_when(|| true)
                .on_key("x", move |_| picked.set("chosen")),
            text!("picked {picked}"),
        ])
    });
    let mut driver = app.driver(20, 3);
    frame(&mut driver, None);
    let rows = frame(&mut driver, press("x"));
    assert!(row_of(&rows, "picked first").is_some(), "{rows:?}");
}
