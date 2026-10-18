//! Responsive layout (`hide_below`, `@media`, `viewport()`) and the error
//! state (`error_when`, `:error`), headless. Snapshots are in
//! `tests/snapshots`; `UPDATE_SNAPSHOTS=1` writes them again.

use std::cell::Cell;
use std::time::Duration;

use intuituive::a11y::Role;
use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::node::Axis;
use intuituive::prelude::*;
use intuituive::widgets::split;
use intuituive::{assert_screen, assert_screen_styled, Driver, Placement, Stylesheet};

/// An ops dashboard: hosts on the left while there is room, two meters
/// side by side, or one above the other on a narrow terminal.
fn dashboard() -> App {
    App::new(|| {
        column([
            label("[b]ops[/] · 3 hosts").fixed(1),
            row([
                list(
                    || vec!["web-1".into(), "web-2".into(), "db-1".into()],
                    signal(0),
                )
                .name("hosts")
                .panel("Hosts")
                .fixed(12)
                .hide_below(60),
                grid(
                    [],
                    [
                        label("42%").name("cpu").focusable().panel("CPU"),
                        label("1.2 GB").name("memory").panel("Memory"),
                    ],
                )
                .name("meters"),
            ]),
            label("[dim]wider for hosts").show_below(60).fixed(1),
        ])
    })
    .stylesheet(
        "#meters { grid-columns: 1fr 1fr; }
         @media (max-width: 59) { #meters { grid-columns: 1fr; } }",
    )
}

#[test]
fn a_dashboard_reflows_between_wide_and_narrow() {
    let mut driver = dashboard().driver(64, 8);
    driver.settle();
    assert_screen!(driver, "dashboard_wide");
    driver.assert_focused("hosts");

    driver.resize(40, 9);
    driver.settle();
    assert_screen!(driver, "dashboard_narrow");
    // The hosts list is gone, so the focus moved on to what is shown.
    driver.assert_focused("cpu");

    driver.resize(64, 8);
    driver.settle();
    assert_screen!(driver, "dashboard_wide");
}

#[test]
fn the_breakpoint_is_the_viewport_width_in_cells() {
    // `hide_below(60)` hides at 59 columns and shows at 60;
    // `@media (max-width: 59)` holds at 59 and not at 60.
    for (width, hosts, stacked) in [(59, false, true), (60, true, false)] {
        let mut driver = dashboard().driver(width, 9);
        driver.settle();
        let rows = driver.screen().plain();
        let text = rows.join("\n");
        assert_eq!(text.contains("web-1"), hosts, "{width}:\n{text}");
        assert_eq!(text.contains("wider for hosts"), !hosts, "{width}:\n{text}");
        let cpu = rows.iter().position(|r| r.contains("CPU")).unwrap();
        let memory = rows.iter().position(|r| r.contains("Memory")).unwrap();
        assert_eq!(memory > cpu, stacked, "{width}:\n{text}");
    }
}

#[test]
fn a_hidden_node_leaves_the_tab_order_and_the_accessibility_tree() {
    let mut driver = dashboard().driver(40, 9);
    driver.settle();
    let tree = driver.accessibility();
    assert!(tree.iter().all(|node| node.role != Role::List), "{tree:#?}");
    driver.press("tab");
    driver.assert_focused("cpu");
}

#[test]
fn viewport_is_the_apps_width_less_the_inspector() {
    let app = App::new(|| {
        let size = viewport();
        text(move || format!("{:?}", size.get()))
    });
    let mut driver = app.driver(30, 2);
    driver.settle();
    assert_eq!(driver.screen().plain()[0].trim_end(), "(30, 2)");
    driver.resize(50, 4);
    driver.settle();
    assert_eq!(driver.screen().plain()[0].trim_end(), "(50, 4)");

    // The inspector takes 40 of 100 columns: the app lays out in 60.
    let app = App::new(|| {
        let size = viewport();
        text(move || format!("{:?}", size.get()))
    })
    .inspector(true);
    let mut driver = app.driver(100, 10);
    driver.settle();
    assert!(
        driver.screen().plain()[0].starts_with("(60, 10)"),
        "{:?}",
        driver.screen().plain()
    );
    // F12 closes it: the whole width again.
    driver.press("f12");
    assert!(
        driver.screen().plain()[0].starts_with("(100, 10)"),
        "{:?}",
        driver.screen().plain()
    );
}

#[test]
fn code_can_reflow_on_the_viewport() {
    let app = App::new(|| {
        let size = viewport();
        switch(
            move || size.get().0 < 30,
            |narrow| {
                let parts = [label("left").auto(), label("right").auto()];
                if narrow {
                    column(parts)
                } else {
                    row(parts)
                }
            },
        )
    });
    let mut driver = app.driver(40, 2);
    driver.settle();
    assert_eq!(driver.screen().plain()[0].trim_end(), "leftright");
    driver.resize(20, 2);
    driver.settle();
    assert_eq!(
        driver.screen().plain()[..2],
        [
            "left".to_string() + &" ".repeat(16),
            "right".to_string() + &" ".repeat(15)
        ]
    );
}

#[test]
fn height_rules_follow_the_rows() {
    let app = App::new(|| {
        column([
            label("title").fixed(1).hide_below_height(4),
            label("body"),
            label("compact").fixed(1).show_below_height(4),
        ])
    })
    .stylesheet("@media (max-height: 3) { label { text-style: bold; } }");
    let mut driver = app.driver(10, 5);
    driver.settle();
    assert_eq!(driver.screen().plain()[0].trim_end(), "title");
    driver.resize(10, 3);
    driver.settle();
    let rows = driver.screen().plain();
    assert_eq!(rows[0].trim_end(), "body", "{rows:?}");
    assert_eq!(rows[2].trim_end(), "compact", "{rows:?}");
    let style = driver.screen().cell(0, 0).style;
    assert_eq!(
        driver.screen().style(style).and_then(|s| s.attr(0)),
        Some(true)
    );
}

#[test]
fn a_panel_hides_with_what_it_holds() {
    let app = App::new(|| {
        row([
            label("nav").hide_below(20).panel("Nav").fixed(7),
            label("main"),
        ])
    });
    let mut driver = app.driver(12, 3);
    driver.settle();
    let text = driver.screen().plain().join("\n");
    assert!(!text.contains("Nav") && text.contains("main"), "{text}");
}

#[test]
fn hide_below_hides_a_split_pane() {
    let app = App::new(|| {
        split(
            Axis::Horizontal,
            label("SIDEBAR").name("side").focusable().hide_below(60),
            label("main").name("main").focusable(),
            signal(0.5),
        )
    });
    let mut driver = app.driver(40, 2);
    driver.settle();
    driver.assert_focused("main");
    assert!(!driver
        .screen()
        .plain()
        .iter()
        .any(|r| r.contains("SIDEBAR")));
    // The pane still shown takes the whole split, with no divider.
    assert_eq!(driver.screen().plain()[0].trim_end(), "main");
}

#[test]
fn media_display_none_hides_a_split_pane() {
    let app = App::new(|| {
        split(
            Axis::Horizontal,
            label("SIDEBAR").name("side"),
            label("main").name("main"),
            signal(0.5),
        )
    })
    .stylesheet("@media (max-width: 59) { #side { display: none; } }");
    let mut driver = app.driver(40, 2);
    driver.settle();
    assert!(!driver
        .screen()
        .plain()
        .iter()
        .any(|r| r.contains("SIDEBAR")));
    driver.resize(80, 2);
    driver.settle();
    assert!(driver.screen().plain()[0].starts_with("SIDEBAR"));
}

thread_local! {
    /// The clicks that reached [`gone`].
    static CLICKS: Cell<u32> = const { Cell::new(0) };
}

/// `GONE`, hidden below 60 columns by code, or by the stylesheet of
/// [`hidden_in`] when `by_sheet`; it counts the clicks that reach it.
fn gone(by_sheet: bool) -> Node {
    let node = label("GONE")
        .name("gone")
        .on_click(|_| CLICKS.with(|clicks| clicks.set(clicks.get() + 1)));
    if by_sheet {
        node
    } else {
        node.hide_below(60)
    }
}

fn shown() -> Node {
    label("main").name("main").focusable()
}

/// Builds a container holding [`gone`], hidden by the sheet when `true`.
type Build = fn(bool) -> Node;

/// Every kind of container, holding [`gone`] beside a focusable node.
const CONTAINERS: [(&str, Build); 11] = [
    ("row", |s| row([gone(s), shown()])),
    ("column", |s| column([gone(s).fixed(1), shown()])),
    ("grid", |s| {
        grid([Size::Flex(1), Size::Flex(1)], [gone(s), shown()])
    }),
    ("each", |s| {
        column([each(|| vec![0], move |_| gone(s)), shown()])
    }),
    ("hsplit", |s| {
        split(Axis::Horizontal, gone(s), shown(), signal(0.5))
    }),
    ("vsplit", |s| {
        split(Axis::Vertical, gone(s), shown(), signal(0.5))
    }),
    ("scroll", |s| column([scroll(gone(s)), shown()])),
    ("switch", |s| {
        column([switch(|| 0, move |_| gone(s)), shown()])
    }),
    ("panel", |s| column([gone(s).panel("Box"), shown()])),
    ("padding", |s| column([gone(s).padding(0, 1), shown()])),
    ("nested", |s| {
        split(
            Axis::Horizontal,
            scroll(switch(|| 0, move |_| gone(s))),
            shown(),
            signal(0.5),
        )
    }),
];

/// The app of `build`, hiding [`gone`] by code or by the stylesheet.
fn hidden_in(build: Build, by_sheet: bool) -> App {
    let app = App::new(move || build(by_sheet));
    if by_sheet {
        app.stylesheet("@media (max-width: 59) { #gone { display: none; } }")
    } else {
        app
    }
}

fn click(driver: &mut Driver, column: u16, row: u16) {
    for kind in [MouseKind::Down(Button::Left), MouseKind::Up(Button::Left)] {
        driver.event(Event::Mouse(Mouse::new(kind, column, row)));
    }
    driver.settle();
}

#[test]
fn a_hidden_node_is_gone_inside_every_container() {
    for by_sheet in [false, true] {
        for (name, build) in CONTAINERS {
            let what = format!(
                "{name}, hidden by {}",
                ["code", "the sheet"][by_sheet as usize]
            );
            CLICKS.with(|clicks| clicks.set(0));
            let mut driver = hidden_in(build, by_sheet).driver(40, 6);
            driver.settle();
            let rows = driver.screen().plain();
            assert!(
                !rows.iter().any(|r| r.contains("GONE")),
                "{what}: {rows:#?}"
            );
            assert!(rows.iter().any(|r| r.contains("main")), "{what}: {rows:#?}");
            for _ in 0..3 {
                let focused = driver.focused_name();
                assert_ne!(focused.as_deref(), Some("gone"), "{what}: focused");
                driver.press("tab");
            }
            let tree = driver.accessibility();
            assert!(
                tree.iter().all(|node| !node.name.contains("GONE")
                    && !node.value.as_deref().unwrap_or("").contains("GONE")),
                "{what}: {tree:#?}"
            );
            for row in 0..6 {
                for column in 0..40 {
                    click(&mut driver, column, row);
                }
            }
            assert_eq!(CLICKS.with(Cell::get), 0, "{what}: clicked");
            // Wide enough, it is back.
            driver.resize(80, 6);
            driver.settle();
            let rows = driver.screen().plain();
            assert!(rows.iter().any(|r| r.contains("GONE")), "{what}: {rows:#?}");
            // Linear mode writes no line for it.
            let mut driver = hidden_in(build, by_sheet).linear(true).driver(40, 6);
            driver.update(Duration::ZERO);
            let lines = driver.render().unwrap_or_default();
            assert!(!lines.contains("GONE"), "{what}: {lines:?}");
            assert!(lines.contains("main"), "{what}: {lines:?}");
        }
    }
}

#[test]
fn a_hidden_node_is_gone_inside_a_modal_and_a_popup() {
    for popup in [false, true] {
        let app = App::new(move || {
            let under = label("under");
            let anchor = under.id();
            under.on_key("m", move |cx| {
                let build = || split(Axis::Vertical, gone(false), shown(), signal(0.5));
                let (width, height) = (Size::Fixed(30), Size::Fixed(3));
                if popup {
                    cx.popup(anchor, Placement::Below, width, height, build)
                } else {
                    cx.modal(width, height, build)
                }
            })
        });
        CLICKS.with(|clicks| clicks.set(0));
        let mut driver = app.driver(40, 5);
        driver.press("m");
        let rows = driver.screen().plain();
        assert!(!rows.iter().any(|r| r.contains("GONE")), "{rows:#?}");
        assert!(rows.iter().any(|r| r.contains("main")), "{rows:#?}");
        driver.assert_focused("main");
        let tree = driver.accessibility();
        assert!(
            tree.iter().all(|node| !node.name.contains("GONE")),
            "{tree:#?}"
        );
        // Inside both boxes: the modal is centred, the pop-up under `under`
        // (a press outside a pop-up closes it).
        for row in 1..4 {
            for column in 5..30 {
                click(&mut driver, column, row);
            }
        }
        assert_eq!(CLICKS.with(Cell::get), 0, "popup: {popup}");
    }
}

#[test]
fn a_media_query_needs_no_space_after_media() {
    Stylesheet::parse("@media(max-width: 80) { #a { display: none; } }").unwrap();
    let css = "@media(min-width: 1)and (max-width: 59) { #side { display: none; } }";
    let app = App::new(|| row([label("side").name("side"), label("main")])).stylesheet(css);
    let mut driver = app.driver(40, 1);
    driver.settle();
    assert_eq!(driver.screen().plain()[0].trim_end(), "main");
    // Another at-rule, or `@media` run into a word, is still refused.
    for css in ["@import(x) { }", "@mediax (max-width: 3) { }"] {
        let error = Stylesheet::parse(css).unwrap_err().to_string();
        assert!(
            error.contains("only `@media` rules are supported"),
            "{error}"
        );
    }
}

#[test]
fn a_65th_media_block_still_follows_the_viewport() {
    let mut css = String::new();
    for i in 0..64 {
        css.push_str(&format!(
            "@media (min-width: 1) {{ #pad{i} {{ size: 1; }} }}\n"
        ));
    }
    css.push_str("@media (min-width: 60) { #wide { display: none; } }\n");
    let app = App::new(|| column([label("WIDE-ONLY-HIDDEN").name("wide"), label("main")]))
        .stylesheet(&css);
    let mut driver = app.driver(40, 2);
    driver.settle();
    assert!(driver.screen().plain()[0].starts_with("WIDE-ONLY-HIDDEN"));
    driver.resize(80, 2);
    driver.settle();
    let rows = driver.screen().plain();
    assert!(
        !rows.iter().any(|r| r.contains("WIDE-ONLY-HIDDEN")),
        "{rows:#?}"
    );
}

#[test]
fn nothing_changes_for_apps_without_responsive_rules() {
    let app = || {
        App::new(|| column([label("a").panel("A"), label("b")])).stylesheet("label { color: red; }")
    };
    let mut driver = app().driver(20, 6);
    driver.settle();
    let wide = driver.stats();
    driver.resize(16, 6);
    driver.settle();
    // A resize draws everything once, as it always has.
    assert!(driver.stats().drawn >= wide.drawn.min(1));
    driver.settle();
    assert_eq!(driver.render(), None, "nothing left to draw");
}

/// A sign-up form: the age must be a number.
fn form(sheet: Option<&str>) -> App {
    let app = App::new(|| {
        let age = signal(String::from("forty"));
        column([
            text!("Age: {age}")
                .name("age")
                .label("Age")
                .focusable()
                .error_when(move || age.with(|a| a.parse::<u8>().is_err()))
                .on_key("f", move |_| age.set("40".into())),
            label("Name: Ada").name("name"),
            label("ok")
                .panel("Status")
                .error_when(move || age.with(|a| a.parse::<u8>().is_err())),
        ])
    });
    match sheet {
        Some(css) => app.stylesheet(css),
        None => app,
    }
}

#[test]
fn error_state_takes_the_themes_look() {
    let mut driver = form(None).driver(16, 5);
    driver.settle();
    assert_screen_styled!(driver, "form_error_theme");
    // Fixed: the error look goes.
    driver.press("f");
    assert_screen_styled!(driver, "form_fixed_theme");
}

#[test]
fn error_state_is_styled_by_the_stylesheet() {
    let sheet = "text:error { background: red; color: white; } \
                 #name:error { color: green; }";
    let mut driver = form(Some(sheet)).driver(16, 5);
    driver.settle();
    assert_screen_styled!(driver, "form_error_sheet");
    driver.press("f");
    assert_screen_styled!(driver, "form_fixed_theme");
}

#[test]
fn the_theme_names_the_error_styles() {
    let theme = Theme::dark()
        .with_config("[styles]\nerror = magenta\nborder.error = yellow\n")
        .unwrap();
    let mut driver = form(None).theme(theme).driver(16, 5);
    driver.settle();
    let screen = driver.screen();
    let style = |x: u16, y: u16| screen.style(screen.cell(x, y).style).cloned();
    let magenta = rich::Color::parse("magenta").unwrap();
    let yellow = rich::Color::parse("yellow").unwrap();
    assert_eq!(style(0, 0).and_then(|s| s.color().cloned()), Some(magenta));
    // The panel's corner, not its content.
    assert_eq!(style(0, 2).and_then(|s| s.color().cloned()), Some(yellow));
    assert_eq!(style(1, 3).and_then(|s| s.color().cloned()), None);
}

#[test]
fn an_error_is_told_to_assistive_technology() {
    let mut driver = form(None).driver(16, 5);
    driver.settle();
    let tree = driver.accessibility();
    let age = tree.iter().find(|node| node.name == "Age").unwrap();
    assert!(age.state.invalid);
    assert_eq!(age.describe(), "Age, invalid");
    assert!(age
        .aria_attributes()
        .contains(&("aria-invalid", "true".to_string())));
    driver.press("f");
    let tree = driver.accessibility();
    let age = tree.iter().find(|node| node.name == "Age").unwrap();
    assert!(!age.state.invalid);
    assert!(!age
        .aria_attributes()
        .iter()
        .any(|(k, _)| *k == "aria-invalid"));
}

#[test]
fn linear_mode_says_invalid_and_says_it_again_when_fixed() {
    let mut driver = form(None).linear(true).driver(30, 5);
    driver.update(std::time::Duration::ZERO);
    let first = driver.render().unwrap();
    assert!(first.contains("Age, invalid"), "{first:?}");
    driver.event(intuituive::interact::Event::Key(
        intuituive::interact::Key::parse("f").unwrap(),
    ));
    driver.update(std::time::Duration::ZERO);
    let second = driver.render().unwrap();
    assert!(
        second.contains("Age") && !second.contains("invalid"),
        "{second:?}"
    );
}
