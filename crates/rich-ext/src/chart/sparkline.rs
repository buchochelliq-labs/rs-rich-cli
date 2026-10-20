//! A chart of a series of values, one cell (or half a cell) a value, on one
//! row or several.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::{
    cells, has_colour, lines_to_segments, theme_style, user_style, Charset, Line, Scale,
    ValueFormat,
};
use crate::fidelity::ascii_text;

/// Heights for [`Charset::Blocks`], lowest first.
const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Heights for [`Charset::Ascii`], lowest first (the ramp `qa::bench` uses).
const ASCII: [char; 8] = ['_', '.', '-', ':', '=', '+', '*', '#'];
/// The tallest sparkline, in rows.
const MAX_ROWS: usize = u16::MAX as usize;

/// Which way a [`Sparkline`] runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    /// The first value on the left (the default).
    #[default]
    LeftToRight,
    /// The first value on the right, against the right edge of the width
    /// given, as ratatui's `RenderDirection::RightToLeft`: a series that
    /// grows at its front scrolls left.
    RightToLeft,
}

/// How finely a [`Sparkline`] in [`Charset::Blocks`] draws the top of a
/// bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Levels {
    /// Eighths of a cell, `▁▂▃▄▅▆▇█` (the default; ratatui's
    /// `NINE_LEVELS`).
    #[default]
    Nine,
    /// Nothing, a half or a whole cell, ` ▄█` (ratatui's `THREE_LEVELS`),
    /// for fonts without the eighths: the lowest eighth draws nothing,
    /// two to six eighths a half, seven and eight a whole cell.
    Three,
}

impl Levels {
    /// The glyph for a cell `eighths` full (1 to 8).
    fn glyph(self, eighths: usize) -> char {
        match self {
            Levels::Nine => BLOCKS[eighths.clamp(1, 8) - 1],
            Levels::Three => match eighths {
                0 | 1 => ' ',
                2..=6 => '▄',
                _ => '█',
            },
        }
    }
}

/// A chart of a series of values: each value is a column of cells whose
/// height shows where it falls between the smallest and largest value.
///
/// - [`Charset::Blocks`] (the default) draws one value per cell with
///   `▁▂▃▄▅▆▇█` ([`levels`](Self::levels) picks ratatui's three-level set
///   instead); [`Charset::Braille`] draws two values per cell, four
///   heights each; [`Charset::Ascii`] uses the ramp `_.-:=+*#`.
/// - It is one row tall, or [`rows`](Self::rows) rows, or the height it is
///   given (a `Layout` region, an intuiTUIve node) when that is not set.
///   Each row adds eight heights in blocks and four in Braille; the lowest
///   value shows at least the lowest glyph.
/// - NaN and infinite values, and the `None`s of
///   [`optional`](Self::optional), are gaps: a space, or
///   [`absent`](Self::absent)'s symbol down the whole column.
/// - It measures to one cell per value (half that in Braille). Given fewer
///   cells, it **resamples**: the values are split into as many equal
///   buckets as there are cells (two per cell in Braille) and each bucket
///   is drawn at the mean of its finite values.
/// - [`min_max`](Self::min_max) styles the lowest and highest cell and
///   writes `min 2 max 9` after the line; [`threshold`](Self::threshold)
///   styles the cells above it and writes `3 > 80`. The words are what make
///   them readable without colour, and they are the first thing left out
///   when the width is short. On several rows they go on the bottom one.
/// - [`direction`](Self::direction) runs it right to left, and
///   [`styles`](Self::styles) gives each value its own style.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Charset, Sparkline};
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let values = [2.0, 4.0, 3.0, 9.0, 6.0, 5.0];
/// let spark = Sparkline::new(values).min_max(true);
/// assert_eq!(console.render_export(&spark), "▁▃▂█▅▄ min 2 max 9\n");
///
/// let ascii = Sparkline::new(values).charset(Charset::Ascii);
/// assert_eq!(console.render_export(&ascii), "_-.#=:\n");
///
/// let braille = Sparkline::new(values).charset(Charset::Braille);
/// assert_eq!(console.render_export(&braille), "⣠⣸⣦\n");
///
/// // Three rows: eight heights each.
/// let tall = Sparkline::new([1.0, 4.0, 7.0, 10.0]).rows(3);
/// assert_eq!(console.render_export(&tall), "   █\n ▁██\n▁███\n");
/// ```
#[derive(Clone, Debug, PartialEq)]
#[must_use = "builder methods return a new value; it does nothing until it is rendered or used"]
pub struct Sparkline {
    values: Vec<f64>,
    min: Option<f64>,
    max: Option<f64>,
    charset: Charset,
    style: Option<String>,
    min_max: bool,
    threshold: Option<f64>,
    format: ValueFormat,
    rows: Option<usize>,
    direction: Direction,
    absent: Option<(String, String)>,
    styles: Vec<Option<String>>,
    levels: Levels,
}

