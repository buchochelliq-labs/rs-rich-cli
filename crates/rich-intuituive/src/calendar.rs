//! A month calendar with a selected day, within an optional range (#719),
//! with styled dates, the days of the months either side, a first weekday
//! of your choice, and a month that only shows (0.0.19 gap batch C7); a
//! year of months laid out as they fit, and an agenda of the days ahead.
//! The date types are rs-rich-interact's, shared with its date pickers.

use std::fmt;
use std::rc::Rc;

use rich::style::StyleType;
use rich::Style;
use rich_interact::{Button, KeyCode, MouseKind};

pub use rich_interact::datetime::{Date, DateTime, Time};

use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

/// A day of the week, for [`CalendarOptions::first_weekday`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Weekday {
    #[default]
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    /// Every day, Monday first.
    pub const ALL: [Weekday; 7] = [
        Weekday::Monday,
        Weekday::Tuesday,
        Weekday::Wednesday,
        Weekday::Thursday,
        Weekday::Friday,
        Weekday::Saturday,
        Weekday::Sunday,
    ];

    /// 0 for Monday to 6 for Sunday, as [`Date::weekday`] counts.
    pub fn index(self) -> u32 {
        self as u32
    }

    /// The day of `date`.
    pub fn of(date: Date) -> Weekday {
        Weekday::ALL[date.weekday() as usize % 7]
    }

    /// The day's first two letters, as a calendar heads its column: `Mo`.
    pub fn short_name(self) -> &'static str {
        ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"][self as usize]
    }

    /// The day's name, in English: `Monday`.
    pub fn name(self) -> &'static str {
        [
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
            "Sunday",
        ][self as usize]
    }
}

/// What a month's title says, for [`CalendarOptions::title`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MonthTitle {
    /// `October 2026`.
    #[default]
    NameYear,
    /// `October`: in a year whose number is shown once.
    Name,
    /// No title: the weekdays on the first row.
    None,
}

/// A function giving a date's style, for [`CalendarOptions::events`].
type Styler = Rc<dyn Fn(Date) -> Option<String>>;

/// How a calendar marks, limits and draws its days: today, the first and
/// last days that can be selected, styled dates (events), the days of the
/// months either side, the weekday the weeks start on, and the styles of
/// its headers and days. Styles are theme keys or definitions
/// (`"bold red"`).
///
/// It is `Clone` but not `Copy` (0.0.19): it holds strings and a function.
#[derive(Clone, Default)]
#[must_use = "options do nothing until they are passed to what takes them"]
pub struct CalendarOptions {
    pub today: Option<Date>,
    pub min: Option<Date>,
    pub max: Option<Date>,
    pub(crate) events: Vec<(Date, String)>,
    pub(crate) styler: Option<Styler>,
    pub(crate) surrounding: Option<String>,
    pub(crate) first_weekday: Weekday,
    pub(crate) weekday_style: Option<String>,
    pub(crate) month_style: Option<String>,
    pub(crate) month_styler: Option<Styler>,
    pub(crate) title: MonthTitle,
    pub(crate) day_style: Option<String>,
}

impl fmt::Debug for CalendarOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CalendarOptions")
            .field("today", &self.today)
            .field("min", &self.min)
            .field("max", &self.max)
            .field("events", &self.events)
            .field("styler", &self.styler.as_ref().map(|_| "Fn(Date)"))
            .field("surrounding", &self.surrounding)
            .field("first_weekday", &self.first_weekday)
            .field("weekday_style", &self.weekday_style)
            .field("month_style", &self.month_style)
            .field(
                "month_styler",
                &self.month_styler.as_ref().map(|_| "Fn(Date)"),
            )
            .field("title", &self.title)
            .field("day_style", &self.day_style)
            .finish()
    }
}

impl PartialEq for CalendarOptions {
    /// Equal when every setting is, and both have the same events function
    /// (the same one, not one that gives the same styles) or neither has.
    fn eq(&self, other: &Self) -> bool {
        let same = |a: &Option<Styler>, b: &Option<Styler>| match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        };
        same(&self.styler, &other.styler)
            && same(&self.month_styler, &other.month_styler)
            && self.title == other.title
            && self.today == other.today
            && self.min == other.min
            && self.max == other.max
            && self.events == other.events
            && self.surrounding == other.surrounding
            && self.first_weekday == other.first_weekday
            && self.weekday_style == other.weekday_style
            && self.month_style == other.month_style
            && self.day_style == other.day_style
    }
}

impl CalendarOptions {
    pub fn new() -> CalendarOptions {
        CalendarOptions::default()
    }

    /// Mark `today` in the theme's `calendar.today` style.
    pub fn today(mut self, today: Date) -> CalendarOptions {
        self.today = Some(today);
        self
    }

