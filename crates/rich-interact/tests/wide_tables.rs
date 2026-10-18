//! Wide and long tables (#716, #717): a `TableSelect` wider than its list
//! scrolls across with its headings over their columns, and a table paged
//! by the `Pager` or a `Viewport` keeps its header on top.

use rich::r#box;
use rich::{Segment, Table};
use rich_interact::components::table_header_rows;
use rich_interact::event::{Event, Modifiers, Mouse, MouseKind};
use rich_interact::headless::{self, Script};
use rich_interact::{Outcome, Pager, TableSelect, Viewport};

/// Five columns, 60 cells of label: `id` 0, `name` 4, `city` 18,
/// `country` 28 and `notes` 44.
fn people() -> TableSelect<usize> {
    let row = |cells: [&str; 5]| cells.iter().map(|c| c.to_string()).collect::<Vec<_>>();
    TableSelect::new(
        "Who",
        ["id", "name", "city", "country", "notes"],
        [
            (
                0,
                row([
                    "1",
                    "Ada Lovelace",
                    "London",
                    "United Kingdom",
                    "first programmer",
                ]),
            ),
            (
                1,
                row(["2", "Grace Hopper", "New York", "United States", "COBOL"]),
            ),
        ],
    )
}

/// The frame's line holding `needle`.
fn line_with<'a>(frame: &'a str, needle: &str) -> &'a str {
    frame
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no line with {needle:?} in:\n{frame}"))
}

/// The cell `needle` starts at in the frame's line holding it (the frames
/// here are ASCII but for the markers, each one cell).
fn column_of(frame: &str, needle: &str) -> usize {
    let line = line_with(frame, needle);
    line[..line.find(needle).unwrap()].chars().count()
}

/// The last frame after `keys`, 30 columns wide.
fn after(table: TableSelect<usize>, keys: &str) -> String {
    let script = Script::new().keys(keys).keys("escape");
    let (outcome, record) = headless::run(table, script, 30, 8);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    // The last frame is the cancelled answer; the one before is the table.
    record.frames[record.frames.len() - 2].clone()
}

#[test]
fn a_wide_table_without_scrolling_is_cut_as_before() {
    let frame = after(people(), "right");
    let heading = line_with(&frame, "name");
    assert_eq!(heading.trim_end(), "  id  name          city");
    assert!(!heading.contains('›') && !heading.contains('‹'), "{frame}");
}

#[test]
fn right_and_left_scroll_a_column_at_a_time_with_the_header_aligned() {
    let table = || people().scroll_x(true);
    // At first the rest is out of view to the right.
    let frame = after(table(), "");
    let heading = line_with(&frame, "name");
    assert!(heading.starts_with("  id  name"), "{frame}");
    assert!(heading.ends_with('›'), "{frame}");
    assert_eq!(heading.chars().count(), 30);

    // One column across: `name` leads, and `‹` says `id` is out of view.
    let frame = after(table(), "right");
    let heading = line_with(&frame, "name");
    assert!(heading.starts_with("‹ name          city"), "{frame}");
    assert!(heading.ends_with('›'), "{frame}");
    assert!(line_with(&frame, "Ada").starts_with("❯ Ada Lovelace  London"));
    assert_eq!(column_of(&frame, "city"), column_of(&frame, "London"));
    assert_eq!(column_of(&frame, "city"), column_of(&frame, "New York"));

    // Two across, then back one.
    let frame = after(table(), "right right left");
    assert_eq!(line_with(&frame, "Ada"), "❯ Ada Lovelace  London    Unit");
}

#[test]
fn the_ends_are_a_key_away_and_the_view_stops_at_the_last_column() {
    let table = || people().scroll_x(true);
    // Shift+End: the last column in view, nothing out of view to the right.
    let frame = after(table(), "shift+end");
    let heading = line_with(&frame, "notes");
    assert!(heading.starts_with("‹ notes"), "{frame}");
    assert!(!heading.contains('›'), "{frame}");
    assert!(line_with(&frame, "first").starts_with("❯ first programmer"));
    // Right past the end stays there.
    let frame = after(table(), "shift+end right right");
    assert!(line_with(&frame, "notes").starts_with("‹ notes"), "{frame}");
    // Shift+Home goes back.
    let frame = after(table(), "shift+end shift+home");
    assert!(
        line_with(&frame, "name").starts_with("  id  name"),
        "{frame}"
    );
}

#[test]
fn frozen_columns_stay_while_the_rest_scroll() {
    let table = || people().scroll_x(true).freeze_columns(1);
    let frame = after(table(), "right");
    // `id` stays; `name` scrolled out, `city` follows it.
    let heading = line_with(&frame, "city");
    assert!(heading.starts_with("‹ id  city      country"), "{frame}");
    assert_eq!(
        line_with(&frame, "London"),
        "❯ 1   London    United Kingdom"
    );
    assert_eq!(
        column_of(&frame, "country"),
        column_of(&frame, "United Kingdom")
    );
    // Left does not scroll the frozen column away.
    let frame = after(table(), "left left");
    assert!(
        line_with(&frame, "name").starts_with("  id  name"),
        "{frame}"
    );
    let frame = after(table(), "shift+end");
    assert!(
        line_with(&frame, "notes").starts_with("‹ id  notes"),
        "{frame}"
    );
    assert_eq!(
        column_of(&frame, "notes"),
        column_of(&frame, "first programmer")
    );
}

