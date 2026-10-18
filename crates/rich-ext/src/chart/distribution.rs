//! Distributions: the count, quantiles and mean of a set of values, as
//! numbers and a box-and-whisker strip on one line.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment};

use super::describe::count;
use super::{
    cells, has_colour, lines_to_segments, theme_style, truncate, user_style, Charset, Line, Scale,
    ValueFormat,
};

/// Strip cells when [`Distribution::strip_width`] is not set.
const DEFAULT_STRIP: usize = 20;
/// The narrowest strip drawn before the numbers and the label give way.
const MIN_STRIP: usize = 8;

/// The numbers that sum up a set of values: how many, the quantiles and
/// the mean. NaN and infinite values are left out and counted in
/// [`ignored`](Self::ignored).
///
/// Quantiles are numpy's default (`method="linear"`, Hyndman and Fan's
/// type 7): for `n` sorted values, the `q` quantile is at position
/// `h = (n - 1) q`, interpolated linearly between the values either side
/// of it. One value is every quantile; no values leave every number NaN.
///
/// ```
/// use rich_ext::chart::Summary;
///
/// let s = Summary::of([1.0, 2.0, 3.0, 4.0, f64::NAN]);
/// assert_eq!((s.count, s.ignored), (4, 1));
/// assert_eq!((s.min, s.p25, s.p50, s.p75, s.max), (1.0, 1.75, 2.5, 3.25, 4.0));
/// assert_eq!(s.p95, 3.8499999999999996);
/// assert_eq!(s.mean, 2.5);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    /// How many finite values.
    pub count: usize,
    /// How many values were NaN or infinite, and left out.
    pub ignored: usize,
    pub min: f64,
    /// The first quartile.
    pub p25: f64,
    /// The median.
    pub p50: f64,
    /// The third quartile.
    pub p75: f64,
    /// The 95th percentile.
    pub p95: f64,
    pub max: f64,
    pub mean: f64,
}

impl Summary {
    /// The summary of `values`.
    pub fn of(values: impl IntoIterator<Item = f64>) -> Summary {
        let (sorted, ignored) = sorted_finite(values);
        Summary::of_sorted(&sorted, ignored)
    }

    fn of_sorted(sorted: &[f64], ignored: usize) -> Summary {
        let q = |p| quantile(sorted, p).unwrap_or(f64::NAN);
        // The mean of the halves, so a sum of large values cannot overflow.
        let mean = if sorted.is_empty() {
            f64::NAN
        } else {
            let n = sorted.len() as f64;
            let sum: f64 = sorted.iter().map(|v| v / n).sum();
            sum.clamp(sorted[0], sorted[sorted.len() - 1])
        };
        Summary {
            count: sorted.len(),
            ignored,
            min: q(0.0),
            p25: q(0.25),
            p50: q(0.5),
            p75: q(0.75),
            p95: q(0.95),
            max: q(1.0),
            mean,
        }
    }

