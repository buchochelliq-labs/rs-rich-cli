//! Panel titles, borders, sides and shadows (0.0.19 gap A1).

use intuituive::prelude::*;
use intuituive::{assert_screen, assert_screen_styled, BorderSet, Driver};

fn shown(app: App, width: u16, height: u16) -> Driver {
    let mut driver = app.driver(width, height);
    driver.settle();
    driver
}

fn style(definition: &str) -> rich::Style {
    rich::Style::parse(definition).unwrap()
}

fn style_at(driver: &Driver, x: u16, y: u16) -> Option<rich::Style> {
    let screen = driver.screen();
    screen.style(screen.cell(x, y).style).cloned()
}

#[test]
fn panel_titles() {
    let app = App::new(|| {
        column([
            label("left").panel("Left"),
            label("centre").panel("Centre").title_align(Align::Center),
            label("right").panel("Right").title_align(Align::End),
            label("two titles at the bottom")
                .panel("")
                .title_bottom("q quits")
                .add_title(Edge::Bottom, Align::End, "1/20"),
        ])
    });
    assert_screen!(shown(app, 30, 12), "panel_titles");
}

#[test]
fn titles_side_by_side_give_way_right_first_then_centred() {
    let app = |width| {
        let app = App::new(|| {
            label("")
                .panel("Left title")
                .title_right("Right")
                .add_title(Edge::Top, Align::Center, "Mid")
        });
        shown(app, width, 3).screen().plain()[0].clone()
    };
    let wide = app(40);
    assert!(
        wide.contains(" Left title ") && wide.contains(" Mid ") && wide.contains(" Right "),
        "{wide}"
    );
    // Too narrow for the centred one beside the left one: it gives way, and
    // the right one, which fits, stays.
    let narrower = app(30);
    assert!(!narrower.contains("Mid"), "{narrower}");
    assert!(
        narrower.contains(" Left title ") && narrower.contains(" Right "),
        "{narrower}"
    );
    // Room for the left one only, and then less: it is cut with `…`.
    assert_eq!(app(16), "╭─ Left title ─╮");
    assert_eq!(app(12), "╭─ Left … ─╮");
    // Two left titles sit side by side, one edge glyph apart.
    let app = App::new(|| {
        label("")
            .panel("One")
            .add_title(Edge::Top, Align::Start, "Two")
    });
    assert_eq!(shown(app, 16, 3).screen().plain()[0], "╭─ One ─ Two ──╮");
}

#[test]
fn panel_sides() {
    let app = App::new(|| {
        column([
            label("top only").panel("Top").borders(Sides::TOP),
            label("left and right")
                .panel("Sides")
                .borders(Sides::LEFT | Sides::RIGHT),
            label("bottom only").panel("").borders(Sides::BOTTOM),
        ])
    });
    let driver = shown(app, 20, 7);
    assert_screen!(driver, "panel_sides");
    let rows = driver.screen().plain();
    // The inside grows into the sides not drawn: the label starts at the
    // left edge under a top-only border.
    assert!(rows[1].starts_with("top only"), "{rows:?}");
    // A title keeps its row with that side off, between the sides.
    assert!(rows[2].starts_with("│Sides"), "{rows:?}");
}

#[test]
fn panel_kinds() {
    let kinds = [
        Border::Rounded,
        Border::Plain,
        Border::Thick,
        Border::Double,
        Border::Ascii,
        Border::LightDoubleDashed,
        Border::HeavyDoubleDashed,
        Border::LightTripleDashed,
        Border::HeavyTripleDashed,
        Border::LightQuadrupleDashed,
        Border::HeavyQuadrupleDashed,
        Border::QuadrantInside,
        Border::QuadrantOutside,
        Border::Custom(BorderSet {
            top: "~",
            bottom: "~",
            ..BorderSet::PLAIN
        }),
    ];
    let app = App::new(move || {
        grid(
            [Size::Flex(1); 7],
            kinds.iter().map(|kind| label("").panel("").border(*kind)),
        )
        .rows([Size::Fixed(3)])
    });
    assert_screen!(shown(app, 28, 6), "panel_kinds");
}

#[test]
fn panel_title_markup() {
    let app = App::new(|| {
        label("body")
            .panel("[b]Pull[/] [red]requests[/]")
            .title_right("[dim]loading")
            .title_style("italic")
    });
    let driver = shown(app, 30, 3);
    assert_screen_styled!(driver, "panel_title_markup");
    // Markup over the title style, over the border's colour.
    let border = style("blue");
    assert_eq!(
        style_at(&driver, 3, 0),
        Some(border.combine(&style("italic bold")))
    );
    // The accessible name is the first title, as plain text.
    assert_eq!(driver.accessibility()[0].name, "Pull requests");
}