#[test]
fn the_view_follows_the_focused_column() {
    let table = || people().scroll_x(true);
    // Ctrl+Right to `country` (28..42): `city` leads so it fits.
    let frame = after(table(), "ctrl+right ctrl+right ctrl+right");
    let heading = line_with(&frame, "country");
    assert!(heading.starts_with("‹ city      country"), "{frame}");
    // And back to `id`.
    let frame = after(
        table(),
        "ctrl+right ctrl+right ctrl+right ctrl+left ctrl+left ctrl+left",
    );
    assert!(
        line_with(&frame, "name").starts_with("  id  name"),
        "{frame}"
    );
}

#[test]
fn shift_and_the_wheel_scroll_across() {
    let wheel = |kind| {
        let mut mouse = Mouse::new(kind, 4, 3);
        mouse.modifiers = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        Event::Mouse(mouse)
    };
    let table = people().scroll_x(true).with_mouse(true);
    let script = Script::new()
        .event(wheel(MouseKind::ScrollDown))
        .event(wheel(MouseKind::ScrollDown))
        .event(wheel(MouseKind::ScrollUp))
        .keys("escape");
    let (_, record) = headless::run(table, script, 30, 8);
    let frame = &record.frames[record.frames.len() - 2];
    assert!(line_with(frame, "name").starts_with("‹ name"), "{frame}");
    // The wheel alone still moves the row.
    let table = people().scroll_x(true).with_mouse(true);
    let script = Script::new().scroll(true, 4, 3).keys("enter");
    let (outcome, _) = headless::run(table, script, 30, 8);
    assert_eq!(outcome.unwrap(), Outcome::Done(1));
}

#[test]
fn a_table_that_fits_does_not_scroll() {
    let table = people().scroll_x(true);
    let script = Script::new().keys("right escape");
    let (_, record) = headless::run(table, script, 80, 8);
    let frame = &record.frames[0];
    let heading = line_with(frame, "notes");
    assert_eq!(
        heading.trim_end(),
        "  id  name          city      country         notes"
    );
    assert_eq!(record.frames.len(), 2, "right changed nothing");
    // The footer says how to scroll.
    assert!(frame.contains("←→ scroll"), "{frame}");
}

// ---------------------------------------------------------------------------
// Sticky headers (#716)

/// A table of `rows` files under a `Name`/`Size` header.
fn files(rows: usize) -> Table {
    let mut table = Table::new();
    table.add_column("Name");
    table.add_column("Size");
    for row in 1..=rows {
        table.add_row(&[&format!("file {row}"), &format!("{row}K")]);
    }
    table
}

/// The lines `renderable` renders as at `width`, without colour.
fn rendered(renderable: &dyn rich::Renderable, width: usize) -> Vec<Vec<Segment>> {
    let console = rich::Console::builder()
        .force_terminal(false)
        .width(width)
        .build();
    let options = console.options();
    console.render_lines(renderable, &options, false)
}

#[test]
fn a_tables_header_is_found_in_its_lines() {
    // rich's default: top edge, header, the line under it.
    assert_eq!(table_header_rows(&rendered(&files(5), 30)), Some(3));
    // A title above it, and a header two lines tall.
    let mut table = files(5).title("Files");
    table.add_column("Last\nchanged");
    assert_eq!(table_header_rows(&rendered(&table, 40)), Some(5));
    // Other boxes, and no edge.
    for set in [
        r#box::ROUNDED,
        r#box::ASCII,
        r#box::DOUBLE_EDGE,
        r#box::MINIMAL,
        r#box::MARKDOWN,
    ] {
        let table = files(5).box_set(set);
        assert_eq!(table_header_rows(&rendered(&table, 30)), Some(3));
    }
    let table = files(5).show_edge(false);
    assert_eq!(table_header_rows(&rendered(&table, 30)), Some(2));
}

#[test]
fn content_that_does_not_start_with_a_table_has_no_header() {
    // No header, a panel, a rule, text.
    assert_eq!(
        table_header_rows(&rendered(&files(5).show_header(false), 30)),
        None
    );
    let panel = rich::Panel::new(Box::new(rich::Text::new("hello\nworld")));
    assert_eq!(table_header_rows(&rendered(&panel, 30)), None);
    let text: Vec<Vec<Segment>> = ["a", "──────", "b"]
        .iter()
        .map(|line| vec![Segment::new(*line, None)])
        .collect();
    assert_eq!(table_header_rows(&text), None);
    // A table far down the content is not the content's header.
    let mut lines: Vec<Vec<Segment>> = (0..8)
        .map(|i| vec![Segment::new(format!("intro {i}"), None)])
        .collect();
    lines.extend(rendered(&files(5), 30));
    assert_eq!(table_header_rows(&lines), None);
    // Nothing under the header: nothing to scroll.
    assert_eq!(table_header_rows(&rendered(&files(5), 30)[..3]), None);
}