    /// Whether there were no finite values.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// The finite values of `values`, sorted, and how many were not finite.
fn sorted_finite(values: impl IntoIterator<Item = f64>) -> (Vec<f64>, usize) {
    let mut ignored = 0;
    let mut sorted: Vec<f64> = values
        .into_iter()
        .filter(|v| {
            let keep = v.is_finite();
            ignored += usize::from(!keep);
            keep
        })
        .collect();
    sorted.sort_by(f64::total_cmp);
    (sorted, ignored)
}

/// The `q` quantile (0 to 1, clamped) of `sorted` values, by linear
/// interpolation (numpy's default method; see [`Summary`]). `None` for no
/// values or a NaN `q`.
///
/// ```
/// use rich_ext::chart::quantile;
///
/// let sorted = [10.0, 20.0, 30.0, 40.0];
/// assert_eq!(quantile(&sorted, 0.5), Some(25.0));
/// assert_eq!(quantile(&sorted, 1.0 / 3.0), Some(20.0));
/// assert_eq!(quantile(&[7.0], 0.9), Some(7.0));
/// assert_eq!(quantile(&[], 0.5), None);
/// ```
pub fn quantile(sorted: &[f64], q: f64) -> Option<f64> {
    if sorted.is_empty() || q.is_nan() {
        return None;
    }
    let h = (sorted.len() - 1) as f64 * q.clamp(0.0, 1.0);
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    let (a, b) = (sorted[lo], sorted[hi]);
    let t = h - lo as f64;
    // `a + t (b - a)`, without overflowing when the two are far apart.
    let v = if (b - a).is_finite() {
        a + t * (b - a)
    } else {
        a * (1.0 - t) + b * t
    };
    Some(if t == 0.0 { a } else { v })
}

/// A set of values summed up on one line: an optional label, a
/// box-and-whisker strip, and the numbers.
///
/// ```text
/// latency ├────▒▒▒▒█▒▒▒▒▒──┼────┤ n 1,000 min 1 p25 4 p50 5 p75 8 p95 12 max 14 mean 6.1
/// ```
///
/// - **The strip** runs from the smallest value to the largest (or
///   [`range`](Self::range)): `├` and `┤` at the ends, `─` whiskers, a
///   shaded box `▒` from the first quartile to the third, the median `█`,
///   and the 95th percentile a `┼` on the whisker. In ASCII: `|`, `-`,
///   `=`, `#` and `+`. The glyphs carry it, so it reads without colour.
/// - **The numbers** are the count (`n`), `min`, `p25`, `p50` (the median),
///   `p75`, `p95`, `max` and `mean`, in [`format`](Self::format) with
///   [`unit`](Self::unit) after each value, then how many NaN and infinite
///   values were left out (`2 ignored`).
/// - Quantiles are interpolated linearly, numpy's default: see
///   [`Summary`].
/// - The strip is [`strip_width`](Self::strip_width) cells (20 by
///   default). Given less width it shrinks to 8 cells, then the numbers
///   shorten to `min`, `p50` and `max`, then go, then the label is cut.
/// - Several on one scale, their columns aligned: [`DistributionSet`].
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Charset, Distribution};
///
/// let console = Console::builder().width(80).color_system(None).build();
/// let values = (1..=20).map(f64::from);
/// let d = Distribution::new(values).label("x").strip_width(12);
/// assert_eq!(
///     console.render_export(&d),
///     "x ├──▒▒▒█▒▒▒─┼┤ n 20 min 1 p25 5.75 p50 10.5 p75 15.25 p95 19.05 max 20 mean 10.5\n"
/// );
/// assert_eq!(
///     console.render_export(&d.clone().charset(Charset::Ascii).numbers(false)),
///     "x |--===#===-+|\n"
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Distribution {
    label: String,
    sorted: Vec<f64>,
    summary: Summary,
    range: Option<(f64, f64)>,
    strip_width: Option<usize>,
    format: ValueFormat,
    unit: String,
    charset: Charset,
    style: Option<String>,
    numbers: bool,
}

impl Distribution {
    /// The distribution of `values`; NaN and infinite values are left out
    /// and counted.
    pub fn new(values: impl IntoIterator<Item = f64>) -> Self {
        let (sorted, ignored) = sorted_finite(values);
        let summary = Summary::of_sorted(&sorted, ignored);
        Distribution {
            label: String::new(),
            sorted,
            summary,
            range: None,
            strip_width: None,
            format: ValueFormat::Compact,
            unit: String::new(),
            charset: Charset::Auto,
            style: None,
            numbers: true,
        }
    }

