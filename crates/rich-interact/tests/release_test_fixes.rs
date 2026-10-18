//! The 0.0.19 release test's second round of audits (workstreams 7 to 10):
//! a regression test for each defect found in Reorder, the date and time
//! pickers, `datetime`'s parsing, the viewport's sticky lines and the
//! pager's table-header detection.

use rich_interact::datetime::{Date, DateTime, Time};
use rich_interact::headless::{self, Script};
use rich_interact::policy::{Fallback, Reason, ScriptedLineIo};
use rich_interact::{
    degrade, DatePicker, DateTimePicker, Outcome, Pager, Reorder, TimePicker, Viewport,
};

fn before_answer(record: &headless::Record) -> &str {
    &record.frames[record.frames.len() - 2]
}

/// Ask `picker` without a terminal, with `lines` typed.
fn typed<C: rich_interact::Component>(
    picker: &mut C,
    lines: &[&str],
) -> Result<Outcome<C::Output>, rich_interact::Error> {
    let mut io = ScriptedLineIo::new(lines.iter().map(|line| line.to_string()));
    degrade(picker, Fallback::Prompt, Reason::StdinNotTerminal, &mut io)
}

// ---- Reorder ----

/// The wheel while an item is grabbed carries it, as the arrows do, so
/// Esc puts that item back rather than moving the one under the focus.
#[test]
fn wheel_while_grabbed_then_escape_restores_the_order() {
    let reorder = Reorder::new("Order", ["a", "b", "c", "d"]).with_mouse(true);
    let script = Script::new()
        .keys("down down space")
        .scroll(false, 3, 2)
        .keys("escape enter");
    let (outcome, _) = headless::run(reorder, script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["a", "b", "c", "d"]));
    // The wheel carries it while grabbed.
    let reorder = Reorder::new("Order", ["a", "b", "c", "d"]).with_mouse(true);
    let script = Script::new()
        .keys("space")
        .scroll(true, 3, 2)
        .scroll(true, 3, 2)
        .keys("space enter");
    let (outcome, _) = headless::run(reorder, script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["b", "c", "a", "d"]));
}

/// A click lands on the row drawn under it after `focus(..)` scrolled the
/// list on its first paint.
#[test]
fn a_click_after_focus_builder_focuses_the_row_clicked() {
    let items: Vec<String> = (0..30).map(|n| format!("i{n}")).collect();
    let reorder = Reorder::new("Long", items)
        .height(5)
        .focus(20)
        .with_mouse(true);
    let script = Script::new().click(3, 1).keys("alt+down enter");
    let (outcome, _) = headless::run(reorder, script, 40, 12);
    let order = outcome.unwrap().value().unwrap();
    assert_eq!(&order[16..18], ["i17", "i16"]);
}

/// A click lands on the row drawn under it after the terminal grew and
/// the list scrolled back to its top.
#[test]
fn a_click_after_the_terminal_grew_focuses_the_row_clicked() {
    let items: Vec<String> = (0..10).map(|n| format!("i{n}")).collect();
    let reorder = Reorder::new("Long", items).with_mouse(true);
    let script = Script::new()
        .keys("end")
        .resize(40, 20)
        .click(3, 1)
        .keys("alt+down enter");
    let (outcome, _) = headless::run(reorder, script, 40, 10);
    assert_eq!(&outcome.unwrap().value().unwrap()[..3], ["i1", "i0", "i2"]);
}