    /// The earliest day that can be selected.
    pub fn min(mut self, min: Date) -> CalendarOptions {
        self.min = Some(min);
        self
    }

    /// The latest day that can be selected.
    pub fn max(mut self, max: Date) -> CalendarOptions {
        self.max = Some(max);
        self
    }

    /// Draw `date` in `style`, as ratatui's `CalendarEventStore` does: a
    /// holiday, a deadline. A later call for the same date wins.
    pub fn event(mut self, date: Date, style: impl Into<String>) -> CalendarOptions {
        let style = style.into();
        self.events.retain(|(d, _)| *d != date);
        self.events.push((date, style));
        self
    }

    /// Style dates with a function: `Some(style)` for the dates it marks.
    /// Dates given to [`event`](Self::event) take their own style instead.
    pub fn events<S: Into<String>>(
        mut self,
        styler: impl Fn(Date) -> Option<S> + 'static,
    ) -> CalendarOptions {
        self.styler = Some(Rc::new(move |date| styler(date).map(Into::into)));
        self
    }

    /// Show the days of the months either side in the first and last
    /// weeks, in `style` (ratatui's `show_surrounding`). A click on one
    /// selects it.
    pub fn surrounding(mut self, style: impl Into<String>) -> CalendarOptions {
        self.surrounding = Some(style.into());
        self
    }

    /// The day the weeks start on (default Monday).
    pub fn first_weekday(mut self, day: Weekday) -> CalendarOptions {
        self.first_weekday = day;
        self
    }

    /// The weekday header's style (default the theme's `calendar.header`,
    /// dim).
    pub fn weekday_style(mut self, style: impl Into<String>) -> CalendarOptions {
        self.weekday_style = Some(style.into());
        self
    }

    /// The month and year's style (default the theme's `calendar.title`,
    /// bold).
    pub fn month_style(mut self, style: impl Into<String>) -> CalendarOptions {
        self.month_style = Some(style.into());
        self
    }

    /// The month's style by the month: `style(first_of_month)`, or `None`
    /// for [`month_style`](Self::month_style)'s. taskwarrior-tui bolds
    /// only the current month's title.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{month, CalendarOptions, Date};
    ///
    /// let this_month = Date::new(2026, 10, 1);
    /// let options = CalendarOptions::new()
    ///     .month_style("default")
    ///     .month_style_with(move |first| (first == this_month).then_some("bold"));
    /// let app = App::new(move || month(|| Date::new(2026, 10, 1), options.clone()));
    /// let mut driver = app.driver(20, 8);
    /// driver.settle();
    /// let styled = intuituive::testing::styled(&driver);
    /// assert!(styled.lines().next().unwrap().contains("[bold]October 2026"));
    /// ```
    pub fn month_style_with<S: Into<String>>(
        mut self,
        style: impl Fn(Date) -> Option<S> + 'static,
    ) -> CalendarOptions {
        self.month_styler = Some(Rc::new(move |date| style(date).map(Into::into)));
        self
    }

    /// What the month's title says: [`MonthTitle::NameYear`] (`October
    /// 2026`, the default), [`MonthTitle::Name`] (`October`) or none.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{month, CalendarOptions, Date, MonthTitle};
    ///
    /// let options = CalendarOptions::new().title(MonthTitle::Name);
    /// let app = App::new(move || month(|| Date::new(2026, 10, 1), options.clone()));
    /// assert_eq!(app.render_with(&[], 20, 8).unwrap()[0].trim_end(), "      October");
    /// ```
    pub fn title(mut self, title: MonthTitle) -> CalendarOptions {
        self.title = title;
        self
    }

    /// The days' style, under everything else (ratatui's `default_style`).
    /// Its background fills the weeks.
    pub fn day_style(mut self, style: impl Into<String>) -> CalendarOptions {
        self.day_style = Some(style.into());
        self
    }

    /// Whether `date` is in the range.
    pub fn allows(&self, date: Date) -> bool {
        self.min.is_none_or(|min| date >= min) && self.max.is_none_or(|max| date <= max)
    }

    /// Whether no day is in the range: its min is after its max.
    pub(crate) fn empty(&self) -> bool {
        empty(self.min, self.max)
    }

    /// `date`, moved into the range.
    pub fn clamp(&self, date: Date) -> Date {
        let mut date = date;
        if let Some(min) = self.min {
            date = date.max(min);
        }
        if let Some(max) = self.max {
            date = date.min(max);
        }
        date
    }

    /// The style `date` is marked with: its own event's, or the events
    /// function's.
    pub fn event_style(&self, date: Date) -> Option<String> {
        self.events
            .iter()
            .find(|(d, _)| *d == date)
            .map(|(_, style)| style.clone())
            .or_else(|| self.styler.as_ref().and_then(|styler| styler(date)))
    }