    /// Written before the strip.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Fix the strip's scale (default: the smallest to the largest value).
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.range = Some((min, max));
        self
    }

    /// Cells for the strip (default 20, at least 1).
    pub fn strip_width(mut self, width: usize) -> Self {
        self.strip_width = Some(width.max(1));
        self
    }

    /// How the numbers are written (default [`ValueFormat::Compact`]).
    pub fn format(mut self, format: ValueFormat) -> Self {
        self.format = format;
        self
    }

    /// Written right after every value but the count, such as `ms`.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    /// Glyphs to draw with: [`Charset::Ascii`] asks for ASCII; anything
    /// else draws blocks and box drawing (ASCII on an ASCII-only console).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// The box's and the median's style: a theme key or a definition
    /// (default `chart.bar`). The whiskers are `chart.axis`.
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// Write the numbers after the strip (default on).
    pub fn numbers(mut self, show: bool) -> Self {
        self.numbers = show;
        self
    }

    /// The numbers.
    pub fn summary(&self) -> Summary {
        self.summary
    }

    /// The label.
    pub fn label_text(&self) -> &str {
        &self.label
    }

    /// The finite values, sorted.
    pub fn values(&self) -> &[f64] {
        &self.sorted
    }

    /// The `q` quantile (0 to 1) of the values, as [`quantile`] gives it.
    pub fn quantile(&self, q: f64) -> Option<f64> {
        quantile(&self.sorted, q)
    }

    /// The scale its strip is drawn on, alone.
    fn own_scale(&self) -> Scale {
        match self.range {
            Some((a, b)) => Scale::new(a, b),
            None => Scale::from_values([self.summary.min, self.summary.max]),
        }
    }

    /// The range of its values, or the fixed one.
    fn extent(&self) -> Option<(f64, f64)> {
        match self.range {
            Some((a, b)) if a.is_finite() && b.is_finite() => Some((a.min(b), a.max(b))),
            _ if self.summary.is_empty() => None,
            _ => Some((self.summary.min, self.summary.max)),
        }
    }

    /// `value` in the format, with the unit.
    fn text(&self, value: f64) -> String {
        let text = self.format.format(value);
        if value.is_finite() {
            format!("{text}{}", self.unit)
        } else {
            text
        }
    }

    /// The numbers as `(key, value)` pairs: all of them, or the short set.
    fn numbers_of(&self, short: bool) -> Vec<(&'static str, String)> {
        let s = &self.summary;
        if s.is_empty() {
            return vec![("n", "0".to_string())];
        }
        if short {
            return vec![
                ("min", self.text(s.min)),
                ("p50", self.text(s.p50)),
                ("max", self.text(s.max)),
            ];
        }
        vec![
            ("n", count(s.count)),
            ("min", self.text(s.min)),
            ("p25", self.text(s.p25)),
            ("p50", self.text(s.p50)),
            ("p75", self.text(s.p75)),
            ("p95", self.text(s.p95)),
            ("max", self.text(s.max)),
            ("mean", self.text(s.mean)),
        ]
    }

    /// The strip, `width` cells on `scale`.
    fn strip(&self, console: &Console, scale: &Scale, width: usize, ascii: bool) -> Line {
        let mut line = Line::new();
        let s = &self.summary;
        if s.is_empty() {
            line.push(&truncate("no data", width, ascii), None);
            line.pad(width - line.width());
            return line;
        }
        let colour = has_colour(console);
        let whisker_style = colour.then(|| theme_style(console, "chart.axis"));
        let box_style = colour.then(|| match &self.style {
            Some(spec) => user_style(console, spec),
            None => theme_style(console, "chart.bar"),
        });
        // Each cell is an equal share of the scale; the top lands in the
        // last.
        let at = |v: f64| {
            let t = scale.normalize(v).unwrap_or(0.0);
            ((t * width as f64).floor() as usize).min(width - 1)
        };
        let (lo, p25, p50, p75, p95, hi) =
            (at(s.min), at(s.p25), at(s.p50), at(s.p75), at(s.p95), at(s.max));
        let [end_l, end_r, whisker, tick, shade, median] = if ascii {
            ['|', '|', '-', '+', '=', '#']
        } else {
            ['├', '┤', '─', '┼', '▒', '█']
        };
        // Each cell's glyph and whether it is part of the box.
        let mut glyphs: Vec<(char, bool)> = vec![(' ', false); width];
        for cell in glyphs.iter_mut().take(hi + 1).skip(lo) {
            *cell = (whisker, false);
        }
        if p95 > p75 {
            glyphs[p95] = (tick, false);
        }
        glyphs[lo] = (end_l, false);
        glyphs[hi] = (end_r, false);
        for cell in glyphs.iter_mut().take(p75 + 1).skip(p25) {
            *cell = (shade, true);
        }
        glyphs[p50] = (median, true);
        for (glyph, boxed) in glyphs {
            let style = if glyph == ' ' {
                None
            } else if boxed {
                box_style.clone()
            } else {
                whisker_style.clone()
            };
            line.push(&glyph.to_string(), style);
        }
        line
    }
}

/// How rows of distributions fit a width.
struct Fit {
    label: usize,
    strip: usize,
    /// `None`: no numbers; `Some(short)`: all of them, or the short set.
    numbers: Option<bool>,
}

