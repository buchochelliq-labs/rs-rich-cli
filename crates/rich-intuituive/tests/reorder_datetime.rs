//! Reorderable lists (#720), and the calendar's range, the time picker and
//! the date-and-time picker (#719).

mod common;

use common::{run, screen};
use intuituive::a11y::Role;
use intuituive::interact::headless::Script;
use intuituive::interact::{Button, Event, Key, MouseKind};
use intuituive::prelude::*;
use intuituive::widgets::{
    calendar_options, datetime_picker, time_picker, time_picker_with, CalendarOptions, Date,
    DateTime, DateTimeOptions, Move, Time, TimeOptions,
};

// ---- Reorderable lists ----

/// Four items in a list over a line showing the order and the selection.
fn list_app(rows: u16) -> App {
    App::new(move || {
        let items = signal(vec!["a".to_string(), "b".into(), "c".into(), "d".into()]);
        let selected = signal(0usize);
        column([
            list(move || items.get(), selected)
                .reorderable(move |step: Move, _| items.update(|items| step.apply(items)))
                .fixed(rows),
            text(move || format!("{} @{}", items.get().join(""), selected.get())).fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
        .on_key("enter", |cx| cx.quit())
    })
}

fn order(script: Script) -> String {
    let rows = screen(&run(list_app(4), script, 12, 5));
    rows[4].clone()
}

#[test]
fn keys_move_the_selected_item() {
    assert_eq!(order(Script::new().keys("alt+down q")), "bacd @1");
    assert_eq!(order(Script::new().keys("down ctrl+down q")), "acbd @2");
    assert_eq!(order(Script::new().keys("end K K q")), "adbc @1");
    assert_eq!(order(Script::new().keys("J J J J q")), "bcda @3");
    assert_eq!(order(Script::new().keys("down alt+end q")), "acdb @3");
    assert_eq!(order(Script::new().keys("end ctrl+home q")), "dabc @0");
    // The ends: nothing moves past them.
    assert_eq!(order(Script::new().keys("alt+up q")), "abcd @0");
    assert_eq!(order(Script::new().keys("end alt+down q")), "abcd @3");
    // Plain arrows still only select.
    assert_eq!(order(Script::new().keys("down down up q")), "abcd @1");
}

#[test]
fn kitty_keys_move_too() {
    let script = Script::new()
        .event(Event::Key(Key::parse("alt+down").unwrap().exact()))
        .keys("q");
    assert_eq!(order(script), "bacd @1");
}

#[test]
fn space_grabs_and_the_arrows_carry() {
    // Grab a, carry it down twice, drop it, then select up.
    assert_eq!(
        order(Script::new().keys("space down j space up q")),
        "bcad @1"
    );
    // Esc puts it back where it was grabbed.
    assert_eq!(order(Script::new().keys("space end escape q")), "abcd @0");
    // Enter drops it and goes on to the app's binding.
    assert_eq!(order(Script::new().keys("space pagedown enter")), "bcda @3");
}

#[test]
fn the_carried_row_is_marked() {
    let rows = screen(&run(list_app(4), Script::new().keys("space down q"), 12, 5));
    assert_eq!(rows[1], "↕ a");
    assert_eq!(rows[0], "b");
    let rows = screen(&run(
        list_app(4),
        Script::new().keys("space space q"),
        12,
        5,
    ));
    assert_eq!(rows[0], "a");
}

#[test]
fn a_drag_carries_a_row() {
    let script = Script::new()
        .mouse(MouseKind::Down(Button::Left), 0, 0)
        .mouse(MouseKind::Drag(Button::Left), 0, 1)
        .mouse(MouseKind::Drag(Button::Left), 0, 2)
        .mouse(MouseKind::Up(Button::Left), 0, 2)
        .keys("q");
    assert_eq!(order(script), "bcad @2");
    // Up from the last row; a drag past the list's top stops at the first.
    let script = Script::new()
        .mouse(MouseKind::Down(Button::Left), 0, 3)
        .mouse(MouseKind::Drag(Button::Left), 0, 1)
        .mouse(MouseKind::Drag(Button::Left), 0, 0)
        .mouse(MouseKind::Up(Button::Left), 0, 0)
        .keys("q");
    assert_eq!(order(script), "dabc @0");
}

#[test]
fn a_drag_past_the_bottom_scrolls_the_list() {
    // Two rows shown: a drag below them carries the item a row further
    // each time, and the list follows.
    let script = Script::new()
        .mouse(MouseKind::Down(Button::Left), 0, 0)
        .mouse(MouseKind::Drag(Button::Left), 0, 2)
        .mouse(MouseKind::Drag(Button::Left), 1, 3)
        .mouse(MouseKind::Up(Button::Left), 1, 3)
        .keys("q");
    let rows = screen(&run(list_app(2), script, 12, 3));
    assert_eq!(rows[2], "bcda @3");
    assert_eq!(rows[..2], ["d", "a"]);
}

#[test]
fn one_item_and_a_plain_node() {
    let app = App::new(|| {
        let items = signal(vec!["only".to_string()]);
        let selected = signal(0usize);
        column([
            list(move || items.get(), selected)
                .reorderable(move |step: Move, _| items.update(|items| step.apply(items)))
                .fixed(1),
            label("x").reorderable(|_, _| {}).fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(
        app,
        Script::new().keys("alt+down K space down escape q"),
        8,
        2,
    ));
    assert_eq!(rows, ["only", "x"]);
}

// ---- The calendar's range ----

fn range_app() -> App {
    App::new(|| {
        let day = signal(Date::new(2026, 10, 15));
        let options = CalendarOptions::new()
            .min(Date::new(2026, 10, 10))
            .max(Date::new(2026, 10, 20));
        column([
            calendar_options(day, options).fixed(8),
            text!("on {day}").fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn a_calendar_stops_at_its_range() {
    let on = |script: Script| screen(&run(range_app(), script.keys("q"), 20, 9))[8].clone();
    assert_eq!(on(Script::new().keys("down down")), "on 2026-10-20");
    assert_eq!(on(Script::new().keys("pageup")), "on 2026-10-10");
    assert_eq!(on(Script::new().keys("home")), "on 2026-10-10");
    assert_eq!(on(Script::new().keys("end")), "on 2026-10-20");
    assert_eq!(on(Script::new().scroll(true, 5, 5)), "on 2026-10-20");
    // A click on a day outside it does nothing: the 27th (Tuesday, row 6).
    assert_eq!(on(Script::new().click(4, 6)), "on 2026-10-15");
    // One inside selects: the 19th (Monday, row 5).
    assert_eq!(on(Script::new().click(1, 5)), "on 2026-10-19");
}

/// A calendar whose min is after its max, over the day it holds.
fn empty_range_app() -> App {
    App::new(|| {
        let day = signal(Date::new(2026, 10, 15));
        let options = CalendarOptions::new()
            .min(Date::new(2026, 10, 20))
            .max(Date::new(2026, 10, 10));
        column([
            calendar_options(day, options).fixed(8),
            text!("on {day}").fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn a_calendar_with_an_empty_range_says_so_and_changes_nothing() {
    let script = Script::new()
        .keys("down end pageup")
        .click(4, 4)
        .scroll(true, 5, 5)
        .keys("q");
    let rows = screen(&run(empty_range_app(), script, 20, 9));
    assert_eq!(rows[0], "    October 2026");
    assert_eq!(rows[2..4], ["✗ the range is empty", "min is after max"]);
    assert_eq!(rows[8], "on 2026-10-15");
    // Assistive technology is told it is in error, with no day selected.
    let mut driver = empty_range_app().driver(20, 9);
    driver.settle();
    let tree = driver.accessibility();
    let grid = tree.iter().find(|node| node.role == Role::Grid).unwrap();
    assert!(grid.state.invalid && !grid.state.selected, "{grid:?}");
    assert_eq!(grid.value, None);
}

#[test]
fn a_time_picker_with_an_empty_range_says_so_and_changes_nothing() {
    let options = TimeOptions::new()
        .min(Time::new(17, 0, 0))
        .max(Time::new(9, 0, 0));
    let script = Script::new().keys("1 4 up right down").scroll(true, 1, 0);
    let rows = screen(&run(time_app(options), script.keys("q"), 50, 2));
    assert_eq!(
        rows,
        ["09:05 ✗ the range is empty: min is after max", "at 09:05"]
    );
}

#[test]
fn a_date_and_time_picker_with_an_empty_range_says_so_and_changes_nothing() {
    // The days the wrong way round, then only the times on one day.
    for (min, max) in [
        (moment(20, 9, 0), moment(16, 17, 0)),
        (moment(17, 12, 0), moment(17, 9, 0)),
    ] {
        let options = DateTimeOptions::new().min(min).max(max);
        let script = Script::new().keys("left down tab 1 4 up q");
        let rows = screen(&run(picker_app(options), script, 24, 12));
        assert_eq!(rows[9..11], ["✗ the range is empty", "min is after max"]);
        assert_eq!(rows[11], "at 2026-10-17 09:00");
        // Said once: the calendar shows its month as ever.
        assert_eq!(rows[2], "          1  2  3  4");
    }
}

// ---- The time picker ----

fn time_app(options: TimeOptions) -> App {
    App::new(move || {
        let at = signal(Time::new(9, 5, 0));
        column([
            time_picker_with(at, options).fixed(1),
            text!("at {at}").fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn time_fields_step_and_take_digits() {
    let at = |options, script: Script| screen(&run(time_app(options), script.keys("q"), 16, 2));
    let rows = at(TimeOptions::new(), Script::new().keys("down right up up"));
    assert_eq!(rows, ["08:07", "at 08:07"]);
    // `1` waits for a second digit; `7` fills the minute at once.
    let rows = at(TimeOptions::new(), Script::new().keys("1 6 7"));
    assert_eq!(rows[1], "at 16:07");
    // Left at the first field is left to the app: nothing breaks.
    let rows = at(TimeOptions::new(), Script::new().keys("left up"));
    assert_eq!(rows[1], "at 10:05");
    let rows = at(
        TimeOptions::new().seconds().twelve_hour(),
        Script::new().keys("right right up right p"),
    );
    assert_eq!(rows, ["09:05:01 PM", "at 21:05:01"]);
}

#[test]
fn a_time_stops_at_its_range() {
    let options = TimeOptions::new()
        .min(Time::new(9, 0, 0))
        .max(Time::new(17, 30, 0));
    let at = |script: Script| screen(&run(time_app(options), script.keys("q"), 16, 2))[1].clone();
    assert_eq!(at(Script::new().keys("down")), "at 09:00");
    // Typed past the end, the hour stops at the last, keeping its minutes.
    assert_eq!(at(Script::new().keys("2 0")), "at 17:05");
    assert_eq!(at(Script::new().keys("up")), "at 10:05");
}

#[test]
fn typing_an_hour_keeps_the_minutes_inside_the_range() {
    let app = App::new(|| {
        let at = signal(Time::new(9, 5, 0));
        let options = TimeOptions::new()
            .min(Time::new(9, 0, 0))
            .max(Time::new(17, 30, 0));
        column([
            time_picker_with(at, options).fixed(1),
            text!("at {at}").fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    });
    let rows = app.render_with(&["1", "4", "q"], 16, 2).unwrap();
    assert_eq!(rows[1].trim_end(), "at 14:05");
}

/// A time picker from `start` within 09:30 to 17:30, after `keys`: what it
/// shows, and every time the app's signal held, in order.
fn typed(start: Time, keys: &str) -> Vec<String> {
    let app = App::new(move || {
        let at = signal(start);
        let held = signal(Vec::<String>::new());
        watch(
            move || at.get(),
            move |time, _| held.update(|held| held.push(time.to_string())),
        );
        let options = TimeOptions::new()
            .min(Time::new(9, 30, 0))
            .max(Time::new(17, 30, 0));
        column([
            time_picker_with(at, options).fixed(1),
            text(move || held.get().join(" ")).fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    });
    screen(&run(app, Script::new().keys(keys).keys("q"), 40, 2))
}

#[test]
fn a_field_typed_past_the_range_stops_at_its_end_alone() {
    // A first digit outside the range is not written, nor moved into it.
    let rows = typed(Time::new(12, 45, 0), "0");
    assert_eq!(rows, ["0_:45", "12:45"]);
    // Complete, the hour stops at the range's first, keeping its minutes.
    assert_eq!(typed(Time::new(12, 45, 0), "0 8")[1], "12:45 09:45");
    // The minutes move too only when they must.
    assert_eq!(typed(Time::new(12, 15, 0), "0 8")[1], "12:15 09:30");
    assert_eq!(typed(Time::new(12, 45, 0), "1 9")[1], "12:45 17:30");
    assert_eq!(typed(Time::new(12, 15, 0), "1 9")[1], "12:15 17:15");
    // On the range's last hour, a minute typed past it stops there; its
    // first digit was inside, so it was written at once, as without a
    // range.
    assert_eq!(typed(Time::new(17, 5, 0), "right 4")[0], "17:4_");
    assert_eq!(
        typed(Time::new(17, 5, 0), "right 4 5")[1],
        "17:05 17:04 17:30"
    );
    // Leaving a field with a digit outside the range writes it, moved in.
    assert_eq!(typed(Time::new(12, 45, 0), "0 right")[1], "12:45 09:45");
    assert_eq!(typed(Time::new(12, 45, 0), "0 up")[1], "12:45 10:45");
}

#[test]
fn typing_beside_a_calendar_keeps_the_minutes_and_says_why_a_field_moved() {
    let options = DateTimeOptions::new()
        .min(moment(17, 9, 30))
        .max(moment(20, 17, 0));
    let typed = |keys: &str| {
        let app = App::new(move || {
            let at = signal(moment(17, 9, 45));
            column([
                datetime_picker(at, options).fixed(11),
                text!("at {at}").fixed(1),
            ])
            .on_key("q", |cx| cx.quit())
        });
        screen(&run(app, Script::new().keys(keys).keys("q"), 24, 12))
    };
    // `1` alone is too early on the first day: it waits for the `4`.
    let rows = typed("tab 1 4");
    assert_eq!(rows[8], "Time 14:45");
    assert_eq!(rows[11], "at 2026-10-17 14:45");
    // An hour typed too early moves up alone, and the line says why.
    let rows = typed("tab 0 8");
    assert_eq!(rows[8], "Time 09:45");
    assert_eq!(rows[9..11], ["✗ the earliest is", "2026-10-17 09:30"]);
    assert_eq!(rows[11], "at 2026-10-17 09:45");
}

#[test]
fn a_click_focuses_a_time_field_and_the_wheel_turns_it() {
    let app = App::new(|| {
        let at = signal(Time::new(9, 5, 0));
        column([time_picker(at).fixed(1), text!("at {at}").fixed(1)]).on_key("q", |cx| cx.quit())
    });
    let script = Script::new().click(4, 0).scroll(false, 4, 0).keys("q");
    let rows = screen(&run(app, script, 16, 2));
    assert_eq!(rows[1], "at 09:06");
}

// ---- The date-and-time picker ----

fn moment(day: u32, hour: u32, minute: u32) -> DateTime {
    DateTime::new(Date::new(2026, 10, day), Time::new(hour, minute, 0))
}

fn picker_app(options: DateTimeOptions) -> App {
    App::new(move || {
        let at = signal(moment(17, 9, 0));
        column([
            datetime_picker(at, options).fixed(11),
            text!("at {at}").fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn tab_moves_from_the_day_to_the_time() {
    let rows = screen(&run(
        picker_app(DateTimeOptions::new()),
        Script::new().keys("right tab up right 3 0 q"),
        24,
        12,
    ));
    assert_eq!(rows[0], "    October 2026");
    assert_eq!(rows[8], "Time 10:30");
    assert_eq!(rows[11], "at 2026-10-18 10:30");
}

#[test]
fn the_range_moves_the_time_on_its_first_day_and_says_why() {
    let options = DateTimeOptions::new()
        .min(moment(16, 12, 0))
        .max(moment(20, 17, 0));
    let rows = screen(&run(picker_app(options), Script::new().keys("q"), 24, 12));
    assert_eq!(rows[9..11], ["2026-10-16 12:00 to", "2026-10-20 17:00"]);
    // A day back: 09:00 on the 16th is too early, so it moves to 12:00.
    let rows = screen(&run(
        picker_app(options),
        Script::new().keys("left q"),
        24,
        12,
    ));
    assert_eq!(rows[9..11], ["✗ the earliest is", "2026-10-16 12:00"]);
    assert_eq!(rows[11], "at 2026-10-16 12:00");
    // Two days back cannot be reached; a day on clears the message.
    let rows = screen(&run(
        picker_app(options),
        Script::new().keys("left left right q"),
        24,
        12,
    ));
    assert_eq!(rows[9], "2026-10-16 12:00 to");
    assert_eq!(rows[11], "at 2026-10-17 12:00");
}