    /// The column, 0 to 6, `date` is drawn in.
    fn column(&self, date: Date) -> u32 {
        (date.weekday() + 7 - self.first_weekday.index()) % 7
    }

    /// Where the first of `date`'s month falls in its first week.
    fn offset(&self, date: Date) -> u32 {
        self.column(date.first_of_month())
    }

    /// The cell (column, row) of `date` in the grid of its month.
    fn cell(&self, date: Date) -> (u16, u16) {
        let at = self.offset(date) + date.day - 1;
        ((at % 7) as u16 * 3, header_rows(self) + (at / 7) as u16)
    }

    /// The date drawn at grid position `at` (from the first week's first
    /// column) of `shown`'s month: a day of it, or with surrounding days a
    /// day of the months either side.
    fn date_at(&self, shown: Date, at: u32) -> Option<Date> {
        let first = shown.first_of_month();
        let days = Date::days_in_month(shown.year, shown.month);
        let offset = self.offset(shown);
        let date = first.add_days(i64::from(at) - i64::from(offset));
        let inside = (offset..offset + days).contains(&at);
        // The weeks the month takes: its own days, and the rest of them.
        let weeks = (offset + days).div_ceil(7);
        (inside || (self.surrounding.is_some() && at < weeks * 7)).then_some(date)
    }
}

/// Where a calendar's day is kept: a day, or the day of a moment whose
/// time a [`datetime_picker`](crate::widgets::datetime_picker) picks
/// beside it.
#[derive(Clone, Copy)]
pub(crate) enum DaySource {
    Day(Signal<Date>),
    Moment {
        value: Signal<DateTime>,
        min: Option<DateTime>,
        max: Option<DateTime>,
        /// Why the last change was moved into the range.
        message: Signal<Option<String>>,
    },
}

impl DaySource {
    /// The day, subscribing the widget that is drawing.
    fn get(&self) -> Date {
        match self {
            DaySource::Day(day) => day.get(),
            DaySource::Moment { value, .. } => value.get().date,
        }
    }

    fn get_untracked(&self) -> Date {
        match self {
            DaySource::Day(day) => day.get_untracked(),
            DaySource::Moment { value, .. } => value.get_untracked().date,
        }
    }

    fn set(&self, date: Date) {
        match *self {
            DaySource::Day(day) => day.set(date),
            DaySource::Moment {
                value,
                min,
                max,
                message,
            } => {
                let at = value.get_untracked();
                set_moment(value, message, min, max, DateTime { date, ..at });
            }
        }
    }
}

/// Whether a range from `min` to `max` holds nothing: `min` is after
/// `max`.
pub(crate) fn empty<T: PartialOrd>(min: Option<T>, max: Option<T>) -> bool {
    matches!((min, max), (Some(min), Some(max)) if min > max)
}

/// What a picker whose range is empty says, as rs-rich-interact's pickers
/// do ("the range is empty: min is after max"), on two lines of twenty
/// cells.
pub(crate) const EMPTY: [&str; 2] = ["✗ the range is empty", "min is after max"];

/// `at` moved into the range, and why it had to move.
pub(crate) fn into_range(
    min: Option<DateTime>,
    max: Option<DateTime>,
    at: DateTime,
) -> (DateTime, Option<String>) {
    match (min, max) {
        (Some(min), _) if at < min => (min, Some(format!("the earliest is {min}"))),
        (_, Some(max)) if at > max => (max, Some(format!("the latest is {max}"))),
        _ => (at, None),
    }
}

/// Set `value` to `at` moved into the range, and say in `message` when it
/// had to move. An empty range changes nothing.
pub(crate) fn set_moment(
    value: Signal<DateTime>,
    message: Signal<Option<String>>,
    min: Option<DateTime>,
    max: Option<DateTime>,
    at: DateTime,
) {
    if empty(min, max) {
        return;
    }
    let (at, why) = into_range(min, max, at);
    value.set(at);
    if message.get_untracked() != why {
        message.set(why);
    }
}

/// Options a function gives, read again when it draws (and subscribing
/// to the signals it reads).
type OptionsFn = Rc<dyn Fn() -> CalendarOptions>;

struct Calendar {
    source: DaySource,
    options: CalendarOptions,
    dynamic: Option<OptionsFn>,
}

/// The width and height of a calendar.
const WIDTH: u16 = 20;
const HEIGHT: u16 = 8;