/// The widths of `rows`' number columns: each key's widest value.
fn number_widths(rows: &[&Distribution], short: bool) -> Vec<usize> {
    let mut widths: Vec<usize> = Vec::new();
    for row in rows {
        for (i, (_, value)) in row.numbers_of(short).iter().enumerate() {
            if widths.len() <= i {
                widths.push(0);
            }
            widths[i] = widths[i].max(cells(value));
        }
    }
    widths
}

/// One row's numbers, each value right-aligned in its column.
fn numbers_line(console: &Console, row: &Distribution, short: bool, widths: &[usize]) -> Line {
    let colour = has_colour(console);
    let key_style = colour.then(|| theme_style(console, "chart.label"));
    let value_style = colour.then(|| theme_style(console, "chart.value"));
    let mut line = Line::new();
    for (i, (key, value)) in row.numbers_of(short).into_iter().enumerate() {
        if i > 0 {
            line.pad(1);
        }
        line.push(key, key_style.clone());
        line.pad(1 + widths.get(i).copied().unwrap_or(0).saturating_sub(cells(&value)));
        line.push(&value, value_style.clone());
    }
    if !short && row.summary.ignored > 0 {
        line.pad(1);
        line.push(
            &format!("{} ignored", count(row.summary.ignored)),
            key_style.clone(),
        );
    }
    line
}

/// The cells `rows`' numbers take: the widest row.
fn numbers_width(rows: &[&Distribution], short: bool) -> usize {
    let widths = number_widths(rows, short);
    let console = Console::builder().color_system(None).build();
    rows.iter()
        .map(|r| numbers_line(&console, r, short, &widths).width())
        .max()
        .unwrap_or(0)
}

/// Fit `rows` into `width`: see [`Distribution`].
fn fit(rows: &[&Distribution], width: usize, strip: usize, numbers: bool) -> Fit {
    let label = rows.iter().map(|r| cells(&r.label)).max().unwrap_or(0);
    let gap = |w: usize| w + usize::from(w > 0);
    let room = width.saturating_sub(gap(label));
    let mut options = Vec::new();
    if numbers {
        options.push(Some(false));
        options.push(Some(true));
    }
    options.push(None);
    for choice in options {
        let extra = choice.map_or(0, |short| 1 + numbers_width(rows, short));
        let take = strip.min(room.saturating_sub(extra));
        if take >= MIN_STRIP.min(strip) && take > 0 {
            return Fit {
                label,
                strip: take,
                numbers: choice,
            };
        }
    }
    // Cut the label, keeping the narrowest strip.
    let keep = MIN_STRIP.min(strip).min(width.max(1));
    let label = label.min(width.saturating_sub(keep + 1));
    Fit {
        label,
        strip: width.saturating_sub(gap(label)).clamp(1, keep.max(1)),
        numbers: None,
    }
}

/// `rows` laid out at `fit`, each on its own scale from `scale_of`.
fn lines_of(
    console: &Console,
    rows: &[&Distribution],
    fit: &Fit,
    ascii: bool,
    scale_of: &dyn Fn(&Distribution) -> Scale,
) -> Vec<Line> {
    let colour = has_colour(console);
    let label_style = colour.then(|| theme_style(console, "chart.label"));
    let widths = fit.numbers.map(|short| number_widths(rows, short));
    rows.iter()
        .map(|row| {
            let mut line = Line::new();
            if fit.label > 0 {
                let label = truncate(&row.label, fit.label, ascii);
                line.push(&label, label_style.clone());
                line.pad(fit.label - cells(&label) + 1);
            }
            line.append(row.strip(console, &scale_of(row), fit.strip, ascii));
            if let (Some(short), Some(widths)) = (fit.numbers, &widths) {
                line.pad(1);
                line.append(numbers_line(console, row, short, widths));
            }
            line
        })
        .collect()
}

fn natural_width(rows: &[&Distribution], strip: usize, numbers: bool) -> usize {
    let label = rows.iter().map(|r| cells(&r.label)).max().unwrap_or(0);
    let numbers = if numbers {
        1 + numbers_width(rows, false)
    } else {
        0
    };
    label + usize::from(label > 0) + strip + numbers
}