impl Sparkline {
    /// A sparkline of `values`, in order.
    pub fn new(values: impl IntoIterator<Item = f64>) -> Self {
        Sparkline {
            values: values.into_iter().collect(),
            min: None,
            max: None,
            charset: Charset::Auto,
            style: None,
            min_max: false,
            threshold: None,
            format: ValueFormat::Compact,
            rows: None,
            direction: Direction::LeftToRight,
            absent: None,
            styles: Vec::new(),
            levels: Levels::Nine,
        }
    }

    /// A sparkline of values that may be absent: each `None` is a gap, as
    /// ratatui's absent values are.
    pub fn optional(values: impl IntoIterator<Item = Option<f64>>) -> Self {
        Self::new(values.into_iter().map(|v| v.unwrap_or(f64::NAN)))
    }

    /// Fix the bottom and top of the scale instead of using the data's
    /// range; values outside are clamped.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Fix only the bottom of the scale (`0.0` to start bars at zero).
    pub fn min(mut self, min: f64) -> Self {
        self.min = Some(min);
        self
    }

    /// Fix only the top of the scale.
    pub fn max(mut self, max: f64) -> Self {
        self.max = Some(max);
        self
    }

    /// Glyphs to draw with (default [`Charset::Auto`]: blocks).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// The line's style: a theme key or a definition (default
    /// `chart.spark`).
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// Mark the lowest and highest values (`chart.min`, `chart.max`) and
    /// write them after the line.
    pub fn min_max(mut self, show: bool) -> Self {
        self.min_max = show;
        self
    }

    /// Style values above `threshold` with `chart.over`, and write how many
    /// there are after the line.
    pub fn threshold(mut self, threshold: f64) -> Self {
        self.threshold = Some(threshold);
        self
    }

    /// How the summary writes values (default [`ValueFormat::Compact`]).
    pub fn format(mut self, format: ValueFormat) -> Self {
        self.format = format;
        self
    }

