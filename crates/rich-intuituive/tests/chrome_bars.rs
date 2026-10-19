//! Round 3 of the demo gaps, widgets: a list's highlight under its markup,
//! one closure per list row, text cut from its start, a status bar of
//! powerline segments, a breadcrumb, key hints, a row of percentage cells,
//! a live node with a frame cap, a tab strip that hides and reports picks,
//! and a list that leaves the wheel to its ancestors.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::widgets::{
    breadcrumb, cell, cells, key_hints, list_lazy, list_rows, live, seg, status_bar, tabs, Caps,
    HighlightLayer, ListRow,
};
use intuituive::{Driver, Solver};

fn shown(app: App, width: u16, height: u16) -> Driver {
    let mut driver = app.driver(width, height);
    driver.settle();
    driver
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

fn styled(driver: &Driver) -> Vec<String> {
    intuituive::testing::styled(driver)
        .lines()
        .map(str::to_string)
        .collect()
}

/// Yazi's hovered directory: rounded ends in the entry's colour, the name
/// reversed in it.
fn hovered(layer: Option<HighlightLayer>) -> Driver {
    let app = App::new(move || {
        let items = || {
            vec![
                "[not reverse blue]\u{e0b6}[/]docs[not reverse blue]\u{e0b4}[/]".to_string(),
                "src".into(),
            ]
        };
        let list = list(items, signal(0)).row_style(|_| Some("blue".into()));
        match layer {
            Some(layer) => list.highlight_layer(layer),
            None => list,
        }
    });
    shown(app, 8, 2)
}

#[test]
fn a_highlight_under_the_markup_leaves_what_the_markup_styles() {
    let under = styled(&hovered(Some(HighlightLayer::Under)));
    assert_eq!(
        under[0],
        "[not reverse blue]\u{e0b6}[/][reverse blue]docs[/][not reverse blue]\u{e0b4}[/]\
         [reverse blue]  [/]"
    );
    // Over (the default), the highlight covers the whole row, ends too.
    let over = styled(&hovered(None));
    assert_eq!(over, styled(&hovered(Some(HighlightLayer::Over))));
    assert_eq!(over[0], "[reverse blue]\u{e0b6}docs\u{e0b4}  [/]");
}

#[test]
fn list_rows_makes_each_row_once_a_draw() {
    let made = Rc::new(Cell::new(0));
    let count = made.clone();
    let app = App::new(move || {
        let names = signal(
            (0..100)
                .map(|i| format!("file {i}"))
                .collect::<Vec<String>>(),
        );
        list_rows(
            move || names.with(Vec::len),
            move |i| {
                count.set(count.get() + 1);
                let name = names.with(|n| n[i].clone());
                ListRow::new(name)
                    .trailing(format!("{i}K"))
                    .marker((i % 2 == 0).then(|| "on yellow".into()))
                    .style(Some("green".into()))
                    .selected_style(Some("reverse green".into()))
            },
            signal(0),
        )
        .marker_column()
    });
    let driver = shown(app, 16, 4);
    assert_eq!(rows(&driver)[0], " file 0       0K");
    assert_eq!(rows(&driver)[3], " file 3       3K");
    // Four rows in view: each made once a draw, though four of its parts
    // were asked for.
    let draws = made.get() / 4;
    assert_eq!(made.get() % 4, 0, "{} rows made", made.get());
    assert!(draws <= 2, "{} rows made", made.get());
    let screen = styled(&driver);
    assert!(
        screen[0].starts_with("[on yellow] [/][reverse green]file 0"),
        "{screen:?}"
    );
    assert!(screen[1].starts_with(" [green]file 1"), "{screen:?}");
}

#[test]
fn list_lazy_still_asks_each_closure() {
    // The old form keeps working, each option its own closure.
    let asked = Rc::new(Cell::new(0));
    let count = asked.clone();
    let app = App::new(move || {
        let count = count.clone();
        list_lazy(|| 10, |i| format!("item {i}"), signal(0)).trailing(move |i| {
            count.set(count.get() + 1);
            i.to_string()
        })
    });
    let driver = shown(app, 10, 2);
    assert_eq!(rows(&driver), ["item 0   0", "item 1   1"]);
    assert!(asked.get() >= 2);
}

#[test]
fn text_cut_from_its_start_keeps_its_end() {
    let app = App::new(|| {
        column([
            label("[cyan]~/projects/rich/src[/]")
                .truncate_start()
                .fixed(1),
            label("short").truncate_start().fixed(1),
            label("one/two/three\nfour").truncate_start().fixed(2),
            label("～～～～end").truncate_start().fixed(1),
        ])
    });
    let driver = shown(app, 8, 5);
    let screen = rows(&driver);
    assert_eq!(screen[0], "…ich/src");
    assert_eq!(screen[1], "short");
    // Each line on its own, and no wrapping.
    assert_eq!(screen[2], "…o/three");
    assert_eq!(screen[3], "four");
    // Wide characters are kept whole.
    assert_eq!(screen[4], "…～～end");
    // What is kept keeps its style.
    assert!(styled(&driver)[0].starts_with("…[cyan]ich/src"));
}

fn status(caps: Caps, width: u16) -> Driver {
    let app = App::new(move || {
        let visual = signal(false);
        status_bar(
            [
                seg(move || if visual.get() { " SEL " } else { " NOR " }.into()).style_with(
                    move || {
                        if visual.get() {
                            "bold on red"
                        } else {
                            "bold on blue"
                        }
                        .into()
                    },
                ),
                seg(|| " 4K ".into()).style("on white"),
                seg(|| " a-rather-long-name.txt".into()),
                seg(String::new).style("on green"),
            ],
            [
                seg(|| "rw- ".into()),
                seg(|| " Top ".into()).style("on white"),
                seg(|| " 1/9 ".into()).style("on blue"),
            ],
        )
        .caps(caps)
        .on_key("v", move |_| visual.update(|v| *v = !*v))
        .focusable()
    });
    shown(app, width, 1)
}

#[test]
fn a_status_bar_joins_its_segments_with_caps() {
    let driver = status(Caps::Powerline, 60);
    assert_eq!(
        rows(&driver)[0],
        " NOR \u{e0b0} 4K \u{e0b0} a-rather-long-name.txt          rw- \u{e0b2} Top \u{e0b2} 1/9"
    );
    // Each cap in the colour of the segment it ends, on the next one's.
    let screen = styled(&driver);
    assert!(
        screen[0].contains("[blue on white]\u{e0b0}[/]"),
        "{screen:?}"
    );
    assert!(screen[0].contains("[white]\u{e0b0}[/]"), "{screen:?}");
    assert!(
        screen[0].contains("[blue on white]\u{e0b2}[/]"),
        "{screen:?}"
    );
    // Rounded ends round the outer edges too.
    let rounded = rows(&status(Caps::Rounded, 60))[0].clone();
    assert!(rounded.starts_with("\u{e0b6} NOR \u{e0b4}"), "{rounded}");
    assert!(rounded.ends_with(" 1/9 \u{e0b4}"), "{rounded}");
    // No caps: the segments side by side.
    assert!(rows(&status(Caps::None, 60))[0].starts_with(" NOR  4K  a-rather"));
}

#[test]
fn a_status_bar_follows_its_signals_and_cuts_the_left_group() {
    let mut driver = status(Caps::Powerline, 34);
    // The right group whole, the left cut with `…`.
    assert_eq!(
        rows(&driver)[0],
        " NOR \u{e0b0} 4K \u{e0b0} a-rat…rw- \u{e0b2} Top \u{e0b2} 1/9"
    );
    driver.press("v");
    assert!(rows(&driver)[0].starts_with(" SEL "));
    assert!(styled(&driver)[0].starts_with("[bold on red] SEL [/][red on white]"));
}

#[test]
fn a_breadcrumb_cuts_from_the_left_and_keeps_its_suffix() {
    let parts = |all: &[&str]| all.iter().map(|p| p.to_string()).collect::<Vec<_>>();
    let app = App::new(move || {
        let trail = signal(parts(&["", "usr", "local", "share", "doc"]));
        breadcrumb(move || trail.get())
            .crumb_separator("/")
            .crumb_suffix(|| " (filter: md)".into())
            .on_key("u", move |_| trail.set(parts(&["", ""])))
            .focusable()
    });
    let mut driver = shown(app, 40, 1);
    // A leading empty part puts the separator first.
    assert_eq!(rows(&driver)[0], "/usr/local/share/doc (filter: md)");
    // The last part is bold.
    assert!(styled(&driver)[0].contains("[bold]doc[/]"));
    let app = App::new(move || {
        breadcrumb(move || parts(&["", "usr", "local", "share", "doc"]))
            .crumb_separator("/")
            .crumb_suffix(|| " (f)".into())
    });
    assert_eq!(rows(&shown(app, 14, 1))[0], "…share/doc (f)");
    driver.press("u");
    assert_eq!(rows(&driver)[0], "/ (filter: md)");
    // Any trail: the default separator is ` › `.
    let app = App::new(move || breadcrumb(move || parts(&["Settings", "Display"])));
    assert_eq!(rows(&shown(app, 30, 1))[0], "Settings › Display");
}

#[test]
fn key_hints_follow_the_focus_and_a_pop_up() {
    let app = App::new(|| {
        column([
            row([
                label("left").bind("d", "delete", |_| {}).focusable(),
                label("right").bind("r", "rename", |_| {}).focusable(),
            ])
            .fixed(1),
            key_hints(),
        ])
        .bind("q", "quit", |cx| cx.quit())
        .bind_sequence("g g", "top", |_| {})
        .on_key("o", |cx| {
            cx.modal(Size::Fixed(10), Size::Fixed(3), || {
                label("hi").bind("esc", "close", |cx| cx.pop())
            })
        })
    });
    let mut driver = shown(app, 40, 6);
    assert_eq!(rows(&driver)[1], "d delete · q quit");
    driver.press("tab");
    assert_eq!(rows(&driver)[1], "r rename · q quit");
    driver.press("o");
    // Under the modal, the hints are its own.
    assert!(
        rows(&driver)[1].starts_with("esc close  "),
        "{:?}",
        rows(&driver)
    );
    assert!(!rows(&driver)[1].contains("quit"));
    // Too narrow for both: the first only.
    let app = App::new(|| {
        column([label("x"), key_hints()])
            .bind("q", "quit", |_| {})
            .bind("s", "save", |_| {})
    });
    assert_eq!(rows(&shown(app, 12, 2))[1], "q quit");
}

#[test]
fn cells_lay_out_as_a_hand_made_ratatui_header() {
    for width in [40, 61, 80, 117, 160] {
        let by_hand = App::new(|| {
            row([
                cell(|| "oscilloscope::scope-tui".into()).percent(35),
                cell(|| "live".into()).percent(25),
                cell(|| "-1.00x+".into()).percent(7),
                cell(|| "60fps".into()).percent(33),
            ])
            .solver(Solver::Ratatui)
            .gap(1)
            .fixed(1)
        });
        let with_cells = App::new(|| {
            cells([
                (35, label("oscilloscope::scope-tui")),
                (25, label("live")),
                (7, label("-1.00x+")),
                (33, text!("60fps")),
            ])
        });
        assert_eq!(
            rows(&shown(by_hand, width, 1)),
            rows(&shown(with_cells, width, 1)),
            "at {width}"
        );
    }
}

#[test]
fn a_live_node_draws_at_its_cap_and_shows_the_last_change() {
    let made = Rc::new(Cell::new(0));
    let count = made.clone();
    let app = App::new(move || {
        let sample = signal(0);
        let stop = signal(false);
        every(Duration::from_millis(5), move |_| {
            if !stop.get_untracked() {
                sample.update(|s| *s += 1)
            }
        });
        let count = count.clone();
        column([live(20, move || {
            count.set(count.get() + 1);
            format!("{}", sample.get())
        })])
        .on_key("s", move |_| stop.set(true))
    });
    let mut driver = shown(app, 10, 1);
    driver.advance(Duration::from_secs(1));
    // 200 changes in a second, drawn 20 times (and the first draw).
    assert!(made.get() <= 22, "drawn {} times", made.get());
    assert!(made.get() >= 18, "drawn {} times", made.get());
    // The last change is shown, once its time comes.
    driver.press("s");
    driver.advance(Duration::from_millis(100));
    let last: u32 = rows(&driver)[0].parse().unwrap();
    assert!(last >= 199, "{last}");
}

#[test]
fn a_tab_strip_hides_below_two_tabs_and_reports_clicks() {
    let app = App::new(|| {
        let titles = signal(vec!["one".to_string()]);
        let active = signal(0usize);
        let picked = signal(String::from("none"));
        column([
            tabs(move || titles.get(), active)
                .auto_hide()
                .on_select(move |to, _| picked.set(format!("picked {to}")))
                .fixed(1),
            text!("{picked} active {active}"),
        ])
        .on_key("t", move |_| titles.update(|t| t.push("two".into())))
    });
    let mut driver = shown(app, 20, 2);
    assert_eq!(rows(&driver)[0], "none active 0");
    driver.press("t");
    assert_eq!(rows(&driver)[0], " one │ two");
    // A click on "two": the handler is told, and the signal left to it.
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        8,
        0,
    )));
    driver.settle();
    assert_eq!(rows(&driver)[1], "picked 1 active 0");
}

#[test]
fn a_list_leaves_the_wheel_to_its_ancestors_at_step_zero() {
    let draw = |step: Option<usize>| {
        let app = App::new(move || {
            let turns = signal(0);
            let at = signal(0usize);
            let items = || (0..20).map(|i| i.to_string()).collect();
            let list = list(items, at);
            let list = match step {
                Some(step) => list.wheel_step(step),
                None => list,
            };
            column([list.fixed(3), text!("{at} {turns}")]).on_mouse(move |_, mouse| {
                turns.update(|t| *t += 1);
                mouse.kind == MouseKind::ScrollDown
            })
        });
        let mut driver = shown(app, 6, 4);
        driver.event(Event::Mouse(Mouse::new(MouseKind::ScrollDown, 0, 0)));
        driver.settle();
        rows(&driver)[3].clone()
    };
    assert_eq!(draw(None), "3 0");
    assert_eq!(draw(Some(5)), "5 0");
    assert_eq!(draw(Some(0)), "0 1");
}