impl Renderable for Distribution {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let ascii = self.charset.resolve(console, options, Charset::Blocks) == Charset::Ascii;
        let rows = [self];
        let strip = self.strip_width.unwrap_or(DEFAULT_STRIP);
        let fit = fit(&rows, width, strip, self.numbers);
        let lines = lines_of(console, &rows, &fit, ascii, &|d| d.own_scale());
        lines_to_segments(lines, width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let strip = self.strip_width.unwrap_or(DEFAULT_STRIP);
        let max = natural_width(&[self], strip, self.numbers);
        Measurement::new(MIN_STRIP.min(max), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// `Distribution latency: 1,000 values, min 1, p25 4, median 5, p75 8, p95
/// 12, max 14, mean 6.1`, with how many were ignored.
impl crate::a11y::AccessibleText for Distribution {
    fn accessible_text(&self, _width: usize) -> String {
        let what = if self.label.is_empty() {
            "Distribution".to_string()
        } else {
            format!("Distribution {}", self.label)
        };
        let s = &self.summary;
        let mut parts = vec![super::describe::counted(s.count, "value", "values")];
        if s.ignored > 0 {
            parts.push(format!("{} ignored", count(s.ignored)));
        }
        if !s.is_empty() {
            for (key, value) in [
                ("min", s.min),
                ("p25", s.p25),
                ("median", s.p50),
                ("p75", s.p75),
                ("p95", s.p95),
                ("max", s.max),
                ("mean", s.mean),
            ] {
                parts.push(format!("{key} {}", self.text(value)));
            }
        }
        format!("{what}: {}", parts.join(", "))
    }
}

/// Distributions stacked on one scale, so they compare at a glance: their
/// labels, strips and numbers in aligned columns, and under the strips the
/// scale's two ends.
///
/// ```text
/// api ├──▒▒█▒▒▒──┼──┤             n 100 min 12 p25 20 …
/// web       ├─▒▒█▒▒▒▒──┼──────┤   n 100 min 31 p25 40 …
///     12                       90
/// ```
///
/// - The scale runs from the smallest value of any of them to the largest
///   (or [`range`](Self::range)). [`shared_scale(false)`](Self::shared_scale)
///   gives each its own, and leaves the scale line out.
/// - [`strip_width`](Self::strip_width), [`numbers`](Self::numbers) and
///   [`charset`](Self::charset) apply to every row; each distribution keeps
///   its label, style, format and unit.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Charset, Distribution, DistributionSet};
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let set = DistributionSet::new()
///     .push(Distribution::new([0.0, 2.0, 4.0, 6.0, 8.0]).label("a"))
///     .push(Distribution::new([4.0, 5.0, 6.0]).label("b"))
///     .strip_width(9)
///     .numbers(false)
///     .charset(Charset::Ascii);
/// assert_eq!(
///     console.render_export(&set),
///     "a |-==#==-+|\n\
///      b     ##=+| \n  0        8\n"
/// );
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DistributionSet {
    items: Vec<Distribution>,
    range: Option<(f64, f64)>,
    strip_width: Option<usize>,
    numbers: Option<bool>,
    charset: Charset,
    shared: Option<bool>,
}

impl DistributionSet {
    /// An empty set; add rows with [`push`](Self::push).
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a distribution as the next row.
    pub fn push(mut self, distribution: Distribution) -> Self {
        self.items.push(distribution);
        self
    }

    /// Add the distribution of `values`, labelled.
    pub fn distribution(
        self,
        label: impl Into<String>,
        values: impl IntoIterator<Item = f64>,
    ) -> Self {
        self.push(Distribution::new(values).label(label))
    }

    /// Fix the shared scale (default: the smallest to the largest value of
    /// any row).
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.range = Some((min, max));
        self
    }

    /// Cells for every strip (default 20).
    pub fn strip_width(mut self, width: usize) -> Self {
        self.strip_width = Some(width.max(1));
        self
    }

    /// Write the numbers after every strip (default on).
    pub fn numbers(mut self, show: bool) -> Self {
        self.numbers = Some(show);
        self
    }

    /// Glyphs to draw every row with (default [`Charset::Auto`]).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// Draw every row on one scale, with a scale line under them (default
    /// on).
    pub fn shared_scale(mut self, shared: bool) -> Self {
        self.shared = Some(shared);
        self
    }

    /// The rows.
    pub fn distributions(&self) -> &[Distribution] {
        &self.items
    }

    fn is_shared(&self) -> bool {
        self.shared.unwrap_or(true)
    }

    /// The shared scale: the fixed range, else every row's range.
    pub fn scale(&self) -> Scale {
        match self.range {
            Some((a, b)) => Scale::new(a, b),
            None => {
                let ends = self
                    .items
                    .iter()
                    .filter_map(Distribution::extent)
                    .flat_map(|(lo, hi)| [lo, hi]);
                Scale::from_values(ends)
            }
        }
    }

    fn strip(&self) -> usize {
        self.strip_width.unwrap_or(DEFAULT_STRIP)
    }

    fn show_numbers(&self) -> bool {
        self.numbers.unwrap_or(true)
    }

    /// The scale's ends under the strips.
    fn scale_line(&self, console: &Console, fit: &Fit, scale: &Scale) -> Line {
        let format = self.items.first().map_or(ValueFormat::Compact, |d| d.format);
        let unit = self.items.first().map_or("", |d| d.unit.as_str());
        let lo = format!("{}{unit}", format.format(scale.min()));
        let hi = format!("{}{unit}", format.format(scale.max()));
        let style = has_colour(console).then(|| theme_style(console, "chart.axis"));
        let mut line = Line::new();
        if fit.label > 0 {
            line.pad(fit.label + 1);
        }
        let (lo_w, hi_w) = (cells(&lo), cells(&hi));
        if lo_w + 1 + hi_w <= fit.strip {
            line.push(&lo, style.clone());
            line.pad(fit.strip - lo_w - hi_w);
            line.push(&hi, style);
        } else if lo_w <= fit.strip {
            line.push(&lo, style);
        }
        line
    }
}

impl Renderable for DistributionSet {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let ascii = self.charset.resolve(console, options, Charset::Blocks) == Charset::Ascii;
        if self.items.is_empty() {
            let mut line = Line::new();
            line.push(&truncate("no data", width, ascii), None);
            return lines_to_segments(vec![line], width);
        }
        let rows: Vec<&Distribution> = self.items.iter().collect();
        let fit = fit(&rows, width, self.strip(), self.show_numbers());
        let shared = self.scale();
        let mut lines = if self.is_shared() {
            lines_of(console, &rows, &fit, ascii, &|_| shared)
        } else {
            lines_of(console, &rows, &fit, ascii, &|d| d.own_scale())
        };
        if self.is_shared() {
            lines.push(self.scale_line(console, &fit, &shared));
        }
        lines_to_segments(lines, width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        if self.items.is_empty() {
            return Measurement::new(7, 7).with_maximum(options.max_width);
        }
        let rows: Vec<&Distribution> = self.items.iter().collect();
        let max = natural_width(&rows, self.strip(), self.show_numbers());
        Measurement::new(MIN_STRIP.min(max), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// `Distributions, 3 on a shared scale from 0 to 120`, then each row's
/// text.
impl crate::a11y::AccessibleText for DistributionSet {
    fn accessible_text(&self, width: usize) -> String {
        if self.items.is_empty() {
            return "Distributions, none".to_string();
        }
        let mut head = format!("Distributions, {}", count(self.items.len()));
        if self.is_shared() {
            let scale = self.scale();
            let format = self.items[0].format;
            head.push_str(&format!(
                " on a shared scale {}",
                super::describe::range(scale.min(), scale.max(), format)
            ));
        }
        let mut out = vec![head];
        out.extend(self.items.iter().map(|d| d.accessible_text(width)));
        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a11y::AccessibleText;
    use rich::Style;

    /// The style of the first segment holding `glyph`.
    fn style_of(line: &[Segment], glyph: &str) -> Option<Style> {
        line.iter()
            .find(|s| s.text.contains(glyph))
            .and_then(|s| s.style.clone())
    }

    fn plain(width: usize) -> Console {
        Console::builder().width(width).color_system(None).build()
    }

    fn rows(width: usize, r: &dyn Renderable) -> Vec<String> {
        plain(width)
            .render_export(r)
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn zz_debug() {
        let d = Distribution::new((0..=100).map(f64::from))
            .label("ms")
            .strip_width(21);
        for w in [90, 70, 68, 60, 40, 24, 12, 10, 6, 3] {
            println!("{w}: {:?}", rows(w, &d));
        }
        let gaps = Distribution::new([1.0, f64::NAN, 3.0]).strip_width(8);
        println!("{:?}", rows(80, &gaps));
        println!("{:?}", rows(40, &Distribution::new([]).label("x").strip_width(8)));
        println!("{:?}", rows(40, &Distribution::new([5.0]).strip_width(9).numbers(false)));
        let values = (1..=20).map(f64::from);
        let d = Distribution::new(values).label("x").strip_width(12);
        println!("{:?}", rows(80, &d));
        let set = DistributionSet::new()
            .push(Distribution::new([0.0, 2.0, 4.0, 6.0, 8.0]).label("a"))
            .push(Distribution::new([4.0, 5.0, 6.0]).label("b"))
            .strip_width(9)
            .numbers(false)
            .charset(Charset::Ascii);
        println!("{:?}", rows(40, &set));
        let set = DistributionSet::new()
            .distribution("api", (10..=50).map(f64::from))
            .distribution("worker", (30..=90).map(f64::from))
            .strip_width(16);
        println!("{:?}", rows(40, &set.clone().shared_scale(false).numbers(false)));
    }

    #[test]
    fn quantiles_match_numpy_linear() {
        // numpy.quantile([1, 2, 3, 4, 5, 6, 7, 8, 9, 10], q)
        let sorted: Vec<f64> = (1..=10).map(f64::from).collect();
        let q = |p| quantile(&sorted, p).unwrap();
        assert_eq!(q(0.0), 1.0);
        assert_eq!(q(0.25), 3.25);
        assert_eq!(q(0.5), 5.5);
        assert_eq!(q(0.75), 7.75);
        assert_eq!(q(0.95), 9.549999999999999);
        assert_eq!(q(1.0), 10.0);
        // numpy.quantile([3, 1, 4, 1, 5, 9, 2, 6], [.25, .5, .75, .95])
        let s = Summary::of([3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0]);
        assert_eq!((s.p25, s.p50, s.p75), (1.75, 3.5, 5.25));
        assert!((s.p95 - 7.949999999999999).abs() < 1e-12, "{}", s.p95);
        assert_eq!(s.mean, 3.875);
        // Out-of-range and NaN `q`.
        assert_eq!(quantile(&sorted, 2.0), Some(10.0));
        assert_eq!(quantile(&sorted, -1.0), Some(1.0));
        assert_eq!(quantile(&sorted, f64::NAN), None);
    }

    #[test]
    fn edge_cases_empty_one_value_and_nan() {
        let empty = Summary::of([]);
        assert!(empty.is_empty());
        assert!(empty.min.is_nan() && empty.p50.is_nan() && empty.mean.is_nan());
        let one = Summary::of([7.0]);
        assert_eq!(
            (one.count, one.min, one.p25, one.p50, one.p95, one.max, one.mean),
            (1, 7.0, 7.0, 7.0, 7.0, 7.0, 7.0)
        );
        let nan = Summary::of([f64::NAN, 2.0, f64::INFINITY, 4.0, f64::NEG_INFINITY]);
        assert_eq!((nan.count, nan.ignored, nan.p50), (2, 3, 3.0));
        let only_nan = Summary::of([f64::NAN]);
        assert_eq!((only_nan.count, only_nan.ignored), (0, 1));
        // Huge values do not overflow the mean or the interpolation.
        let huge = Summary::of([f64::MAX, f64::MAX]);
        assert_eq!((huge.mean, huge.p50), (f64::MAX, f64::MAX));
        let wide = Summary::of([-f64::MAX, f64::MAX]);
        assert_eq!(wide.p50, 0.0);
    }

    #[test]
    fn a_distribution_draws_a_strip_and_its_numbers() {
        let d = Distribution::new((0..=100).map(f64::from))
            .label("ms")
            .strip_width(21);
        assert_eq!(
            rows(90, &d),
            vec!["ms ├────▒▒▒▒▒█▒▒▒▒▒───┼┤ n 101 min 0 p25 25 p50 50 p75 75 p95 95 max 100 mean 50"]
        );
        // Narrower: the strip shrinks, then the numbers shorten, then go.
        assert_eq!(
            rows(60, &d),
            vec!["ms ├▒▒█▒▒┼┤ n 101 min 0 p25 25 p50 50 p75 75 p95 95 max 100 mean 50"]
        );
        assert_eq!(rows(40, &d), vec!["ms ├────▒▒▒▒▒█▒▒▒▒▒───┼┤ min 0 p50 50 max 100"]);
        assert_eq!(rows(24, &d), vec!["ms ├────▒▒▒▒▒█▒▒▒▒▒───┼┤"]);
        assert_eq!(rows(10, &d), vec!["m ├▒▒█▒┼┤"]);
        // Ignored values are counted.
        let gaps = Distribution::new([1.0, f64::NAN, 3.0]).strip_width(8);
        assert_eq!(
            rows(80, &gaps),
            vec!["├──█───┤ n 2 min 1 p25 1.5 p50 2 p75 2.5 p95 2.9 max 3 mean 2 1 ignored"]
        );
        // No values: no strip, a count of 0.
        assert_eq!(
            rows(40, &Distribution::new([]).label("x").strip_width(8)),
            vec!["x no data  n 0"]
        );
        // One value: everything at the middle of its widened scale.
        assert_eq!(
            rows(40, &Distribution::new([5.0]).strip_width(9).numbers(false)),
            vec!["    █    "]
        );
    }

    #[test]
    fn ascii_and_colour() {
        let d = Distribution::new((0..=100).map(f64::from))
            .strip_width(11)
            .numbers(false);
        let ascii = Console::builder()
            .width(40)
            .color_system(None)
            .ascii_only(true)
            .build();
        assert_eq!(ascii.render_export(&d), "|--==#==-+|\n");
        let coloured = Console::builder()
            .width(40)
            .force_terminal(true)
            .color_system(Some(rich::ColorSystem::Truecolor))
            .no_color(false)
            .build();
        let lines = coloured.render_lines(&d, &coloured.options(), false);
        assert_eq!(style_of(&lines[0], "█"), Style::parse("cyan").ok());
        assert_eq!(style_of(&lines[0], "─"), Style::parse("bright_black").ok());
    }

    #[test]
    fn a_set_shares_its_scale_and_aligns_its_numbers() {
        let set = DistributionSet::new()
            .distribution("api", (10..=50).map(f64::from))
            .distribution("worker", (30..=90).map(f64::from))
            .strip_width(16);
        assert_eq!(
            rows(100, &set),
            vec![
                "api    ├──▒▒█▒▒─┼┤        n 41 min 10 p25 20 p50 30 p75 40 p95 48 max 50 mean 30",
                "worker      ├─▒▒▒█▒▒▒─┼┤  n 61 min 30 p25 45 p50 60 p75 75 p95 87 max 90 mean 60",
                "       10            90",
            ]
        );
        // Each on its own scale: no scale line.
        let own = set.clone().shared_scale(false).numbers(false);
        assert_eq!(
            rows(40, &own),
            vec!["api    ├───▒▒▒▒█▒▒▒──┼┤", "worker ├───▒▒▒▒█▒▒▒──┼┤"]
        );
        assert_eq!(
            set.accessible_text(80),
            "Distributions, 2 on a shared scale from 10 to 90\n\
             Distribution api: 41 values, min 10, p25 20, median 30, p75 40, p95 48, max 50, mean 30\n\
             Distribution worker: 61 values, min 30, p25 45, median 60, p75 75, p95 87, max 90, mean 60"
        );
    }

    #[test]
    fn accessible_text_reads_the_numbers() {
        let d = Distribution::new([1.0, 2.0, 3.0, 4.0, f64::NAN])
            .label("latency")
            .unit("ms");
        assert_eq!(
            d.accessible_text(80),
            "Distribution latency: 4 values, 1 ignored, min 1ms, p25 1.75ms, median 2.5ms, \
             p75 3.25ms, p95 3.85ms, max 4ms, mean 2.5ms"
        );
        assert_eq!(
            Distribution::new([f64::NAN]).accessible_text(80),
            "Distribution: 0 values, 1 ignored"
        );
    }
}