/// A month of days, with one selected: the month and year, the weekdays
/// from Monday, then the weeks. ←/→ move the selection a day, ↑/↓ a week,
/// PgUp/PgDn a month, and Home/End to the first and last day of the month.
/// A click selects a day, and the wheel turns the month. The selected day
/// is highlighted in the theme's `selected` style while the calendar has
/// the focus, and `calendar.selected` otherwise. It is 20 x 8 cells.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{calendar, Date};
///
/// let app = App::new(|| {
///     let day = signal(Date::new(2026, 10, 7));
///     calendar(day).on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 20, 8).unwrap();
/// assert_eq!(screen[0].trim_end(), "    October 2026");
/// assert_eq!(screen[1].trim_end(), "Mo Tu We Th Fr Sa Su");
/// assert_eq!(screen[2].trim_end(), "          1  2  3  4");
/// ```
pub fn calendar(selected: Signal<Date>) -> Node {
    calendar_options(selected, CalendarOptions::default())
}

/// A [`calendar`] that marks `today` in the theme's `calendar.today` style.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{calendar_with, Date};
///
/// let app = App::new(|| {
///     let day = signal(Date::new(2026, 10, 7));
///     calendar_with(day, Some(Date::new(2026, 10, 7))).on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["right", "q"], 20, 8).unwrap();
/// assert_eq!(screen[3].trim_end(), " 5  6  7  8  9 10 11");
/// ```
pub fn calendar_with(selected: Signal<Date>, today: Option<Date>) -> Node {
    calendar_options(
        selected,
        CalendarOptions {
            today,
            ..CalendarOptions::default()
        },
    )
}

/// A [`calendar`] with [`CalendarOptions`]: a range (#719), styled dates,
/// the days of the months either side, the first weekday and the styles.
///
/// Days before [`min`](CalendarOptions::min) or after
/// [`max`](CalendarOptions::max) are drawn in the theme's
/// `calendar.disabled` style (dim unless set), the keys and the wheel stop
/// at the range's ends, and a click on a day outside it does nothing. A
/// range whose min is after its max is empty: where the weeks go, the
/// calendar says so, as rs-rich-interact's pickers do ("the range is empty:
/// min is after max"), in the theme's `error` style, and nothing changes
/// the day.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{calendar_options, CalendarOptions, Date};
///
/// let app = App::new(|| {
///     let day = signal(Date::new(2026, 10, 7));
///     let range = CalendarOptions::new()
///         .min(Date::new(2026, 10, 5))
///         .max(Date::new(2026, 10, 9));
///     column([
///         calendar_options(day, range).fixed(8),
///         text(move || format!("on {}", day.get())),
///     ])
///     .on_key("q", |cx| cx.quit())
/// });
/// // A week on is past the end: the selection stops on the last day.
/// let screen = app.render_with(&["down", "q"], 20, 9).unwrap();
/// assert_eq!(screen[8].trim_end(), "on 2026-10-09");
/// ```
///
/// Sunday first, with the days either side and a holiday:
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{calendar_options, CalendarOptions, Date, Weekday};
///
/// let app = App::new(|| {
///     let day = signal(Date::new(2026, 12, 1));
///     let options = CalendarOptions::new()
///         .event(Date::new(2026, 12, 25), "red underline")
///         .surrounding("dim")
///         .first_weekday(Weekday::Sunday);
///     calendar_options(day, options).on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 20, 8).unwrap();
/// assert_eq!(screen[1].trim_end(), "Su Mo Tu We Th Fr Sa");
/// assert_eq!(screen[2].trim_end(), "29 30  1  2  3  4  5");
/// assert_eq!(screen[6].trim_end(), "27 28 29 30 31  1  2");
/// ```
pub fn calendar_options(selected: Signal<Date>, options: CalendarOptions) -> Node {
    widget(Calendar {
        source: DaySource::Day(selected),
        options,
        dynamic: None,
    })
}

/// A [`calendar_options`] whose options `options` gives, read again
/// whenever a signal it reads changes: today that moves at midnight, a
/// range that follows another field, events from the app's state.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{calendar_options_with, CalendarOptions, Date};
///
/// let app = App::new(|| {
///     let day = signal(Date::new(2026, 10, 7));
///     let latest = signal(Date::new(2026, 10, 9));
///     column([
///         calendar_options_with(day, move || CalendarOptions::new().max(latest.get())).fixed(8),
///         text!("{day}"),
///     ])
///     .on_key("m", move |_| latest.set(Date::new(2026, 10, 31)))
/// });
/// let mut driver = app.driver(20, 9);
/// driver.press("down");
/// assert_eq!(driver.screen().plain()[8].trim_end(), "2026-10-09");
/// driver.press("m down");
/// assert_eq!(driver.screen().plain()[8].trim_end(), "2026-10-16");
/// ```
pub fn calendar_options_with(
    selected: Signal<Date>,
    options: impl Fn() -> CalendarOptions + 'static,
) -> Node {
    widget(Calendar {
        source: DaySource::Day(selected),
        options: options(),
        dynamic: Some(Rc::new(options)),
    })
}

