//! Flex alignment and wrapping against CSS, at the edges (the 0.0.19
//! release test).

mod common;

use common::{run, screen};
use intuituive::prelude::*;
use intuituive::AlignContent;
use rich_interact::headless::Script;

fn show(app: App, width: u16, height: u16) -> Vec<String> {
    screen(&run(app, Script::new().keys("q"), width, height))
}

/// Four 25% children, gap 2, in a 20-cell wrapping row. CSS: each is 5
/// cells (25% of the container), three fit on the first line (5+2+5+2+5 =
/// 19), the fourth wraps. All four are the same width.
#[test]
fn wrapped_percentages_are_of_the_container() {
    let app = App::new(|| {
        row(["aaaaaaaaaa", "bbbbbbbbbb", "cccccccccc", "dddddddddd"]
            .map(|t| label(t).size(Size::Percent(25))))
        .gap(2)
        .wrap()
        .align_content(AlignContent::Start)
        .on_key("q", |cx| cx.quit())
    });
    let rows = show(app, 20, 10);
    eprintln!("{rows:#?}");
    let a = rows[0].matches('a').count();
    let d = rows
        .iter()
        .find(|r| r.contains('d'))
        .map_or(0, |r| r.matches('d').count());
    assert_eq!(
        (a, d),
        (5, 5),
        "widths of a (line 1) and d (line 2): {rows:?}"
    );
}

/// Two flexible children in a wrapping row: the line was broken so that
/// both contents (7 + 1) fit, and the docs say the flexible ones start at
/// their content and share what the line leaves. CSS (`flex: 1`, min-width
/// auto) gives "aaaaaaa" its 7 cells.
#[test]
fn wrapped_flexible_children_keep_their_content() {
    let app = App::new(|| {
        row([label("aaaaaaa"), label("b")])
            .wrap()
            .align_content(AlignContent::Start)
            .on_key("q", |cx| cx.quit())
    });
    let rows = show(app, 10, 2);
    eprintln!("{rows:#?}");
    assert_eq!(rows[0].matches('a').count(), 7, "{rows:?}");
}

/// align_self set before padding/panel is lost: the wrapper node takes the
/// size and span, but not the alignment.
#[test]
fn align_self_survives_padding() {
    let app = App::new(|| {
        column([label("ab").auto().align_self(Align::End).padding(0, 0)])
            .on_key("q", |cx| cx.quit())
    });
    let rows = show(app, 10, 2);
    eprintln!("{rows:#?}");
    assert_eq!(rows[0].find("ab"), Some(8), "{rows:?}");
}

/// Many zero-width children with space-around in a wide row:
/// `free * (2 * i + 1)` overflows u32.
#[test]
fn space_around_many_zero_width_children_does_not_overflow() {
    let app = App::new(|| {
        row((0..40_000).map(|_| label("").size(Size::Fixed(0))))
            .justify(Justify::SpaceAround)
            .on_key("q", |cx| cx.quit())
    });
    let _ = show(app, u16::MAX, 1);
}