    /// Height in rows (at least 1, at most 65535), whatever height it is
    /// given. Without it, the sparkline fills the height it is given, or
    /// takes one row.
    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = Some(rows.clamp(1, MAX_ROWS));
        self
    }

    /// Which way it runs (default [`Direction::LeftToRight`]).
    pub fn direction(mut self, direction: Direction) -> Self {
        self.direction = direction;
        self
    }

    /// Draw gaps with `symbol` down the whole column, in `style` (a theme
    /// key or a definition; empty for none), as ratatui's
    /// `absent_value_symbol` and `absent_value_style`. A symbol that is
    /// not one cell wide draws a space; on an ASCII console a symbol past
    /// ASCII is written as its look-alike, or `?`.
    pub fn absent(mut self, symbol: impl Into<String>, style: impl Into<String>) -> Self {
        self.absent = Some((symbol.into(), style.into()));
        self
    }

    /// A style for each value, in order (theme keys or definitions); `None`,
    /// and values past the end of the list, keep the sparkline's. A value's
    /// own style wins over the marks of [`min_max`](Self::min_max) and
    /// [`threshold`](Self::threshold). A resampled cell takes the style of
    /// the last styled value in its bucket.
    pub fn styles(mut self, styles: Vec<Option<String>>) -> Self {
        self.styles = styles;
        self
    }

    /// How finely blocks draw the top of a bar (default [`Levels::Nine`]).
    pub fn levels(mut self, levels: Levels) -> Self {
        self.levels = levels;
        self
    }

    /// The values; a gap is NaN.
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// The charset this sparkline draws with on `console`.
    pub(crate) fn resolved_charset(&self, console: &Console, options: &ConsoleOptions) -> Charset {
        self.charset.resolve(console, options, Charset::Blocks)
    }

    /// Cells for every value and the summary, at `charset`.
    pub(crate) fn natural_width(&self, charset: Charset) -> usize {
        self.natural_cells(charset) + self.summary_width()
    }

    fn scale(&self) -> Scale {
        Scale::from_values(self.values.iter().copied()).bounds(self.min, self.max)
    }

    /// Values per cell for a charset.
    fn per_cell(charset: Charset) -> usize {
        if charset == Charset::Braille {
            2
        } else {
            1
        }
    }

    /// The text after the line, as (text, theme key) parts.
    fn summary(&self) -> Vec<(String, &'static str)> {
        let finite: Vec<f64> = self
            .values
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .collect();
        let mut parts = Vec::new();
        if self.min_max && !finite.is_empty() {
            let lo = finite.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            parts.push((format!("min {}", self.format.format(lo)), "chart.min"));
            parts.push((format!("max {}", self.format.format(hi)), "chart.max"));
        }
        if let Some(t) = self.threshold {
            let over = finite.iter().filter(|v| **v > t).count();
            if over > 0 {
                parts.push((format!("{over} > {}", self.format.format(t)), "chart.over"));
            }
        }
        parts
    }

    fn summary_width(&self) -> usize {
        self.summary().iter().map(|(t, _)| 1 + cells(t)).sum()
    }

    fn natural_cells(&self, charset: Charset) -> usize {
        self.values.len().div_ceil(Self::per_cell(charset))
    }

    /// The values to draw in `slots` slots, each with the index of its
    /// first value and its own style: as they are when they fit, otherwise
    /// the mean of each of `slots` equal buckets.
    fn resample(&self, slots: usize) -> Vec<Sample> {
        let n = self.values.len();
        let own = |i: usize| self.styles.get(i).cloned().flatten();
        if n <= slots {
            return self
                .values
                .iter()
                .enumerate()
                .map(|(i, &value)| Sample {
                    value,
                    start: i,
                    style: own(i),
                })
                .collect();
        }
        (0..slots)
            .map(|i| {
                let start = i * n / slots;
                let end = ((i + 1) * n / slots).max(start + 1);
                let bucket: Vec<f64> = self.values[start..end]
                    .iter()
                    .copied()
                    .filter(|v| v.is_finite())
                    .collect();
                let value = if bucket.is_empty() {
                    f64::NAN
                } else {
                    bucket.iter().sum::<f64>() / bucket.len() as f64
                };
                Sample {
                    value,
                    start,
                    style: (start..end).rev().find_map(own),
                }
            })
            .collect()
    }

    /// One row, as [`KpiCard`](super::KpiCard) draws it.
    pub(crate) fn line(&self, console: &Console, charset: Charset, width: usize) -> Line {
        self.lines(console, charset, width, 1)
            .pop()
            .unwrap_or_default()
    }

    /// `rows` rows, at most `width` cells wide.
    fn lines(&self, console: &Console, charset: Charset, width: usize, rows: usize) -> Vec<Line> {
        let per_cell = Self::per_cell(charset);
        let natural = self.natural_cells(charset);
        let summary_w = self.summary_width();
        let (cells_used, show_summary) = if natural + summary_w <= width {
            (natural, true)
        } else {
            (natural.min(width), false)
        };
        let mut samples = self.resample(cells_used * per_cell);
        let scale = self.scale();
        let base = self
            .style
            .as_deref()
            .map(|s| user_style(console, s))
            .unwrap_or_else(|| theme_style(console, "chart.spark"));
        let colour = has_colour(console);
        let finite = || self.values.iter().copied().filter(|v| v.is_finite());
        let lo = finite().fold(f64::INFINITY, f64::min);
        let hi = finite().fold(f64::NEG_INFINITY, f64::max);
        // Mark extremes on the drawn samples (the first of each).
        let drawn = |pick: f64| {
            samples.iter().position(|s| s.value == pick).or_else(|| {
                // Resampled: the slot whose bucket holds the extreme.
                let at = self.values.iter().position(|v| *v == pick)?;
                samples.iter().rposition(|s| s.start <= at)
            })
        };
        let (min_slot, max_slot) = if self.min_max && lo.is_finite() {
            (drawn(lo), drawn(hi))
        } else {
            (None, None)
        };
        let slot_style = |i: usize, sample: &Sample| -> Option<Style> {
            if !colour {
                return None;
            }
            if let Some(own) = &sample.style {
                return Some(user_style(console, own));
            }
            let key = if self.threshold.is_some_and(|t| sample.value > t) {
                "chart.over"
            } else if max_slot == Some(i) {
                "chart.max"
            } else if min_slot == Some(i) {
                "chart.min"
            } else {
                return Some(base.clone());
            };
            Some(theme_style(console, key))
        };
        let absent = self.absent.as_ref().map(|(symbol, style)| {
            let symbol = if charset == Charset::Ascii && !symbol.is_ascii() {
                ascii_text(symbol)
            } else {
                symbol.clone()
            };
            let symbol = if cells(&symbol) == 1 {
                symbol
            } else {
                " ".to_string()
            };
            let style = (colour && !style.trim().is_empty()).then(|| user_style(console, style));
            (symbol, style)
        });
        let gap = |out: &mut Line| match &absent {
            Some((symbol, style)) => out.push(symbol, style.clone()),
            None => out.push(" ", None),
        };
        // How many steps of `steps` a finite value fills: at least one.
        let height = |value: f64, steps: usize| {
            scale
                .normalize(value)
                .map(|n| 1 + (n * (steps - 1) as f64).round() as usize)
        };

        // The cells of each row, bottom row last, left to right in drawing
        // order; right to left reverses the samples first.
        let mut indexed: Vec<(usize, Sample)> = samples.drain(..).enumerate().collect();
        if self.direction == Direction::RightToLeft {
            indexed.reverse();
        }
        let mut bars: Vec<Line> = (0..rows).map(|_| Line::new()).collect();
        match charset {
            Charset::Braille => {
                // Two values a cell, paired from the first value's end: right
                // to left, the first value is the right half of the last
                // cell.
                let mut slots: Vec<Option<&(usize, Sample)>> = indexed.iter().map(Some).collect();
                if self.direction == Direction::RightToLeft && slots.len() % 2 == 1 {
                    slots.insert(0, None);
                }
                for chunk in slots.chunks(2) {
                    // A cell whose left half is empty has its value right.
                    let offset = usize::from(chunk[0].is_none());
                    let pair: Vec<&(usize, Sample)> = chunk.iter().flatten().copied().collect();
                    let dots: Vec<Option<usize>> = pair
                        .iter()
                        .map(|(_, s)| height(s.value, rows * 4))
                        .collect();
                    if dots.iter().all(Option::is_none) {
                        for row in &mut bars {
                            gap(row);
                        }
                        continue;
                    }
                    let mut style = None;
                    for (i, s) in &pair {
                        if !s.value.is_finite() {
                            continue;
                        }
                        let own = slot_style(*i, s);
                        if style.is_none() || own.as_ref() != Some(&base) {
                            style = own;
                        }
                    }
                    for (r, row) in bars.iter_mut().enumerate() {
                        let from_bottom = rows - 1 - r;
                        let mut bits = 0u32;
                        for (k, d) in dots.iter().enumerate() {
                            let half = k + offset;
                            let Some(d) = d else {
                                continue;
                            };
                            let here = d.saturating_sub(from_bottom * 4).min(4);
                            let column: [u32; 4] = if half == 0 {
                                [0x40, 0x04, 0x02, 0x01]
                            } else {
                                [0x80, 0x20, 0x10, 0x08]
                            };
                            bits |= column[..here].iter().sum::<u32>();
                        }
                        if bits == 0 {
                            row.push(" ", None);
                        } else {
                            let glyph = char::from_u32(0x2800 + bits).unwrap_or(' ');
                            row.push(&glyph.to_string(), style.clone());
                        }
                    }
                }
            }
            _ => {
                let ascii = charset == Charset::Ascii;
                for (i, sample) in &indexed {
                    let Some(eighths) = height(sample.value, rows * 8) else {
                        for row in &mut bars {
                            gap(row);
                        }
                        continue;
                    };
                    let style = slot_style(*i, sample);
                    for (r, row) in bars.iter_mut().enumerate() {
                        let from_bottom = rows - 1 - r;
                        let here = eighths.saturating_sub(from_bottom * 8).min(8);
                        let glyph = match here {
                            0 => ' ',
                            n if ascii => ASCII[n - 1],
                            n => self.levels.glyph(n),
                        };
                        if glyph == ' ' {
                            row.push(" ", None);
                        } else {
                            row.push(&glyph.to_string(), style.clone());
                        }
                    }
                }
            }
        }

        // The summary goes on the bottom row: after the bars, or before
        // them right to left, each part a space from the bars.
        let parts = if show_summary {
            self.summary()
        } else {
            Vec::new()
        };
        let words = |before: bool| {
            let mut line = Line::new();
            for (text, key) in &parts {
                if before {
                    line.push(text, colour.then(|| theme_style(console, key)));
                    line.push(" ", None);
                } else {
                    line.push(" ", None);
                    line.push(text, colour.then(|| theme_style(console, key)));
                }
            }
            line
        };
        let last = rows - 1;
        match self.direction {
            Direction::LeftToRight => {
                if let Some(bottom) = bars.get_mut(last) {
                    bottom.append(words(false));
                }
                bars
            }
            Direction::RightToLeft => {
                // Against the right edge of the width.
                let words = words(true);
                let words_w = words.width();
                let lead = width.saturating_sub(cells_used + words_w);
                let mut words = Some(words);
                bars.into_iter()
                    .enumerate()
                    .map(|(r, row)| {
                        let mut line = Line::new();
                        line.pad(lead);
                        match if r == last { words.take() } else { None } {
                            Some(words) => line.append(words),
                            None => line.pad(words_w),
                        }
                        line.append(row);
                        line
                    })
                    .collect()
            }
        }
    }
}

