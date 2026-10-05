//! KPI cards: a label, a value, its change and its trend.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment};

use super::{
    cells, has_colour, lines_to_segments, theme_style, truncate, Charset, Line, Sparkline, Status,
    ValueFormat,
};

/// The narrowest card that keeps its border.
const MIN_BORDERED: usize = 5;

/// A change, as written on the card.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Delta {
    Absolute(f64),
    Percent(f64),
}

impl Delta {
    fn value(self) -> f64 {
        match self {
            Delta::Absolute(v) | Delta::Percent(v) => v,
        }
    }
}

/// One key number: a label, the value, its change with a direction, an
/// optional sparkline and an optional [`Status`].
///
/// ```text
/// ╭───────────────────────╮
/// │ Requests         ✓ ok │
/// │ 12.4k/s               │
/// │ ▲ +8.49% vs last week │
/// │ ▁▂▂▄▆▅▃▆█▇▅▃          │
/// ╰───────────────────────╯
/// ```
///
/// - The delta is an arrow and a signed number: `▲ +8.5%`, `▼ -120`,
///   `= 0` (`^`, `v` and `=` in ASCII). It is styled `chart.delta.good` or
///   `chart.delta.bad` by whether the change is for the better
///   ([`higher_is_better`](Self::higher_is_better), on by default), and
///   the arrow and sign carry the direction without colour.
/// - The status is a symbol and a word (`✓ ok`, `! warning`,
///   `✗ critical`), right-aligned on the label's line.
/// - The card measures to its content, so cards sit side by side in a
///   `Columns` or a `Table` grid; [`width`](Self::width) fixes the width and
///   [`expand`](Self::expand) fills what it is given, so a row of cards
///   lines up. Every card with the same parts has the same height.
/// - Given less width the label is cut, then the other lines; the
///   sparkline resamples. Below 5 cells the border goes.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Charset, KpiCard, Status};
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let card = KpiCard::new("Errors", 42.0)
///     .previous(56.0)
///     .higher_is_better(false)
///     .caption("vs yesterday")
///     .status(Status::Warning);
/// assert_eq!(
///     console.render_export(&card),
///     concat!(
///         "╭─────────────────────╮\n",
///         "│ Errors    ! warning │\n",
///         "│ 42                  │\n",
///         "│ ▼ -25% vs yesterday │\n",
///         "╰─────────────────────╯\n",
///     )
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct KpiCard {
    label: String,
    value: f64,
    format: ValueFormat,
    unit: String,
    delta: Option<Delta>,
    caption: Option<String>,
    higher_is_better: bool,
    sparkline: Option<Sparkline>,
    status: Option<Status>,
    width: Option<usize>,
    expand: bool,
    border: bool,
    charset: Charset,
}

impl KpiCard {
    /// A card called `label` showing `value`.
    pub fn new(label: impl Into<String>, value: f64) -> Self {
        KpiCard {
            label: label.into(),
            value,
            format: ValueFormat::Compact,
            unit: String::new(),
            delta: None,
            caption: None,
            higher_is_better: true,
            sparkline: None,
            status: None,
            width: None,
            expand: false,
            border: true,
            charset: Charset::Auto,
        }
    }

    /// How the value and an absolute delta are written (default
    /// [`ValueFormat::Compact`]).
    pub fn format(mut self, format: ValueFormat) -> Self {
        self.format = format;
        self
    }

    /// Written right after the value, such as `%`, `/s` or ` ms`.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    /// The change as an amount, written in the value's format.
    pub fn delta(mut self, delta: f64) -> Self {
        self.delta = Some(Delta::Absolute(delta));
        self
    }

    /// The change as a percentage.
    pub fn delta_percent(mut self, percent: f64) -> Self {
        self.delta = Some(Delta::Percent(percent));
        self
    }