/// The calendar half of a date-and-time picker.
pub(crate) fn moment_calendar(source: DaySource, options: CalendarOptions) -> Node {
    widget(Calendar {
        source,
        options,
        dynamic: None,
    })
}

/// A month that only shows, as ratatui's `calendar::Monthly`: the month of
/// the date `shown` gives (it may read signals), with today, the events,
/// the surrounding days and the styles of `options`, and no selection. It
/// takes no focus and no keys; twelve in a [`grid`](crate::grid) make a
/// year. It is 20 x 8 cells, as a [`calendar`] is.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{month, CalendarOptions, Date};
///
/// let app = App::new(|| {
///     let shown = signal(Date::new(2026, 2, 1));
///     month(move || shown.get(), CalendarOptions::new()).on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 20, 8).unwrap();
/// assert_eq!(screen[0].trim_end(), "   February 2026");
/// assert_eq!(screen[2].trim_end(), "                   1");
/// ```
pub fn month(shown: impl Fn() -> Date + 'static, options: CalendarOptions) -> Node {
    let options = Rc::new(options);
    month_with(shown, move || (*options).clone())
}

/// A [`month`] whose options `options` gives, read again whenever a
/// signal it reads changes: its events from the app's tasks, its title's
/// style from the year shown.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{month_with, CalendarOptions, Date};
/// use intuituive::Driver;
///
/// let app = App::new(|| {
///     let due = signal(Date::new(2026, 10, 7));
///     month_with(
///         || Date::new(2026, 10, 1),
///         move || CalendarOptions::new().event(due.get(), "underline"),
///     )
///     .on_key("n", move |_| due.set(Date::new(2026, 10, 8)))
/// });
/// let mut driver = app.driver(20, 8);
/// let week = |driver: &Driver| intuituive::testing::styled(driver).lines().nth(3).unwrap().to_string();
/// driver.settle();
/// assert!(week(&driver).contains("[underline] 7"));
/// driver.press("n");
/// assert!(week(&driver).contains("[underline] 8"));
/// ```
pub fn month_with(
    shown: impl Fn() -> Date + 'static,
    options: impl Fn() -> CalendarOptions + 'static,
) -> Node {
    widget(Month {
        shown: Box::new(shown),
        options: Box::new(options),
    })
}

struct Month {
    shown: Box<dyn Fn() -> Date>,
    options: Box<dyn Fn() -> CalendarOptions>,
}

/// A style given by name or definition, as the calendar's options hold
/// them; one that does not resolve is no style.
fn named(cx: &DrawCx, spec: &str) -> Style {
    cx.console()
        .get_style(&StyleType::Name(spec.to_string()))
        .unwrap_or_default()
}

/// Draw the month of `shown`, its day `selected` highlighted when there is
/// one, with `options`.
/// The month title's style: the month's own, else the options', else the
/// theme's.
fn title_style(cx: &DrawCx, options: &CalendarOptions, shown: Date) -> Style {
    let own = (options.month_styler.as_ref()).and_then(|style| style(shown.first_of_month()));
    match own.as_ref().or(options.month_style.as_ref()) {
        Some(spec) => named(cx, spec),
        None => cx.style("calendar.title", "bold"),
    }
}

/// Draw `shown`'s title as `options` say, centred on the first row.
fn draw_title(cx: &DrawCx, canvas: &mut Canvas, shown: Date, options: &CalendarOptions) {
    let title = match options.title {
        MonthTitle::NameYear => format!("{} {}", shown.month_name(), shown.year),
        MonthTitle::Name => shown.month_name().to_string(),
        MonthTitle::None => return,
    };
    let x = WIDTH.saturating_sub(title.len() as u16) / 2;
    canvas.print(x, 0, &title, Some(&title_style(cx, options, shown)));
}

/// The rows above the weeks: the title's (when there is one) and the
/// weekdays'.
fn header_rows(options: &CalendarOptions) -> u16 {
    if options.title == MonthTitle::None {
        1
    } else {
        2
    }
}