/// A value as drawn: itself or a bucket's mean, the index of its first
/// value, and its own style.
#[derive(Clone, Debug)]
struct Sample {
    value: f64,
    start: usize,
    style: Option<String>,
}

impl Renderable for Sparkline {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let charset = self.charset.resolve(console, options, Charset::Blocks);
        let width = options.max_width;
        let rows = self.rows.or(options.height).unwrap_or(1).clamp(1, MAX_ROWS);
        lines_to_segments(self.lines(console, charset, width, rows), width)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let charset = self.charset.resolve(console, options, Charset::Blocks);
        let natural = self.natural_cells(charset);
        let max = natural + self.summary_width();
        Measurement::new(natural.min(4), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// `Sparkline, 24 values: min 12, max 52, last 14, trend up`, with the
/// gaps counted and how many values are over the threshold.
impl crate::a11y::AccessibleText for Sparkline {
    fn accessible_text(&self, _width: usize) -> String {
        self.facts("Sparkline")
    }
}

impl Sparkline {
    /// The finite values' range, with a fixed bound in place of its end:
    /// what a shared scale unifies.
    pub(crate) fn value_extent(&self) -> Option<(f64, f64)> {
        let data = super::describe::extent(self.values.iter().copied());
        let lo = self.min.or(data.map(|d| d.0))?;
        let hi = self.max.or(data.map(|d| d.1))?;
        Some((lo.min(hi), lo.max(hi)))
    }

    /// `what`, the count and the series' facts.
    pub(crate) fn facts(&self, what: &str) -> String {
        if self.values.is_empty() {
            return format!("{what}, no values");
        }
        let points = super::describe::indexed(&self.values);
        let facts = super::describe::series(&points, self.format, ("value", "values"));
        // "24 values, 1 missing, min …": the count goes before the colon.
        let (count, rest) = facts.split_once(", ").unwrap_or((facts.as_str(), ""));
        let mut out = format!("{what}, {count}");
        if !rest.is_empty() {
            out.push_str(": ");
            out.push_str(rest);
        }
        if let Some(t) = self.threshold {
            let over = self
                .values
                .iter()
                .filter(|v| v.is_finite() && **v > t)
                .count();
            out.push_str(&format!(
                ", {} above {}",
                super::describe::count(over),
                self.format.format(t)
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessible_text_names_the_extremes_and_the_trend() {
        use crate::a11y::AccessibleText;
        let spark = Sparkline::new([2.0, 4.0, 3.0, 9.0, 6.0, 5.0]);
        assert_eq!(
            spark.accessible_text(80),
            "Sparkline, 6 values: min 2, max 9, last 5, trend up"
        );
        let gaps = Sparkline::optional([Some(9.0), None, Some(4.0), Some(1.0)]).threshold(3.0);
        assert_eq!(
            gaps.accessible_text(80),
            "Sparkline, 4 values: 1 missing, min 1, max 9, last 1, trend down, 2 above 3"
        );
        assert_eq!(
            Sparkline::new([]).accessible_text(80),
            "Sparkline, no values"
        );
    }

    fn rows(width: usize, spark: &Sparkline) -> Vec<String> {
        let console = Console::builder().width(width).color_system(None).build();
        console
            .render_export(spark)
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn coloured(width: usize) -> Console {
        Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(rich::ColorSystem::Standard))
            .no_color(false)
            .build()
    }

    #[test]
    fn several_rows_stack_eighths() {
        let spark = Sparkline::new((1..=9).map(f64::from)).rows(3);
        assert_eq!(
            rows(20, &spark),
            vec!["      ▂▅█", "   ▂▅▇███", "▁▄▇██████"]
        );
        // One row is as before.
        assert_eq!(rows(20, &spark.clone().rows(1)), vec!["▁▂▃▄▅▅▆▇█"]);
        // The summary goes on the bottom row.
        let summary = Sparkline::new([1.0, 2.0, 3.0]).min_max(true).rows(2);
        assert_eq!(
            rows(20, &summary),
            vec![" ▁█            ", "▁██ min 1 max 3"]
        );
    }

    #[test]
    fn braille_stacks_four_dots_a_row() {
        let spark = Sparkline::new([1.0, 5.0, 3.0, 8.0, 2.0])
            .charset(Charset::Braille)
            .rows(2);
        assert_eq!(rows(20, &spark), vec!["⢀⢸ ", "⣸⣾⡄"]);
    }

    #[test]
    fn right_to_left_puts_the_first_value_at_the_right_edge() {
        let spark = Sparkline::new([1.0, 2.0, 3.0]).direction(Direction::RightToLeft);
        assert_eq!(rows(6, &spark), vec!["   █▅▁"]);
        // The summary goes before the bars.
        let summary = spark.clone().min_max(true);
        assert_eq!(rows(20, &summary), vec!["     min 1 max 3 █▅▁"]);
        // In Braille the first value is the right half of the last cell.
        let braille = spark.charset(Charset::Braille);
        assert_eq!(rows(2, &braille), vec!["⢸⣆"]);
        // Fewer cells than values: resampled, still from the right.
        let long = Sparkline::new((0..8).map(f64::from)).direction(Direction::RightToLeft);
        assert_eq!(rows(4, &long), vec!["█▆▄▂"]);
    }

    #[test]
    fn absent_values_fill_their_column() {
        let spark = Sparkline::optional([Some(1.0), None, Some(3.0)]);
        assert_eq!(rows(10, &spark), vec!["▁ █"]);
        let marked = spark.clone().absent("░", "red");
        assert_eq!(rows(10, &marked), vec!["▁░█"]);
        assert_eq!(rows(10, &marked.clone().rows(2)), vec![" ░█", "▁░█"]);
        // A symbol that is not one cell wide is a space.
        assert_eq!(rows(10, &spark.clone().absent("日", "")), vec!["▁ █"]);
        // On an ASCII console the symbol is ASCII.
        let ascii = Console::builder()
            .width(10)
            .color_system(None)
            .ascii_only(true)
            .build();
        assert_eq!(ascii.render_export(&marked), "_.#\n");
        // The absent style is used.
        let console = coloured(10);
        let lines = console.render_lines(&marked, &console.options(), false);
        let gap = lines[0].iter().find(|s| s.text == "░").unwrap();
        assert_eq!(gap.style, Style::parse("red").ok());
    }

    #[test]
    fn each_value_can_have_its_own_style() {
        let spark = Sparkline::new([1.0, 2.0, 3.0])
            .style("cyan")
            .styles(vec![None, Some("red".into())]);
        let console = coloured(10);
        assert_eq!(
            console.render_export(&spark),
            "\u{1b}[36m▁\u{1b}[0m\u{1b}[31m▅\u{1b}[0m\u{1b}[36m█\u{1b}[0m\n"
        );
        // A value's style wins over the marks.
        let marked = spark.min_max(true).threshold(2.5).styles(vec![
            Some("green".into()),
            None,
            Some("blue".into()),
        ]);
        let lines = console.render_lines(&marked, &console.options(), false);
        let styles: Vec<Option<Style>> = lines[0].iter().take(3).map(|s| s.style.clone()).collect();
        assert_eq!(styles[0], Style::parse("green").ok());
        assert_eq!(styles[2], Style::parse("blue").ok());
        // Resampled, a cell takes the last styled value of its bucket.
        let long = Sparkline::new([1.0, 2.0, 3.0, 4.0]).styles(vec![
            Some("red".into()),
            Some("blue".into()),
            None,
            None,
        ]);
        let narrow = coloured(2);
        let lines = narrow.render_lines(&long, &narrow.options(), false);
        assert_eq!(lines[0][0].style, Style::parse("blue").ok());
    }

    #[test]
    fn three_levels_draw_halves_and_wholes() {
        let spark = Sparkline::new((0..8).map(f64::from)).levels(Levels::Three);
        assert_eq!(rows(10, &spark), vec![" ▄▄▄▄▄██"]);
        assert_eq!(rows(10, &spark.rows(2)), vec!["    ▄▄▄█", " ▄▄█████"]);
    }

    #[test]
    fn it_fills_the_height_it_is_given() {
        let console = Console::builder().width(10).color_system(None).build();
        let spark = Sparkline::new([1.0, 2.0, 3.0]);
        let lines = console.render_lines(&spark, &console.options().update_height(4), false);
        let text: Vec<String> = lines
            .iter()
            .map(|l| l.iter().map(|s| s.text.as_str()).collect())
            .collect();
        assert_eq!(text, vec!["  █", " ▁█", " ██", "▁██"]);
        // Its own rows win.
        let lines =
            console.render_lines(&spark.rows(1), &console.options().update_height(4), false);
        assert_eq!(lines.iter().filter(|l| !l.is_empty()).count(), 1);
    }
}