    /// The change from `previous` to the value, as a percentage (as an
    /// amount when `previous` is 0).
    pub fn previous(mut self, previous: f64) -> Self {
        let change = self.value - previous;
        self.delta = Some(if previous != 0.0 && previous.is_finite() {
            Delta::Percent(change / previous.abs() * 100.0)
        } else {
            Delta::Absolute(change)
        });
        self
    }

    /// Words after the delta, such as `vs last week`.
    pub fn caption(mut self, caption: impl Into<String>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    /// Whether a rise is good news (default on); off for errors, latency
    /// and costs.
    pub fn higher_is_better(mut self, higher: bool) -> Self {
        self.higher_is_better = higher;
        self
    }

    /// A sparkline under the delta.
    pub fn sparkline(mut self, sparkline: Sparkline) -> Self {
        self.sparkline = Some(sparkline);
        self
    }

    /// A sparkline of `values` under the delta.
    pub fn trend(self, values: impl IntoIterator<Item = f64>) -> Self {
        self.sparkline(Sparkline::new(values))
    }

    /// The status, right-aligned on the label's line.
    pub fn status(mut self, status: Status) -> Self {
        self.status = Some(status);
        self
    }

    /// Fix the card's width, border included.
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width.max(1));
        self
    }

    /// Fill the width given (default off: the content's width).
    pub fn expand(mut self, expand: bool) -> Self {
        self.expand = expand;
        self
    }

    /// Draw the border (default on).
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Glyphs to draw with: [`Charset::Ascii`] asks for ASCII; anything else
    /// draws box drawing and arrows (ASCII on an ASCII-only console). A
    /// sparkline keeps its own charset unless this asks for ASCII.
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    fn value_text(&self) -> String {
        let value = self.format.format(self.value);
        if self.value.is_finite() {
            format!("{value}{}", self.unit)
        } else {
            value
        }
    }

    fn delta_text(&self, ascii: bool) -> Option<String> {
        let delta = self.delta?;
        let v = delta.value();
        // NaN, or a change too large for an `f64`: flat, with no sign.
        let direction = v.is_finite().then(|| v.partial_cmp(&0.0)).flatten();
        let arrow = match (direction, ascii) {
            (Some(std::cmp::Ordering::Greater), false) => "▲",
            (Some(std::cmp::Ordering::Greater), true) => "^",
            (Some(std::cmp::Ordering::Less), false) => "▼",
            (Some(std::cmp::Ordering::Less), true) => "v",
            _ => "=",
        };
        let number = match delta {
            Delta::Absolute(v) => self.format.format(v),
            Delta::Percent(v) => {
                let text = ValueFormat::Compact.format(v);
                if v.is_finite() {
                    format!("{text}%")
                } else {
                    text
                }
            }
        };
        let sign = if v > 0.0 && v.is_finite() && number != "0" && number != "0%" {
            "+"
        } else {
            ""
        };
        Some(format!("{arrow} {sign}{number}"))
    }

    fn delta_key(&self) -> &'static str {
        let v = self.delta.map_or(0.0, Delta::value);
        if !v.is_finite() || v == 0.0 {
            "chart.delta.flat"
        } else if (v > 0.0) == self.higher_is_better {
            "chart.delta.good"
        } else {
            "chart.delta.bad"
        }
    }

    fn charsets(&self, console: &Console, options: &ConsoleOptions) -> (bool, Option<Charset>) {
        let ascii = self.charset.resolve(console, options, Charset::Blocks) == Charset::Ascii;
        let spark = self.sparkline.as_ref().map(|s| {
            if ascii {
                Charset::Ascii
            } else {
                s.resolved_charset(console, options)
            }
        });
        (ascii, spark)
    }

    /// The content's natural width, border not included.
    fn inner_width(&self, ascii: bool, spark: Option<Charset>) -> usize {
        let header = cells(&self.label)
            + self.status.map_or(0, |s| {
                cells(&s.text(ascii)) + usize::from(!self.label.is_empty())
            });
        let delta = match (self.delta_text(ascii), &self.caption) {
            (Some(d), Some(c)) => cells(&d) + 1 + cells(c),
            (Some(d), None) => cells(&d),
            (None, Some(c)) => cells(c),
            (None, None) => 0,
        };
        let spark = match (&self.sparkline, spark) {
            (Some(s), Some(charset)) => s.natural_width(charset),
            _ => 0,
        };
        header
            .max(cells(&self.value_text()))
            .max(delta)
            .max(spark)
            .max(1)
    }

    fn natural_width(&self, ascii: bool, spark: Option<Charset>) -> usize {
        self.inner_width(ascii, spark) + if self.border { 4 } else { 0 }
    }

    fn lines(&self, console: &Console, options: &ConsoleOptions) -> Vec<Line> {
        let given = options.max_width;
        let (ascii, spark) = self.charsets(console, options);
        let colour = has_colour(console);
        let total = if self.expand {
            given
        } else {
            self.width
                .unwrap_or_else(|| self.natural_width(ascii, spark))
                .min(given)
        };
        let border = self.border && total >= MIN_BORDERED;
        let inner = if border { total - 4 } else { total };
        let style = |key: &str| colour.then(|| theme_style(console, key));

        let mut content = Vec::new();
        // The label, and the status on the right.
        let mut header = Line::new();
        let status = self.status.map(|s| truncate(&s.text(ascii), inner, ascii));
        let status_w = status.as_deref().map_or(0, cells);
        let label_room = inner.saturating_sub(status_w + usize::from(status_w > 0));
        let label = truncate(&self.label, label_room, ascii);
        header.push(&label, style("chart.kpi.label"));
        if let (Some(text), Some(s)) = (&status, self.status) {
            header.pad(inner - cells(&label) - status_w);
            header.push(text, style(s.key()));
        }
        content.push(header);
        let mut value = Line::new();
        value.push(
            &truncate(&self.value_text(), inner, ascii),
            style("chart.kpi.value"),
        );
        content.push(value);
        let delta = self.delta_text(ascii);
        if delta.is_some() || self.caption.is_some() {
            let mut line = Line::new();
            if let Some(d) = &delta {
                line.push(&truncate(d, inner, ascii), style(self.delta_key()));
            }
            if let Some(caption) = &self.caption {
                let room = inner.saturating_sub(line.width() + usize::from(line.width() > 0));
                if room > 0 {
                    if line.width() > 0 {
                        line.pad(1);
                    }
                    line.push(&truncate(caption, room, ascii), style("chart.label"));
                }
            }
            content.push(line);
        }
        if let (Some(s), Some(charset)) = (&self.sparkline, spark) {
            content.push(s.line(console, charset, inner));
        }

        if !border {
            for line in &mut content {
                line.crop(inner);
                let fill = inner - line.width();
                line.pad(fill);
            }
            return content;
        }
        let edge = style("chart.kpi.border");
        let (tl, tr, bl, br, h, v) = if ascii {
            ('+', '+', '+', '+', '-', '|')
        } else {
            ('╭', '╮', '╰', '╯', '─', '│')
        };
        let rule = |l: char, r: char| {
            let mut line = Line::new();
            line.push(
                &format!("{l}{}{r}", h.to_string().repeat(total - 2)),
                edge.clone(),
            );
            line
        };
        let mut out = vec![rule(tl, tr)];
        for mut body in content {
            body.crop(inner);
            let fill = inner - body.width();
            body.pad(fill);
            let mut line = Line::new();
            line.push(&format!("{v} "), edge.clone());
            line.append(body);
            line.push(&format!(" {v}"), edge.clone());
            out.push(line);
        }
        out.push(rule(bl, br));
        out
    }
}

impl Renderable for KpiCard {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        lines_to_segments(self.lines(console, options), options.max_width)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let (ascii, spark) = self.charsets(console, options);
        let max = if self.expand {
            options.max_width
        } else {
            self.width
                .unwrap_or_else(|| self.natural_width(ascii, spark))
        };
        let min = self.width.unwrap_or(if self.border { 8 } else { 4 });
        Measurement::new(min.min(max), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}
