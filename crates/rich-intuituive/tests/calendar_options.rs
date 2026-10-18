//! Calendar events, the days of the months either side, the first weekday,
//! styles per calendar, and months that only show (0.0.19 gap batch C7,
//! after ratatui's `calendar::Monthly`). Snapshots are in
//! `tests/snapshots`; `UPDATE_SNAPSHOTS=1` writes them.

mod common;

use common::{run, screen};
use intuituive::interact::headless::Script;
use intuituive::prelude::*;
use intuituive::widgets::{calendar_options, month, CalendarOptions, Date, Weekday};
use intuituive::{assert_screen, assert_screen_styled};

/// December 2026: today on the 9th, Christmas and Boxing Day, weekends.
fn december() -> CalendarOptions {
    CalendarOptions::new()
        .today(Date::new(2026, 12, 9))
        .event(Date::new(2026, 12, 25), "red underline")
        .event(Date::new(2026, 12, 26), "red")
        .events(|date: Date| (date.weekday() >= 5).then_some("blue"))
}

/// A calendar on `day` with `options`, over a line naming the day.
fn calendar_app(day: Date, options: CalendarOptions) -> App {
    App::new(move || {
        let day = signal(day);
        column([
            calendar_options(day, options.clone()).fixed(8),
            text!("on {day}").fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn calendar_events() {
    let mut driver = calendar_app(Date::new(2026, 12, 1), december()).driver(20, 9);
    driver.settle();
    assert_screen_styled!(driver, "calendar_events");
    let styled = intuituive::testing::styled(&driver);
    // Christmas in its own style; the weekends from the function; the
    // event wins over the function on Boxing Day, a Saturday.
    assert!(styled.contains("[underline red]25[/]"), "{styled}");
    assert!(styled.contains("[red]26[/]"), "{styled}");
    assert!(styled.contains("[blue]27[/]"), "{styled}");
    // Today, and the selection over everything.
    assert!(styled.contains("[bold] 9[/]"), "{styled}");
    assert!(styled.contains("[reverse] 1[/]"), "{styled}");
}

#[test]
fn calendar_surrounding() {
    let options = CalendarOptions::new().surrounding("dim");
    let mut driver = calendar_app(Date::new(2026, 10, 7), options.clone()).driver(20, 9);
    driver.settle();
    assert_screen_styled!(driver, "calendar_surrounding");
    let rows = screen(&run(
        calendar_app(Date::new(2026, 10, 7), options.clone()),
        Script::new().keys("q"),
        20,
        9,
    ));
    // September's last days before the 1st, November's first after the
    // 31st, in the first and last weeks only.
    assert_eq!(rows[2], "28 29 30  1  2  3  4");
    assert_eq!(rows[6], "26 27 28 29 30 31  1");
    assert_eq!(rows[7], "");
    // A click on one selects it.
    let on = |script: Script| {
        screen(&run(
            calendar_app(Date::new(2026, 10, 7), options.clone()),
            script.keys("q"),
            20,
            9,
        ))[8]
            .clone()
    };
    assert_eq!(on(Script::new().click(1, 2)), "on 2026-09-28");
    assert_eq!(on(Script::new().click(19, 6)), "on 2026-11-01");
    // Without surrounding days, the same cells are empty and do nothing.
    let plain = |script: Script| {
        screen(&run(
            calendar_app(Date::new(2026, 10, 7), CalendarOptions::new()),
            script.keys("q"),
            20,
            9,
        ))[8]
            .clone()
    };
    assert_eq!(plain(Script::new().click(1, 2)), "on 2026-10-07");
}

#[test]
fn calendar_sunday_first() {
    let options = CalendarOptions::new().first_weekday(Weekday::Sunday);
    let mut driver = calendar_app(Date::new(2026, 10, 7), options.clone()).driver(20, 9);
    driver.settle();
    assert_screen!(driver, "calendar_sunday_first");
    let rows = screen(&run(
        calendar_app(Date::new(2026, 10, 7), options.clone()),
        Script::new().keys("q"),
        20,
        9,
    ));
    assert_eq!(rows[1], "Su Mo Tu We Th Fr Sa");
    // October 2026 starts on a Thursday.
    assert_eq!(rows[2], "             1  2  3");
    assert_eq!(rows[3], " 4  5  6  7  8  9 10");
    // The keys move as before; a click picks the day under it.
    let on = |script: Script| {
        screen(&run(
            calendar_app(Date::new(2026, 10, 7), options.clone()),
            script.keys("q"),
            20,
            9,
        ))[8]
            .clone()
    };
    assert_eq!(on(Script::new().keys("right down")), "on 2026-10-15");
    // Sunday the 4th is the first column of row 3.
    assert_eq!(on(Script::new().click(1, 3)), "on 2026-10-04");
    // Every first weekday lays the month out the same way round.
    for first in Weekday::ALL {
        let rows = screen(&run(
            calendar_app(
                Date::new(2026, 10, 7),
                CalendarOptions::new().first_weekday(first),
            ),
            Script::new().keys("q"),
            20,
            9,
        ));
        assert!(
            rows[1].starts_with(first.short_name()),
            "{first:?}: {rows:?}"
        );
        // The 1st falls in the column of its own weekday, a Thursday.
        let column = rows[2].find(" 1").unwrap() / 3;
        let names: Vec<&str> = rows[1].split(' ').collect();
        assert_eq!(names[column], "Th", "{first:?}: {rows:?}");
    }
}

#[test]
fn styles_for_one_calendar() {
    let options = CalendarOptions::new()
        .weekday_style("bold green")
        .month_style("italic")
        .day_style("on grey19");
    let mut driver = calendar_app(Date::new(2026, 10, 7), options).driver(20, 9);
    driver.settle();
    let styled = intuituive::testing::styled(&driver);
    let rows: Vec<&str> = styled.lines().collect();
    assert!(rows[0].contains("[italic]October 2026[/]"), "{styled}");
    assert!(rows[1].starts_with("[bold green]Mo Tu"), "{styled}");
    // The days' background fills the weeks of the month, gaps included.
    assert!(rows[3].starts_with("[on grey19] 5  6 "), "{styled}");
    assert!(rows[2].starts_with("[on grey19]         "), "{styled}");
}

#[test]
fn calendar_year() {
    // Twelve months that only show, three to a row: a year, as ratatui's
    // calendar-explorer draws one.
    let app = App::new(|| {
        let year = signal(2026);
        let months: Vec<Node> = (1..=12)
            .map(|m| {
                let options = CalendarOptions::new()
                    .today(Date::new(2026, 10, 17))
                    .event(Date::new(2026, 12, 25), "red");
                month(move || Date::new(year.get(), m, 1), options).padding(0, 1)
            })
            .collect();
        grid([Size::Fixed(22); 3], months).on_key("q", |cx| cx.quit())
    });
    let mut driver = app.driver(66, 32);
    driver.settle();
    assert_screen!(driver, "calendar_year");
    let rows = intuituive::testing::plain(&driver);
    let rows: Vec<&str> = rows.lines().collect();
    assert!(rows[0].contains("January 2026") && rows[0].contains("March 2026"));
    assert!(rows[24].contains("October 2026") && rows[24].contains("December 2026"));
    // Months only show: nothing takes the focus.
    assert_eq!(driver.focused(), None);
}

#[test]
fn a_month_follows_its_signal() {
    let app = App::new(|| {
        let shown = signal(Date::new(2026, 1, 1));
        column([
            month(move || shown.get(), CalendarOptions::new()).fixed(8),
            label("next").fixed(1),
        ])
        .on_key("n", move |_| shown.update(|d| *d = d.add_months(1)))
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("n n q"), 20, 9));
    assert_eq!(rows[0].trim(), "March 2026");
}

#[test]
fn the_options_compare_and_print() {
    let a = december();
    let b = a.clone();
    assert_eq!(a, b);
    // Another function is another option, even one that marks the same.
    let c = december();
    assert_ne!(a, c);
    let debug = format!("{a:?}");
    assert!(debug.contains("first_weekday: Monday"), "{debug}");
    assert_eq!(
        a.event_style(Date::new(2026, 12, 26)).as_deref(),
        Some("red")
    );
    assert_eq!(
        a.event_style(Date::new(2026, 12, 27)).as_deref(),
        Some("blue")
    );
    assert_eq!(a.event_style(Date::new(2026, 12, 28)), None);
}