fn draw_month(
    cx: &mut DrawCx,
    canvas: &mut Canvas,
    shown: Date,
    selected: Option<Date>,
    options: &CalendarOptions,
) {
    draw_title(cx, canvas, shown, options);
    let top = header_rows(options);
    let header = match &options.weekday_style {
        Some(spec) => named(cx, spec),
        None => cx.style("calendar.header", "dim"),
    };
    let names: Vec<&str> = (0..7)
        .map(|i| Weekday::ALL[((options.first_weekday.index() + i) % 7) as usize].short_name())
        .collect();
    canvas.print(0, top - 1, &names.join(" "), Some(&header));
    // A month that only shows never asks about the focus, so it does not
    // draw again when the focus moves.
    let highlight = match selected {
        Some(_) if cx.focused() => cx.style("selected", "reverse"),
        Some(_) => cx.style("calendar.selected", "underline"),
        None => Style::new(),
    };
    let today = cx.style("calendar.today", "bold");
    let disabled = cx.style("calendar.disabled", "dim");
    let day_style = options.day_style.as_deref().map(|spec| named(cx, spec));
    let surrounding = options.surrounding.as_deref().map(|spec| named(cx, spec));
    let days = Date::days_in_month(shown.year, shown.month);
    let weeks = (options.offset(shown) + days).div_ceil(7);
    // The day style's background fills the weeks.
    if let Some(back) = day_style
        .as_ref()
        .and_then(|s| s.bgcolor())
        .map(|c| Style::new().with_bgcolor(c.clone()))
    {
        canvas.fill(0, top, WIDTH, weeks as u16, Some(&back));
    }
    for at in 0..weeks * 7 {
        let Some(date) = options.date_at(shown, at) else {
            continue;
        };
        let (x, y) = ((at % 7) as u16 * 3, top + (at / 7) as u16);
        let outside = date.month != shown.month || date.year != shown.year;
        // Under the marks: the days' style, the surrounding days', the
        // events'. Then today, the range and the selection, as before.
        let mut layers: Vec<Style> = Vec::new();
        layers.extend(day_style.clone());
        if outside {
            layers.extend(surrounding.clone());
        }
        if let Some(spec) = options.event_style(date) {
            layers.push(named(cx, &spec));
        }
        let mark = if Some(date) == selected {
            let mut style = highlight.clone();
            if Some(date) == options.today {
                style = today.combine(&style);
            }
            Some(style)
        } else if !options.allows(date) {
            Some(disabled.clone())
        } else if Some(date) == options.today {
            Some(today.clone())
        } else {
            None
        };
        layers.extend(mark);
        let style = layers
            .into_iter()
            .reduce(|under, over| under.combine(&over));
        canvas.print(x, y, &format!("{:>2}", date.day), style.as_ref());
    }
}

impl Calendar {
    fn change(&self, f: impl FnOnce(Date) -> Date) {
        let date = self.options.clamp(f(self.source.get_untracked()));
        self.source.set(date);
    }
}

impl Widget for Calendar {
    fn name(&self) -> &'static str {
        "calendar"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::Grid
    }

    fn cursor(&self) -> Option<crate::screen::Rect> {
        if self.options.empty() {
            return None;
        }
        // The selected day's cell, as `draw` places it.
        let (x, y) = self.options.cell(self.source.get_untracked());
        Some(crate::screen::Rect::new(x, y, 2, 1))
    }

    fn access_state(&self) -> crate::a11y::AccessState {
        let empty = self.options.empty();
        crate::a11y::AccessState {
            selected: !empty,
            invalid: empty,
            ..Default::default()
        }
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => WIDTH,
            Axis::Vertical => HEIGHT,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        if let Some(options) = &self.dynamic {
            self.options = options();
        }
        let selected = self.source.get();
        if self.options.empty() {
            // No day can be selected: why, where the weeks go.
            draw_title(cx, canvas, selected, &self.options);
            let style = cx.theme().error_style("error");
            for (row, line) in (2..).zip(EMPTY) {
                canvas.print(0, row, line, Some(&style));
            }
            return;
        }
        draw_month(cx, canvas, selected, Some(selected), &self.options);
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        if let Some(options) = &self.dynamic {
            self.options = options();
        }
        let top = header_rows(&self.options);
        if self.options.empty() {
            // Nothing can be selected, so nothing changes the day.
            return Used::No;
        }
        match event {
            WidgetEvent::Key(key) => {
                if key.modifiers.ctrl || key.modifiers.alt {
                    return Used::No;
                }
                match key.code {
                    KeyCode::Left => self.change(|d| d.add_days(-1)),
                    KeyCode::Right => self.change(|d| d.add_days(1)),
                    KeyCode::Up => self.change(|d| d.add_days(-7)),
                    KeyCode::Down => self.change(|d| d.add_days(7)),
                    KeyCode::PageUp => self.change(|d| d.add_months(-1)),
                    KeyCode::PageDown => self.change(|d| d.add_months(1)),
                    KeyCode::Home => self.change(Date::first_of_month),
                    KeyCode::End => self.change(Date::last_of_month),
                    _ => return Used::No,
                }
            }
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(button) if mouse.row >= top && mouse.column < WIDTH => {
                    let selected = self.source.get_untracked();
                    let at = (mouse.row - top) as u32 * 7 + mouse.column as u32 / 3;
                    match self.options.date_at(selected, at) {
                        // A day outside the range is used up: nothing
                        // selects it, and no menu speaks for the old date.
                        Some(date) if !self.options.allows(date) => return Used::Yes,
                        Some(date) => {
                            self.source.set(date);
                            // Another button leaves the press to the node's
                            // own handler, as a table does.
                            if button != Button::Left {
                                return Used::No;
                            }
                        }
                        // Not a day: another button is used up, so no
                        // menu speaks for the old date.
                        None if button != Button::Left => return Used::Yes,
                        None => return Used::No,
                    }
                }
                MouseKind::ScrollUp => self.change(|d| d.add_months(-1)),
                MouseKind::ScrollDown => self.change(|d| d.add_months(1)),
                _ => return Used::No,
            },
            _ => return Used::No,
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}