/// Numbers typed for a long list are read in linear time.
#[test]
fn reordering_a_long_list_by_numbers_is_not_quadratic() {
    let n = 40_000;
    let items: Vec<String> = (0..n).map(|i| format!("item {i}")).collect();
    let line = (1..=n)
        .rev()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let mut io = ScriptedLineIo::new([line]);
    let started = std::time::Instant::now();
    let outcome = degrade(
        &mut Reorder::new("Order", items),
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(
        outcome.unwrap().value().unwrap()[0],
        format!("item {}", n - 1)
    );
    assert!(
        started.elapsed().as_secs_f64() < 1.0,
        "{:?}",
        started.elapsed()
    );
}

// ---- The date and time pickers ----

const DAY: Date = Date::new(2026, 10, 17);

/// Tab moves through a time picker's fields and then out of it, so a
/// picker in a container does not keep the focus; Shift+Tab comes back in
/// at its last field, and past the last child focus wraps round to its
/// first.
#[test]
fn tab_leaves_a_time_picker_in_a_column() {
    use rich_interact::compose::{Column, ComponentExt};
    use rich_interact::{Flow, Input};

    let app = || {
        let picker = TimePicker::new("At")
            .value(Time::new(9, 0, 0))
            .map(|time: Time| Flow::Done(time.to_string()));
        let name = Input::new("Name").map(|s: String| Flow::Done(s));
        Column::new().child(picker).child(name)
    };
    // The hour, the minute, then the input.
    let script = Script::new().keys("tab tab").text("abc").keys("enter");
    let (outcome, _) = headless::run(app(), script, 40, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("abc".to_string()));
    let script = Script::new().keys("tab tab shift+tab up enter");
    let (outcome, _) = headless::run(app(), script, 40, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("09:01".to_string()));
    let script = Script::new().keys("tab tab tab up enter");
    let (outcome, _) = headless::run(app(), script, 40, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("10:00".to_string()));

    // A date-and-time picker: the grid, the hour, the minute, then out.
    let app = || {
        let picker = DateTimePicker::new("At")
            .value(DAY.at(Time::new(9, 0, 0)))
            .map(|at: DateTime| Flow::Done(at.to_string()));
        let name = Input::new("Name").map(|s: String| Flow::Done(s));
        Column::new().child(picker).child(name)
    };
    let script = Script::new().keys("tab tab tab").text("abc").keys("enter");
    let (outcome, _) = headless::run(app(), script, 40, 20);
    assert_eq!(outcome.unwrap(), Outcome::Done("abc".to_string()));
    // Back from the grid: round to the input, then the minute, the hour
    // and the grid.
    let script = Script::new().keys("shift+tab shift+tab up");
    let script = script.keys("shift+tab shift+tab right enter");
    let (outcome, _) = headless::run(app(), script, 40, 20);
    assert_eq!(
        outcome.unwrap(),
        Outcome::Done("2026-10-18 09:01".to_string())
    );
}

/// On its own a picker's Tab still goes round its parts.
#[test]
fn tab_still_cycles_a_picker_on_its_own() {
    let picker = TimePicker::new("At").value(Time::new(9, 0, 0));
    let script = Script::new().keys("tab tab up enter");
    let (outcome, _) = headless::run(picker, script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(10, 0, 0)));
    let picker = TimePicker::new("At").value(Time::new(9, 0, 0));
    let script = Script::new().keys("shift+tab up enter");
    let (outcome, _) = headless::run(picker, script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(9, 1, 0)));
}

/// Without seconds a time picker answers the minute it shows.
#[test]
fn a_time_picker_without_seconds_answers_what_it_shows() {
    let picker = TimePicker::new("At")
        .seconds(false)
        .value(Time::new(14, 30, 45));
    let (outcome, record) = headless::run(picker, Script::new().keys("enter"), 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(14, 30, 0)));
    assert_eq!(record.last_frame(), "? At › 14:30");
}

/// Without seconds the limit a message names is one the picker takes.
#[test]
fn a_time_picker_refuses_what_it_shows_as_the_limit() {
    let picker = TimePicker::new("At")
        .seconds(false)
        .value(Time::new(14, 30, 45))
        .max(Time::new(14, 30, 0));
    let (outcome, record) = headless::run(picker, Script::new().keys("enter escape"), 40, 6);
    assert!(!before_answer(&record).contains("latest is 14:30"));
    assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(14, 30, 0)));
}