/// The last frame's lines after `keys`, `height` rows tall.
fn paged(pager: Pager, keys: &str, height: u16) -> Vec<String> {
    let (outcome, record) = headless::run(pager, Script::new().keys(keys), 30, height);
    assert_eq!(outcome.unwrap(), Outcome::Done(()));
    record
        .last_frame()
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect()
}

#[test]
fn a_paged_table_keeps_its_header_on_top() {
    let pager = || Pager::new(files(60)).sticky_table_header(true);
    // 12 rows: 11 of content and the status line.
    let lines = paged(pager(), "pagedown pagedown q", 12);
    assert!(lines[0].starts_with("┏━"), "{lines:#?}");
    assert!(lines[1].contains("Name") && lines[1].contains("Size"));
    assert!(lines[2].starts_with("┡━"), "{lines:#?}");
    // Two pages of eight rows under it.
    assert!(lines[3].contains("file 17"), "{lines:#?}");
    assert!(lines[10].contains("file 24"), "{lines:#?}");
    assert!(lines[11].starts_with("lines 20–27 of 64"), "{lines:#?}");
    // The header's columns are the rows' columns.
    let bars = |line: &str| -> Vec<usize> {
        line.chars()
            .enumerate()
            .filter(|(_, c)| matches!(c, '┃' | '│'))
            .map(|(at, _)| at)
            .collect()
    };
    assert_eq!(bars(&lines[1]), bars(&lines[3]));

    // At the end, the last row is above the bottom edge, under the header.
    let lines = paged(pager(), "end q", 12);
    assert!(lines[1].contains("Name"), "{lines:#?}");
    assert!(lines[9].contains("file 60"), "{lines:#?}");
    assert!(lines[10].starts_with('└'), "{lines:#?}");

    // Without it, the header scrolls away as before.
    let lines = paged(Pager::new(files(60)), "pagedown pagedown q", 12);
    assert!(
        !lines.iter().any(|line| line.contains("Name")),
        "{lines:#?}"
    );
    assert!(lines[0].contains("file 20"), "{lines:#?}");
}

#[test]
fn a_search_scrolls_under_the_header() {
    let pager = Pager::new(files(60))
        .sticky_table_header(true)
        .search("file 40");
    let lines = paged(pager, "q", 12);
    assert!(lines[1].contains("Name"), "{lines:#?}");
    assert!(
        lines.iter().any(|line| line.contains("file 40")),
        "{lines:#?}"
    );
}

#[test]
fn sticky_lines_can_be_given() {
    let lines: Vec<Vec<Segment>> = (1..=50)
        .map(|i| vec![Segment::new(format!("line {i}"), None)])
        .collect();
    let lines = paged(Pager::lines(lines).sticky_header(2), "j j j q", 6);
    assert_eq!(
        lines,
        [
            "line 1",
            "line 2",
            "line 6",
            "line 7",
            "line 8",
            "lines 6–8 of 50 · / search · q"
        ]
    );
}

#[test]
fn a_viewport_keeps_its_sticky_lines() {
    let lines: Vec<Vec<Segment>> = (1..=50)
        .map(|i| vec![Segment::new(format!("line {i}"), None)])
        .collect();
    let index = |viewport: &Viewport| -> Vec<usize> {
        viewport.rows(5).iter().map(|(at, _)| *at).collect()
    };
    let mut viewport = Viewport::new(lines.clone()).sticky(1);
    // A page of five: one sticky, four scrolling.
    assert!(viewport.scroll(10, 5));
    assert_eq!(index(&viewport), [0, 11, 12, 13, 14]);
    assert_eq!(viewport.visible(5).len(), 4);
    assert_eq!(viewport.status(5), "lines 12–15 of 50");
    // The end: the last line last.
    viewport.scroll_to(usize::MAX, 5);
    assert_eq!(viewport.offset(), 45);
    assert_eq!(index(&viewport), [0, 46, 47, 48, 49]);
    // Showing a line scrolls just enough; a sticky line is always shown.
    assert!(viewport.show(20, 5));
    assert_eq!(index(&viewport)[1], 20);
    assert!(!viewport.show(0, 5));
    // At the top, the page is the first lines, as without sticky lines.
    viewport.scroll_to(0, 5);
    assert_eq!(index(&viewport), [0, 1, 2, 3, 4]);
    assert_eq!(viewport.status(5), "lines 1–5 of 50");

    // Run as a component.
    let (outcome, record) = headless::run(
        Viewport::new(lines).sticky(1),
        Script::new().keys("pagedown enter"),
        20,
        6,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(4));
    let frame = record.last_frame();
    assert!(frame.starts_with("line 1\nline 6\n"), "{frame}");
}
