//! A month calendar with a selected day, within an optional range (#719),
//! with styled dates, the days of the months either side, a first weekday
//! of your choice, and a month that only shows (0.0.19 gap batch C7). The
//! date types are rs-rich-interact's, shared with its date pickers.

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
            .field("day_style", &self.day_style)
            .finish()
    }
}

impl PartialEq for CalendarOptions {
    /// Equal when every setting is, and both have the same events function
    /// (the same one, not one that gives the same styles) or neither has.
    fn eq(&self, other: &Self) -> bool {
        let same_styler = match (&self.styler, &other.styler) {
            (None, None) => true,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        };
        same_styler
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
        ((at % 7) as u16 * 3, 2 + (at / 7) as u16)
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

struct Calendar {
    source: DaySource,
    options: CalendarOptions,
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
    })
}

/// The calendar half of a date-and-time picker.
pub(crate) fn moment_calendar(source: DaySource, options: CalendarOptions) -> Node {
    widget(Calendar { source, options })
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
    widget(Month {
        shown: Box::new(shown),
        options,
    })
}

struct Month {
    shown: Box<dyn Fn() -> Date>,
    options: CalendarOptions,
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
fn draw_month(
    cx: &mut DrawCx,
    canvas: &mut Canvas,
    shown: Date,
    selected: Option<Date>,
    options: &CalendarOptions,
) {
    let title = format!("{} {}", shown.month_name(), shown.year);
    let x = WIDTH.saturating_sub(title.len() as u16) / 2;
    let title_style = match &options.month_style {
        Some(spec) => named(cx, spec),
        None => cx.style("calendar.title", "bold"),
    };
    canvas.print(x, 0, &title, Some(&title_style));
    let header = match &options.weekday_style {
        Some(spec) => named(cx, spec),
        None => cx.style("calendar.header", "dim"),
    };
    let names: Vec<&str> = (0..7)
        .map(|i| Weekday::ALL[((options.first_weekday.index() + i) % 7) as usize].short_name())
        .collect();
    canvas.print(0, 1, &names.join(" "), Some(&header));
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
        canvas.fill(0, 2, WIDTH, weeks as u16, Some(&back));
    }
    for at in 0..weeks * 7 {
        let Some(date) = options.date_at(shown, at) else {
            continue;
        };
        let (x, y) = ((at % 7) as u16 * 3, 2 + (at / 7) as u16);
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
        let selected = self.source.get();
        if self.options.empty() {
            // No day can be selected: why, where the weeks go.
            let title = format!("{} {}", selected.month_name(), selected.year);
            let x = WIDTH.saturating_sub(title.len() as u16) / 2;
            let title_style = match &self.options.month_style {
                Some(spec) => named(cx, spec),
                None => cx.style("calendar.title", "bold"),
            };
            canvas.print(x, 0, &title, Some(&title_style));
            let style = cx.theme().error_style("error");
            for (row, line) in (2..).zip(EMPTY) {
                canvas.print(0, row, line, Some(&style));
            }
            return;
        }
        draw_month(cx, canvas, selected, Some(selected), &self.options);
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
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
                MouseKind::Down(button) if mouse.row >= 2 && mouse.column < WIDTH => {
                    let selected = self.source.get_untracked();
                    let at = (mouse.row as u32 - 2) * 7 + mouse.column as u32 / 3;
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
        draw_month(cx, canvas, shown, None, &self.options);
    }
}