/// Without seconds everything is to the minute, whatever order the
/// builders come in: the earliest rounds up, the rest round down.
#[test]
fn without_seconds_the_range_is_to_the_minute() {
    // 09:00 shows, and is before 09:00:30: the earliest is 09:01.
    let picker = TimePicker::new("At")
        .min(Time::new(9, 0, 30))
        .value(Time::new(9, 0, 0));
    let script = Script::new().keys("enter right up enter");
    let (outcome, record) = headless::run(picker, script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(9, 1, 0)));
    assert!(record
        .frames
        .iter()
        .any(|frame| frame.contains("✗ the earliest is 09:01")));
    // Seconds turned on after the limits keep them.
    let picker = TimePicker::new("At")
        .value(Time::new(14, 30, 45))
        .max(Time::new(14, 30, 50))
        .seconds(true);
    let (outcome, _) = headless::run(picker, Script::new().keys("enter"), 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(14, 30, 45)));
    // A typed answer and a default are to the minute too.
    let mut picker = TimePicker::new("At").max(Time::new(14, 30, 0));
    let outcome = typed(&mut picker, &["14:30:45"]);
    assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(14, 30, 0)));
    let mut picker = TimePicker::new("At").default(Time::new(9, 0, 30));
    let outcome = typed(&mut picker, &[""]);
    assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(9, 0, 0)));
    let mut picker = TimePicker::new("At")
        .min(Time::new(9, 0, 30))
        .default(Time::new(9, 0, 30));
    let error = typed(&mut picker, &[""]).unwrap_err().to_string();
    assert!(error.contains("the earliest is 09:01"), "{error}");
    // A date and time: the earliest at 23:59:30 rounds up to the next day,
    // so the grid starts there.
    let earliest = DAY.at(Time::new(23, 59, 30));
    let mut picker = DateTimePicker::new("At").min(earliest);
    let error = typed(&mut picker, &["2026-10-17 23:59"])
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("the earliest is 2026-10-18 00:00"),
        "{error}"
    );
    let picker = DateTimePicker::new("At")
        .value(DAY.at(Time::new(23, 59, 0)))
        .min(earliest);
    let (outcome, _) = headless::run(picker, Script::new().keys("enter"), 40, 16);
    let next = Date::new(2026, 10, 18);
    assert_eq!(
        outcome.unwrap(),
        Outcome::Done(next.at(Time::new(23, 59, 0)))
    );
    // No minute of a time picker's day is at or after 23:59:30.
    let mut picker = TimePicker::new("At").min(Time::new(23, 59, 30));
    let error = typed(&mut picker, &["23:59"]).unwrap_err().to_string();
    assert!(error.contains("the earliest is 23:59:30"), "{error}");
}

