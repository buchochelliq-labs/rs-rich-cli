//! A port of ratatui's `calendar-explorer` example
//! (<https://github.com/ratatui/ratatui/tree/main/examples/apps/calendar-explorer>),
//! rebuilt on intuiTUIve. It reuses none of ratatui's code: it rebuilds
//! the behaviour and the look with intuiTUIve's API.
//!
//! Ported from ratatui 0.30 (<https://github.com/ratatui/ratatui>), by
//! Florian Dehau and the Ratatui Developers, MIT licence. The design, the
//! layout and the keys are theirs; the credit for them is theirs. The days
//! marked are our own choice of international days.
//!
//!     cargo run -p rs-rich-intuituive --example calendar
//!     cargo run -p rs-rich-intuituive --example calendar -- --today 2026-10-17
//!
//! A year of twelve months, four across and three down, with today, the
//! selected day, international days and the seasons marked.
//!
//! h/l (or ←/→) move a day, j/k (or ↓/↑) a week, n/p a month, t back to
//! today; s goes round the six looks; ? lists the keys, Ctrl+P finds a
//! command, q quits. A click on a day selects it, and the wheel turns the
//! month.
//!
//! The looks: plain; with the days of the months either side; with the
//! weekdays' header styled; with the months' header styled; with the
//! seasons coloured; and all of them on a background. ratatui's looks also
//! hide the headers: a `month` node always draws its month and weekday
//! headers (no option hides them, a gap after C7), so here the looks style
//! them instead.
//!
//! What intuiTUIve adds: each month reads only whether the selection is in
//! it, through a `memo`, so moving the selection draws the one or two
//! months it leaves and enters and the line under them, not the year
//! (`tests/calendar.rs` checks it). The months have labels for linear mode.

use intuituive::interact::{Button, MouseKind};
use intuituive::prelude::*;
use intuituive::widgets::{month, CalendarOptions, Date};

/// The looks `s` goes round, by name.
pub const LOOKS: [&str; 6] = [
    "plain",
    "surrounding days",
    "weekday header",
    "month header",
    "seasons",
    "everything",
];

/// The international days marked, with their names: our own choice.
pub const DAYS: [((u32, u32), &str); 10] = [
    ((1, 1), "New Year's Day"),
    ((3, 8), "International Women's Day"),
    ((3, 21), "World Poetry Day"),
    ((4, 22), "Earth Day"),
    ((5, 20), "World Bee Day"),
    ((6, 5), "World Environment Day"),
    ((9, 21), "International Day of Peace"),
    ((10, 5), "World Teachers' Day"),
    ((11, 13), "World Kindness Day"),
    ((12, 31), "New Year's Eve"),
];

/// The international day on `date`, if there is one.
pub fn day_name(date: Date) -> Option<&'static str> {
    DAYS.iter()
        .find(|((m, d), _)| *m == date.month && *d == date.day)
        .map(|(_, name)| *name)
}

/// The season of `date` in the north, and its colour.
pub fn season(date: Date) -> (&'static str, &'static str) {
    match (date.month, date.day) {
        (3, 20..) | (4..=5, _) | (6, ..=20) => ("spring", "green"),
        (6, 21..) | (7..=8, _) | (9, ..=22) => ("summer", "yellow"),
        (9, 23..) | (10..=11, _) | (12, ..=20) => ("autumn", "dark_orange"),
        _ => ("winter", "cyan"),
    }
}

/// How the app runs: its command line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// The day marked as today, and selected first.
    pub today: Date,
    /// Run in linear mode.
    pub linear: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            today: Date::today_utc(),
            linear: false,
        }
    }
}

impl Options {
    /// `--today YYYY-MM-DD` and `--linear`.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
        let mut options = Options::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--today" => {
                    let value = args.next().ok_or("--today takes a date")?;
                    options.today = value
                        .parse()
                        .map_err(|_| format!("--today takes a date, as 2026-10-17, not {value}"))?;
                }
                "--linear" => options.linear = true,
                _ => return Err(format!("unknown argument {arg}")),
            }
        }
        Ok(options)
    }
}