impl Widget for Month {
    fn name(&self) -> &'static str {
        "month"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::Grid
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => WIDTH,
            Axis::Vertical => HEIGHT,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let shown = (self.shown)();
        let options = (self.options)();
        draw_month(cx, canvas, shown, None, &options);
    }
}

/// How a [`year_view`] lays out and marks its months: each month's
/// [`CalendarOptions`] (titled by name only, as the year heads them), how
/// many across (as many as fit by default), the gaps between them, and
/// the year's heading.
#[derive(Clone, Debug, PartialEq)]
#[must_use = "options do nothing until they are passed to what takes them"]
pub struct YearOptions {
    pub(crate) calendar: CalendarOptions,
    pub(crate) across: Option<u16>,
    pub(crate) gap: (u16, u16),
    pub(crate) heading: bool,
    pub(crate) heading_style: Option<String>,
}

impl Default for YearOptions {
    fn default() -> YearOptions {
        YearOptions {
            calendar: CalendarOptions::new().title(MonthTitle::Name),
            across: None,
            gap: (2, 1),
            heading: true,
            heading_style: None,
        }
    }
}

impl YearOptions {
    /// As many months across as fit, two columns and a row apart, each
    /// titled by its name, under the year.
    pub fn new() -> YearOptions {
        YearOptions::default()
    }

    /// Each month's options: today, events, the first weekday, styles
    /// (their [`title`](CalendarOptions::title) included).
    pub fn calendar(mut self, options: CalendarOptions) -> YearOptions {
        self.calendar = options;
        self
    }

    /// Mark dates: `style(date)` gives a style for the dates it marks (a
    /// task's due date, a holiday), as [`CalendarOptions::events`].
    pub fn marks<S: Into<String>>(
        mut self,
        style: impl Fn(Date) -> Option<S> + 'static,
    ) -> YearOptions {
        self.calendar = self.calendar.events(style);
        self
    }

    /// Mark `today` in the theme's `calendar.today` style.
    pub fn today(mut self, today: Date) -> YearOptions {
        self.calendar.today = Some(today);
        self
    }

    /// The day the weeks start on.
    pub fn first_weekday(mut self, day: Weekday) -> YearOptions {
        self.calendar.first_weekday = day;
        self
    }

    /// Always `months` across (1 to 12), rather than as many as fit.
    pub fn across(mut self, months: u16) -> YearOptions {
        self.across = Some(months.clamp(1, 12));
        self
    }

    /// `columns` between months side by side, `rows` between rows of
    /// them.
    pub fn gap(mut self, columns: u16, rows: u16) -> YearOptions {
        self.gap = (columns, rows);
        self
    }

    /// Show the year above the months (on by default), in `style` (the
    /// theme's `calendar.title`, bold, by default).
    pub fn heading(mut self, on: bool) -> YearOptions {
        self.heading = on;
        self
    }

    /// The heading's style.
    pub fn heading_style(mut self, style: impl Into<String>) -> YearOptions {
        self.heading_style = Some(style.into());
        self
    }

    /// How many months go across `width`.
    fn across_in(&self, width: u16) -> u16 {
        self.across.unwrap_or_else(|| {
            (width.saturating_add(self.gap.0) / (WIDTH + self.gap.0)).clamp(1, 12)
        })
    }

    /// The rows above the months.
    fn top(&self) -> u16 {
        if self.heading {
            2
        } else {
            0
        }
    }

    /// The height of the year `across` months across.
    fn height(&self, across: u16) -> u16 {
        let rows = 12u16.div_ceil(across);
        self.top() + rows * HEIGHT + (rows - 1) * self.gap.1
    }
}