#[test]
fn panel_title_no_border() {
    let app = App::new(|| {
        column([
            label("under a title")
                .panel("[b]Stats")
                .border(Border::None),
            label("next")
                .panel("")
                .border(Border::None)
                .title_bottom("end"),
        ])
    });
    let driver = shown(app, 16, 4);
    assert_screen!(driver, "panel_title_no_border");
    // No lines: the title takes its row and the content the rest.
    assert_eq!(driver.screen().plain()[0].trim_end(), "Stats");
    assert_eq!(driver.screen().plain()[1].trim_end(), "under a title");
}

#[test]
fn panel_shadow() {
    let app = App::new(|| {
        row([
            label("medium").panel("A").shadow(Shadow::MediumShade),
            label("block").panel("B").shadow(Shadow::Block),
            label("own").panel("C").shadow(Shadow::Symbol('$')),
        ])
    });
    assert_screen!(shown(app, 33, 4), "panel_shadow");
    // A theme's `shadow` style colours it.
    let app = App::new(|| label("x").panel("").shadow(Shadow::DarkShade))
        .theme(Theme::dark().style("shadow", style("red")));
    let driver = shown(app, 5, 4);
    assert_eq!(style_at(&driver, 4, 1), Some(style("red")));
    assert_eq!(driver.screen().plain()[3], " ▓▓▓▓");
}

#[test]
fn the_focus_still_recolours_the_border() {
    let app = App::new(|| {
        column([
            label("one")
                .focusable()
                .panel("One")
                .border(Border::Thick)
                .border_style("green"),
            label("two")
                .focusable()
                .panel("Two")
                .title_align(Align::Center),
        ])
    })
    .theme(Theme {
        border: style("blue"),
        border_focused: style("red"),
        ..Theme::default()
    });
    let mut driver = shown(app, 16, 6);
    // The first has the focus: red, thick; the other blue.
    assert_eq!(style_at(&driver, 0, 0), Some(style("red")));
    assert_eq!(driver.screen().plain()[0], "┏━ One ━━━━━━━━┓");
    assert_eq!(style_at(&driver, 0, 3), Some(style("blue")));
    driver.press("tab");
    // Code's border style while the focus is elsewhere, the theme's
    // focused one while it is inside.
    assert_eq!(style_at(&driver, 0, 0), Some(style("green")));
    assert_eq!(style_at(&driver, 0, 3), Some(style("red")));
    assert_eq!(driver.screen().plain()[3], "╭──── Two ─────╮");
}

#[test]
fn a_stylesheet_draws_what_the_code_does() {
    let code = App::new(|| {
        label("body")
            .panel("Files")
            .title_align(Align::Center)
            .title_bottom("q quits")
            .border(Border::Thick)
            .borders(Sides::TOP | Sides::BOTTOM)
    });
    let sheet = App::new(|| label("body").panel("Files")).stylesheet(
        "panel { border: thick; border-title-align: center; border-subtitle: \"q quits\"; \
         border-sides: top bottom; }",
    );
    let (code, sheet) = (shown(code, 20, 4), shown(sheet, 20, 4));
    assert_eq!(code.screen().plain(), sheet.screen().plain());
    assert_eq!(code.screen().plain()[0], "━━━━━━ Files ━━━━━━━");
    // Code wins over the sheet.
    let app = App::new(|| {
        label("x")
            .panel("T")
            .title_align(Align::End)
            .border(Border::Double)
    })
    .stylesheet("panel { border: round; border-title-align: center; }");
    assert_eq!(shown(app, 12, 3).screen().plain()[0], "╔══════ T ═╗");
}

#[test]
fn a_sheets_border_on_any_node_takes_titles_and_sides() {
    let app = App::new(|| label("inside").name("box")).stylesheet(
        "#box { border: dashed; border-title: Box; border-title-align: right; \
         border-subtitle: end; border-sides: top bottom left; }",
    );
    let rows = shown(app, 14, 4).screen().plain();
    assert_eq!(rows[0], "┌╌╌╌╌╌╌ Box ╌╌");
    assert_eq!(rows[1], "╎inside       ");
    assert_eq!(rows[3], "└╌ end ╌╌╌╌╌╌╌");
}

#[test]
fn narrow_panels_with_titles_still_draw() {
    for width in 1..8 {
        let app = App::new(|| {
            label("x")
                .panel("Title")
                .title_right("R")
                .title_bottom("B")
                .shadow(Shadow::LightShade)
        });
        let rows = shown(app, width, 4).screen().plain();
        assert_eq!(rows.len(), 4, "{width}: {rows:?}");
    }
}