/// A month's options in `look`, with the selected day (`selected` is its
/// day, while it is in this month) reversed over the rest.
fn month_options(look: usize, today: Date, selected: Memo<Option<u32>>) -> CalendarOptions {
    let seasons = matches!(look, 4 | 5);
    let mut options = CalendarOptions::new()
        .today(today)
        .events(move |date: Date| {
            let mut style = Vec::new();
            if seasons {
                style.push(season(date).1);
            }
            if day_name(date).is_some() {
                style.push("bold red underline");
            }
            if selected.get() == Some(date.day) {
                style.push("reverse");
            }
            (!style.is_empty()).then(|| style.join(" "))
        });
    if matches!(look, 1 | 5) {
        options = options.surrounding("dim");
    }
    if matches!(look, 2 | 5) {
        options = options.weekday_style("bold yellow");
    }
    if matches!(look, 3 | 5) {
        options = options.month_style("bold reverse blue");
    }
    if look == 5 {
        options = options.day_style("on grey15");
    }
    options
}

/// The date at `column`, `row` of a month drawn at its node's top left,
/// as the `month` node lays its weeks out from Monday.
pub fn date_at(first: Date, column: u16, row: u16) -> Option<Date> {
    if row < 2 || column >= 20 || column % 3 == 2 {
        return None;
    }
    let at = i64::from(row - 2) * 7 + i64::from(column / 3);
    let day = at - i64::from(first.weekday()) + 1;
    let days = i64::from(Date::days_in_month(first.year, first.month));
    (1..=days)
        .contains(&day)
        .then(|| Date::new(first.year, first.month, day as u32))
}

/// The year, in `look`.
fn year(look: usize, today: Date, selected: Signal<Date>) -> Node {
    let year = memo(move || selected.get().year);
    let months: Vec<Node> = (1..=12u32)
        .map(|m| {
            let here = memo(move || {
                let date = selected.get();
                (date.month == m).then_some(date.day)
            });
            month(
                move || Date::new(year.get(), m, 1),
                month_options(look, today, here),
            )
            .on_mouse(move |_, mouse| match mouse.kind {
                MouseKind::Down(Button::Left) => {
                    let first = Date::new(year.get_untracked(), m, 1);
                    match date_at(first, mouse.column, mouse.row) {
                        Some(date) => {
                            selected.set(date);
                            true
                        }
                        None => false,
                    }
                }
                MouseKind::ScrollUp => {
                    selected.update(|d| *d = d.add_months(-1));
                    true
                }
                MouseKind::ScrollDown => {
                    selected.update(|d| *d = d.add_months(1));
                    true
                }
                _ => false,
            })
            .name("month")
            .label(Date::new(2000, m, 1).month_name())
        })
        .collect();
    grid([Size::Flex(1); 4], months).rows([Size::Fixed(8); 3])
}

/// The app, as `options` says.
pub fn calendar_app(options: Options) -> App {
    let today = options.today;
    App::new(move || {
        let selected = signal(today);
        let look = signal(0usize);
        let title = text(move || {
            format!(
                " [bold]{}[/] · look: [bold]{}[/] [dim](s)[/]",
                selected.get().year,
                LOOKS[look.get()]
            )
        })
        .name("title")
        .fixed(1);
        let body = switch(move || look.get(), move |look| year(look, today, selected));
        let footer = text(move || {
            let date = selected.get();
            let weekday =
                ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][date.weekday() as usize];
            let (season, colour) = season(date);
            let mut line = format!(
                " {weekday} {} {} {} · [{colour}]{season}[/]",
                date.day,
                date.month_name(),
                date.year
            );
            if date == today {
                line.push_str(" · [bold]today[/]");
            }
            if let Some(name) = day_name(date) {
                line.push_str(&format!(" · [bold red]{name}[/]"));
            }
            line
        })
        .name("footer")
        .live()
        .fixed(1);
        let by = move |days: i64| move |_: &mut Ctx| selected.update(|d| *d = d.add_days(days));
        let months = move |n: i32| move |_: &mut Ctx| selected.update(|d| *d = d.add_months(n));
        column([title, body.flex(1), footer])
            .bind("left h", "the day before", by(-1))
            .bind("right l", "the day after", by(1))
            .bind("up k", "the week before", by(-7))
            .bind("down j", "the week after", by(7))
            .bind("p pageup", "the month before", months(-1))
            .bind("n pagedown", "the month after", months(1))
            .bind("t", "back to today", move |_| selected.set(today))
            .bind("s", "the next look", move |_| {
                look.update(|l| *l = (*l + 1) % LOOKS.len())
            })
            .bind("q", "quit", |cx| cx.quit())
    })
    .help_key("?")
    .palette_key("ctrl+p")
    .linear(options.linear)
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let options = Options::parse(std::env::args().skip(1))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    calendar_app(options).run()
}