/// A year of months that only show, as many across as fit, centred, under
/// the year: taskwarrior-tui's calendar, with its marked dates
/// ([`YearOptions::marks`]). `year` may read signals; so may the
/// [`year_view_with`] options. It takes no focus and no keys.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{year_view, Date, YearOptions};
///
/// let options = YearOptions::new().marks(|d: Date| (d == Date::new(2026, 1, 9)).then_some("red"));
/// let app = App::new(move || year_view(|| 2026, options.clone()));
/// let screen = app.render_with(&[], 64, 40).unwrap();
/// assert_eq!(screen[0].trim(), "2026");
/// assert_eq!(
///     screen[2].trim_end(),
///     "      January               February               March"
/// );
/// assert_eq!(screen[4].trim_end(), "          1  2  3  4                     1                     1");
/// ```
pub fn year_view(year: impl Fn() -> i32 + 'static, options: YearOptions) -> Node {
    let options = Rc::new(options);
    year_view_with(year, move || (*options).clone())
}

/// A [`year_view`] whose options `options` gives, read again whenever a
/// signal it reads changes.
pub fn year_view_with(
    year: impl Fn() -> i32 + 'static,
    options: impl Fn() -> YearOptions + 'static,
) -> Node {
    widget(Year {
        year: Box::new(year),
        options: Box::new(options),
    })
}

struct Year {
    year: Box<dyn Fn() -> i32>,
    options: Box<dyn Fn() -> YearOptions>,
}

impl Widget for Year {
    fn name(&self) -> &'static str {
        "year_view"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::Grid
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        let options = (self.options)();
        match axis {
            // Three across, unless it says.
            Axis::Horizontal => {
                let across = options.across.unwrap_or(3);
                across * WIDTH + (across - 1) * options.gap.0
            }
            Axis::Vertical => options.height(options.across_in(width)),
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let year = (self.year)();
        let options = (self.options)();
        let width = canvas.width();
        let across = options.across_in(width);
        let used = across * WIDTH + (across - 1) * options.gap.0;
        let left = width.saturating_sub(used) / 2;
        if options.heading {
            let style = match &options.heading_style {
                Some(spec) => named(cx, spec),
                None => cx.style("calendar.title", "bold"),
            };
            let heading = year.to_string();
            let x = width.saturating_sub(heading.len() as u16) / 2;
            canvas.print(x, 0, &heading, Some(&style));
        }
        let origin = canvas.rect;
        for month in 0..12u16 {
            let (column, row) = (month % across, month / across);
            let x = left + column * (WIDTH + options.gap.0);
            let y = options.top() + row * (HEIGHT + options.gap.1);
            let rect = crate::screen::Rect::new(
                origin.x.saturating_add(x),
                origin.y.saturating_add(y),
                WIDTH,
                HEIGHT,
            )
            .intersection(origin);
            if rect.is_empty() {
                continue;
            }
            let mut part = Canvas {
                screen: &mut *canvas.screen,
                rect,
                written: &mut *canvas.written,
            };
            let shown = Date::new(year, month as u32 + 1, 1);
            draw_month(cx, &mut part, shown, None, &options.calendar);
        }
    }
}

/// The days from `from` (which may read signals) for `days` days, each
/// with what `items` gives for it (console markup, one line each) under a
/// heading (`Monday 19 October 2026`, bold);
/// days with nothing are left out, and with nothing at all it says so,
/// dimmed. An agenda beside a [`calendar`] or a [`year_view`], as
/// calcurse's and khal's.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{agenda, Date};
///
/// let app = App::new(|| {
///     agenda(|| Date::new(2026, 10, 19), 7, |day: Date| match day.day {
///         19 => vec!["Pay the rent".into()],
///         21 => vec!["Dentist, 9:00".into(), "Call Ada".into()],
///         _ => vec![],
///     })
/// });
/// let screen = app.render_with(&[], 30, 6).unwrap();
/// assert_eq!(screen[0].trim_end(), "Monday 19 October 2026");
/// assert_eq!(screen[1].trim_end(), "  Pay the rent");
/// assert_eq!(screen[2].trim_end(), "Wednesday 21 October 2026");
/// assert_eq!(screen[4].trim_end(), "  Call Ada");
/// ```
pub fn agenda(
    from: impl Fn() -> Date + 'static,
    days: u32,
    items: impl Fn(Date) -> Vec<String> + 'static,
) -> Node {
    crate::node::text(move || {
        let start = from();
        let mut lines = Vec::new();
        for n in 0..days {
            let day = start.add_days(n as i64);
            let listed = items(day);
            if listed.is_empty() {
                continue;
            }
            lines.push(format!(
                "[bold]{} {} {} {}[/]",
                Weekday::of(day).name(),
                day.day,
                day.month_name(),
                day.year
            ));
            lines.extend(listed.into_iter().map(|item| format!("  {item}")));
        }
        if lines.is_empty() {
            lines.push("[dim]Nothing scheduled".into());
        }
        lines.join("\n")
    })
    .what("agenda")
}