/// An empty line does not answer a default outside the range, as the
/// grid would not have started on it.
#[test]
fn an_empty_line_does_not_answer_a_default_outside_the_range() {
    let picker = || {
        DatePicker::new("Due")
            .min(Date::new(2026, 10, 20))
            .default(Date::new(2026, 10, 1))
    };
    let mut io = ScriptedLineIo::new([""]);
    let outcome = degrade(
        &mut picker(),
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    let (interactive, _) = headless::run(picker(), Script::new().keys("enter"), 40, 14);
    assert!(outcome.is_err() || outcome.as_ref().ok() == interactive.as_ref().ok());
    let error = outcome.unwrap_err().to_string();
    assert!(error.contains("the earliest is 2026-10-20"), "{error}");
    // Asked for defaults, there is none to give.
    let mut io = ScriptedLineIo::new(Vec::<String>::new());
    let outcome = degrade(
        &mut picker(),
        Fallback::Default,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert!(outcome.is_err());
}

/// A default the validator refuses is refused as a typed one would be.
#[test]
fn a_default_the_validator_refuses_is_not_answered() {
    let picker = || {
        DatePicker::new("Due")
            .default(DAY)
            .validate(|_| Err("never".into()))
    };
    let error = typed(&mut picker(), &[""]).unwrap_err().to_string();
    assert!(error.contains("never"), "{error}");
    let mut io = ScriptedLineIo::new(Vec::<String>::new());
    let outcome = degrade(
        &mut picker(),
        Fallback::Default,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert!(outcome.is_err());
}

/// A range with its earliest after its latest says so.
#[test]
fn an_inverted_range_is_not_accepted_silently() {
    for bound in ["2026-10-20", "2026-10-10"] {
        let mut picker = DatePicker::new("Due")
            .min(Date::new(2026, 10, 20))
            .max(Date::new(2026, 10, 10));
        let mut io = ScriptedLineIo::new([bound]);
        let outcome = degrade(
            &mut picker,
            Fallback::Prompt,
            Reason::StdinNotTerminal,
            &mut io,
        );
        let message = format!("{outcome:?}");
        assert!(
            outcome.is_ok() || message.contains("after") || message.contains("empty"),
            "{message}"
        );
    }
    let picker = TimePicker::new("At")
        .min(Time::new(17, 0, 0))
        .max(Time::new(9, 0, 0));
    let (_, record) = headless::run(picker, Script::new().keys("enter escape"), 40, 8);
    assert!(
        before_answer(&record).contains("the range is empty"),
        "{}",
        before_answer(&record)
    );
}

/// An explicit start wins over the default's, whatever the order.
#[test]
fn an_explicit_value_wins_over_the_default() {
    let first = Date::new(2026, 10, 1);
    for picker in [
        DatePicker::new("Due").value(first).default(DAY),
        DatePicker::new("Due").default(DAY).value(first),
    ] {
        let (outcome, _) = headless::run(picker, Script::new().keys("enter"), 40, 14);
        assert_eq!(outcome.unwrap(), Outcome::Done(first));
    }
    let (outcome, _) = headless::run(
        DatePicker::new("Due").default(DAY),
        Script::new().keys("enter"),
        40,
        14,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(DAY));
    let picker = TimePicker::new("At")
        .value(Time::new(8, 0, 0))
        .default(Time::new(9, 0, 0));
    let (outcome, _) = headless::run(picker, Script::new().keys("enter"), 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(8, 0, 0)));
    let picker = DateTimePicker::new("At")
        .value(first.at(Time::new(8, 0, 0)))
        .default(DAY.at(Time::new(9, 0, 0)));
    let (outcome, _) = headless::run(picker, Script::new().keys("enter"), 40, 16);
    assert_eq!(
        outcome.unwrap(),
        Outcome::Done(first.at(Time::new(8, 0, 0)))
    );
}

/// The pickers keep to the years ISO 8601's four digits write, 1 to
/// 9999, so every answer prints and parses back.
#[test]
fn the_pickers_stay_within_years_1_to_9999() {
    let last = Date::new(9999, 12, 31);
    let first = Date::new(1, 1, 1);
    for (start, keys, expected) in [
        (last, "right enter", last),
        (last, "} enter", last),
        (first, "left enter", first),
        (first, "pageup enter", first),
    ] {
        let picker = DatePicker::new("Due").value(start);
        let (outcome, _) = headless::run(picker, Script::new().keys(keys), 40, 14);
        assert_eq!(outcome.unwrap(), Outcome::Done(expected), "{keys}");
    }
    let picker = DateTimePicker::new("At").value(last.at(Time::new(9, 0, 0)));
    let (outcome, _) = headless::run(picker, Script::new().keys("down enter"), 40, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done(last.at(Time::new(9, 0, 0))));
    assert!(typed(&mut DatePicker::new("Due"), &["0000-01-01"]).is_err());
}

// ---- datetime ----

/// A `T` before a 12-hour time splits the date from it.
#[test]
fn a_date_time_with_a_t_and_a_twelve_hour_clock_parses() {
    let expected = DateTime::new(DAY, Time::new(14, 30, 0));
    assert_eq!("2026-10-17T2:30 pm".parse::<DateTime>(), Ok(expected));
    assert_eq!("2026-10-17t2:30pm".parse::<DateTime>(), Ok(expected));
    assert_eq!("2026-10-17 2:30 pm".parse::<DateTime>(), Ok(expected));
    assert!("today 2:30 pm".parse::<DateTime>().is_ok());
}

/// A year is four digits, 0001 to 9999.
#[test]
fn a_date_needs_four_year_digits() {
    for bad in ["26-10-17", "026-10-17", "0000-01-01", "10000-01-01"] {
        assert!(bad.parse::<Date>().is_err(), "{bad:?}");
    }
    assert_eq!("0001-01-01".parse(), Ok(Date::new(1, 1, 1)));
    assert_eq!("9999-12-31".parse(), Ok(Date::new(9999, 12, 31)));
}

// ---- The viewport's sticky lines ----

fn numbered(count: usize) -> Vec<Vec<rich::Segment>> {
    (0..count)
        .map(|i| vec![rich::Segment::new(format!("line {i}"), None)])
        .collect()
}

/// Sticky lines a page cannot hold all of: the first scroll away and the
/// last stay, so every line is shown at some offset.
#[test]
fn a_page_smaller_than_the_sticky_lines_still_shows_every_line() {
    let mut viewport = Viewport::new(numbered(20)).sticky(4);
    let mut seen = std::collections::BTreeSet::new();
    for offset in 0..20 {
        viewport.scroll_to(offset, 3);
        for (i, _) in viewport.rows(3) {
            seen.insert(i);
        }
    }
    let never: Vec<usize> = (0..20).filter(|i| !seen.contains(i)).collect();
    assert!(never.is_empty(), "never shown: {never:?}");
}

/// The rows, the status line and `show` agree on what a small page shows.
#[test]
fn sticky_lines_taller_than_the_page_scroll_until_the_last_ones_stick() {
    let mut viewport = Viewport::new(numbered(20)).sticky(4);
    let index = |viewport: &Viewport| -> Vec<usize> {
        viewport.rows(3).iter().map(|(at, _)| *at).collect()
    };
    // At the top, the first lines in order.
    assert_eq!(index(&viewport), [0, 1, 2]);
    assert_eq!(viewport.status(3), "lines 1–3 of 20");
    assert!(viewport.scroll(1, 3));
    assert_eq!(index(&viewport), [1, 2, 3]);
    assert_eq!(viewport.status(3), "lines 2–4 of 20");
    // Then the last two sticky lines stay, and the rest scroll under them.
    assert!(viewport.scroll(1, 3));
    assert_eq!(index(&viewport), [2, 3, 4]);
    assert_eq!(viewport.status(3), "lines 5–5 of 20");
    assert_eq!(viewport.visible(3).len(), 1);
    viewport.scroll_to(usize::MAX, 3);
    assert_eq!(viewport.offset(), 17);
    assert_eq!(index(&viewport), [2, 3, 19]);
    assert_eq!(viewport.status(3), "lines 20–20 of 20");
    // A line that stays needs no scroll; one that scrolled away comes back.
    assert!(!viewport.show(3, 3));
    assert!(viewport.show(1, 3));
    assert!(index(&viewport).contains(&1), "{:?}", index(&viewport));
    assert!(viewport.show(10, 3));
    assert_eq!(index(&viewport), [2, 3, 10]);
    // With room for them all, nothing changes: they all stay.
    let mut viewport = Viewport::new(numbered(20)).sticky(4);
    viewport.scroll(3, 6);
    let shown: Vec<usize> = viewport.rows(6).iter().map(|(at, _)| *at).collect();
    assert_eq!(shown, [0, 1, 2, 3, 7, 8]);
    assert_eq!(viewport.status(6), "lines 8–9 of 20");
}

/// A pager on a page too short for a table's whole top keeps its column
/// names, and a search finds a line among the sticky ones.
#[test]
fn pager_table_header_on_a_tiny_page() {
    let mut table = rich::Table::new().title("Files");
    table.add_column("Name");
    table.add_column("Size");
    for i in 1..=30 {
        table.add_row(&[&format!("file {i}"), &format!("{i}K")]);
    }
    let (outcome, record) = headless::run(
        Pager::new(table).sticky_table_header(true),
        Script::new().keys("pagedown q"),
        30,
        4,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(()));
    assert!(
        record.last_frame().contains("Name"),
        "{}",
        record.last_frame()
    );
    // Six sticky lines on a page of five: a search for the sixth shows it.
    let lines: Vec<Vec<rich::Segment>> = (1..=40)
        .map(|i| vec![rich::Segment::new(format!("line {i}"), None)])
        .collect();
    let pager = Pager::lines(lines).sticky_header(6);
    let script = Script::new().keys("/").text("line 6").keys("enter q");
    let (_, record) = headless::run(pager, script, 30, 6);
    let frame = record.last_frame();
    assert!(frame.lines().any(|line| line == "line 6"), "{frame}");
    assert!(frame.contains("match 1/1"), "{frame}");
}

// ---- The pager's table-header detection ----

fn rendered(r: &dyn rich::Renderable, width: usize) -> Vec<Vec<rich::Segment>> {
    let console = rich::Console::builder()
        .force_terminal(false)
        .width(width)
        .build();
    let options = console.options();
    console.render_lines(r, &options, false)
}

/// A table without a header, its rows divided by lines that look like
/// the line under a header, has no header to pin.
#[test]
fn a_headerless_table_with_row_lines_has_no_header() {
    let mut table = rich::Table::new().show_header(false).show_lines(true);
    table.add_column("k");
    table.add_column("v");
    for i in 1..=6 {
        table.add_row(&[&format!("key {i}"), &format!("value {i}")]);
    }
    assert_eq!(
        rich_interact::components::table_header_rows(&rendered(&table, 30)),
        None
    );
    // A header whose line differs from the lines between rows is found.
    let mut table = rich::Table::new().show_lines(true);
    table.add_column("k");
    table.add_column("v");
    for i in 1..=6 {
        table.add_row(&[&format!("key {i}"), &format!("value {i}")]);
    }
    assert_eq!(
        rich_interact::components::table_header_rows(&rendered(&table, 30)),
        Some(3)
    );
}
