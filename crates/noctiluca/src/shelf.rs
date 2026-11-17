//! Drawing a [`Shelf`] (a box that scrolls sideways over alike items:
//! "Customers also viewed", a row of videos) and paging it.
//!
//! The projection draws the shelf's items where Chrome lays them out, in
//! its box, so an item scrolled out of it is not drawn
//! ([`Shelf::hides`]). With the [shelves](crate::adapt::Rule::Shelves)
//! rule, a strip under the box says where it is and pages it: intuiTUIve's
//! `carousel`, drawn offscreen over the items in view, its `‹` and `›` at
//! the box's ends (dimmed at an end) and its count in the middle,
//! `‹      1–3 of 6      ›`. The shelf keys (`<` and `>`) scroll the
//! page's box sideways by what it shows ([`page_by`]), so Chrome lays out
//! the next items and a lazy shelf loads them, and the page is read
//! again.

use rich_intuituive::carousel;
use rich_intuituive::prelude::{label, signal};

use crate::layout::{Line, Span};
use crate::model::Shelf;

/// The strip under `shelf`, `width` cells wide: intuiTUIve's `carousel`'s
/// arrows and count over the items in view, on one row.
/// Whether `control` is one of a shelf's own arrow buttons, which its
/// strip stands in for: a button a few cells wide, beside the shelf (in its
/// band, or a couple of rows above or under it) and at its left or right
/// end, not in one of its items.
pub fn pages_a_shelf(shelves: &[Shelf], control: &crate::model::Control, cell: (f64, f64)) -> bool {
    let (cw, ch) = cell;
    let r = control.rect;
    control.role == crate::model::Role::Button
        && r.width > 0.0
        && r.width <= 4.0 * cw
        && r.height <= 2.0 * ch
        && shelves.iter().any(|shelf| {
            let s = shelf.rect;
            let band = r.bottom() >= s.y - 2.0 * ch && r.y <= s.bottom() + 2.0 * ch;
            let end = (r.x - s.x).abs() <= 3.0 * cw || (r.right() - s.right()).abs() <= 3.0 * cw;
            let in_item = shelf.items.iter().any(|i| {
                r.x >= i.x - 0.5
                    && r.right() <= i.right() + 0.5
                    && r.y >= i.y - 0.5
                    && r.bottom() <= i.bottom() + 0.5
            });
            band && end && !in_item
        })
}

pub fn strip(shelf: &Shelf, width: usize) -> Line {
    let width = width.max(8);
    let (first, shown) = shelf.shown();
    let len = shelf.items.len();
    let key = format!("shelf|{first}|{shown}|{len}");
    // The carousel draws its arrows on its items' middle row and its
    // count under them: two rows, folded into one.
    let rows = crate::indicator::offscreen(key, width, 2, move || {
        let first = signal(first);
        carousel(move || len, first, |_| label("")).per_page(shown.max(1))
    });
    fold(&rows)
}

/// Two rows as one: each cell the second row's where it draws something,
/// else the first's.
fn fold(rows: &[Line]) -> Line {
    let cells = |line: &Line| -> Vec<(char, String)> {
        line.spans
            .iter()
            .flat_map(|s| s.text.chars().map(move |c| (c, s.style.clone())))
            .collect()
    };
    let (top, bottom) = match rows {
        [top, bottom, ..] => (cells(top), cells(bottom)),
        [only] => (cells(only), Vec::new()),
        [] => return Line::default(),
    };
    let mut line = Line::default();
    for (k, cell) in top.iter().enumerate() {
        let (c, style) = match bottom.get(k) {
            Some(under) if under.0 != ' ' => under,
            _ => cell,
        };
        match line.spans.last_mut() {
            Some(last) if last.style == *style => last.text.push(*c),
            _ => line.spans.push(Span::new(c.to_string(), style.clone())),
        }
    }
    line
}

/// How far a page of `shelf` scrolls it, in CSS pixels, `forward` or
/// back: what it shows, less a little so the item cut at its edge shows
/// whole on the next page.
pub fn page_by(shelf: &Shelf, forward: bool) -> f64 {
    let (_, shown) = shelf.shown();
    // An item's stride: from one item's left to the next's.
    let stride = shelf
        .items
        .windows(2)
        .map(|w| w[1].x - w[0].x)
        .find(|d| *d > 0.0);
    let by = match stride {
        Some(stride) if shown > 0 => stride * shown as f64,
        _ => shelf.client_width.max(shelf.rect.width),
    };
    if forward { by } else { -by }
}
