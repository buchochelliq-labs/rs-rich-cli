//! Flex alignment: `align_items`, `align_self`, `justify`, `wrap` and
//! `align_content`, from code and from a stylesheet.

mod common;

use common::{row_of, run, screen};
use intuituive::prelude::*;
use intuituive::AlignContent;
use rich_interact::headless::Script;

fn show(app: App, width: u16, height: u16) -> Vec<String> {
    screen(&run(app, Script::new().keys("q"), width, height))
}

fn fixed(text: &'static str, cells: u16) -> Node {
    label(text).size(Size::Fixed(cells))
}

#[test]
fn align_items_places_children_across_a_column() {
    for (align, at) in [
        (Align::Start, 0),
        (Align::Center, 4),
        (Align::End, 8),
        (Align::Stretch, 0),
    ] {
        let app = App::new(move || {
            column([label("ab").auto()])
                .align_items(align)
                .on_key("q", |cx| cx.quit())
        });
        let rows = show(app, 10, 3);
        assert_eq!(rows[0].find("ab"), Some(at), "{align:?}: {rows:?}");
    }
}

#[test]
fn align_self_wins_over_align_items() {
    let app = App::new(|| {
        row([fixed("a", 3), fixed("b", 3).align_self(Align::Start)])
            .align_items(Align::End)
            .on_key("q", |cx| cx.quit())
    });
    let rows = show(app, 10, 4);
    assert_eq!(row_of(&rows, "a"), Some(3), "{rows:?}");
    assert_eq!(row_of(&rows, "b"), Some(0), "{rows:?}");
}

#[test]
fn justify_shares_the_room_children_leave() {
    for (justify, a, b) in [
        (Justify::Start, 0, 2),
        (Justify::Center, 3, 5),
        (Justify::End, 6, 8),
        (Justify::SpaceBetween, 0, 8),
        (Justify::SpaceAround, 1, 6),
        (Justify::SpaceEvenly, 2, 6),
    ] {
        let app = App::new(move || {
            row([fixed("a", 2), fixed("b", 2)])
                .justify(justify)
                .on_key("q", |cx| cx.quit())
        });
        let rows = show(app, 10, 1);
        assert_eq!(rows[0].find('a'), Some(a), "{justify:?}: {rows:?}");
        assert_eq!(rows[0].find('b'), Some(b), "{justify:?}: {rows:?}");
    }
}

#[test]
fn a_flexible_child_leaves_nothing_to_justify() {
    let app = App::new(|| {
        row([fixed("a", 2), label("b")])
            .justify(Justify::End)
            .on_key("q", |cx| cx.quit())
    });
    let rows = show(app, 10, 1);
    assert_eq!(rows[0].find('a'), Some(0), "{rows:?}");
    assert_eq!(rows[0].find('b'), Some(2), "{rows:?}");
}

#[test]
fn justify_keeps_docked_children_at_their_edges() {
    let app = App::new(|| {
        row([
            fixed("m", 2),
            fixed("r", 1).name("right"),
            fixed("l", 1).name("left"),
        ])
        .justify(Justify::Center)
        .on_key("q", |cx| cx.quit())
    })
    .stylesheet("#left { dock: left; } #right { dock: right; }");
    let rows = show(app, 12, 1);
    assert_eq!(rows[0].find('l'), Some(0), "{rows:?}");
    assert_eq!(rows[0].find('r'), Some(11), "{rows:?}");
    // Between them, 10 cells for a 2-cell child: 4 either side.
    assert_eq!(rows[0].find('m'), Some(5), "{rows:?}");
}

#[test]
fn a_wrapping_row_starts_a_new_line_for_what_does_not_fit() {
    let app = App::new(|| {
        row(["a", "b", "c", "d", "e"].map(|t| fixed(t, 4)))
            .gap(1)
            .wrap()
            .align_content(AlignContent::Start)
            .on_key("q", |cx| cx.quit())
    });
    // Three fit in 14 cells (4 + 1 + 4 + 1 + 4); the next two wrap, a gap
    // below the first line.
    let rows = show(app, 14, 6);
    assert_eq!(rows[0], "a    b    c", "{rows:?}");
    assert_eq!(rows[1], "", "{rows:?}");
    assert_eq!(rows[2], "d    e", "{rows:?}");
}

#[test]
fn wrapped_lines_justify_on_their_own() {
    let app = App::new(|| {
        row(["a", "b", "c"].map(|t| fixed(t, 4)))
            .wrap()
            .justify(Justify::End)
            .align_content(AlignContent::Start)
            .on_key("q", |cx| cx.quit())
    });
    // Two fit in 10 cells; the line of one ends at the right too.
    let rows = show(app, 10, 3);
    assert_eq!(rows[0].find('a'), Some(2), "{rows:?}");
    assert_eq!(rows[1].find('c'), Some(6), "{rows:?}");
}

#[test]
fn an_auto_wrapping_row_is_as_tall_as_its_lines() {
    let app = App::new(|| {
        column([
            row(["a", "b", "c"].map(|t| fixed(t, 4))).wrap().auto(),
            label("below").auto(),
        ])
        .on_key("q", |cx| cx.quit())
    });
    let rows = show(app, 10, 5);
    assert_eq!(row_of(&rows, "below"), Some(2), "{rows:?}");
}

#[test]
fn the_stylesheet_sets_alignment_and_code_wins() {
    let app = App::new(|| {
        column([
            row([fixed("a", 2)]).name("sheet"),
            row([fixed("b", 2)]).name("code").justify(Justify::Start),
            column([label("c").auto()]).name("cross"),
        ])
        .on_key("q", |cx| cx.quit())
    })
    .stylesheet(
        "row, column { size: 1; } #sheet, #code { justify-content: flex-end; } \
         #cross { align-items: center; }",
    );
    let rows = show(app, 10, 3);
    assert_eq!(rows[0].find('a'), Some(8), "{rows:?}");
    assert_eq!(rows[1].find('b'), Some(0), "code wins: {rows:?}");
    assert_eq!(rows[2].find('c'), Some(4), "{rows:?}");
}

#[test]
fn the_inspector_shows_alignment() {
    let app = App::new(|| {
        column([label("a").auto().align_self(Align::End)])
            .justify(Justify::Center)
            .align_items(Align::Center)
            .on_key("q", |cx| cx.quit())
    })
    .inspector(true);
    let rows = show(app, 100, 12);
    let text = rows.join("\n");
    // The panel cuts the line at its width.
    assert!(
        text.contains("column align-items:center justify:c"),
        "{text}"
    );
    assert!(text.contains("label align-self:flex-end"), "{text}");
}

#[test]
fn wrapped_flexible_children_start_from_their_clamped_content() {
    let app = App::new(|| {
        row(["a", "b", "c"].map(|t| label(t).min_size(4)))
            .wrap()
            .align_content(AlignContent::Start)
            .on_key("q", |cx| cx.quit())
    });
    // Each starts at 4 (its minimum, above its 1-cell content): two fit in
    // 10, and share the line; the third has a line to itself.
    let rows = show(app, 10, 3);
    assert_eq!(rows[0].find('a'), Some(0), "{rows:?}");
    assert_eq!(rows[0].find('b'), Some(5), "{rows:?}");
    assert_eq!(rows[1].find('c'), Some(0), "{rows:?}");
}
