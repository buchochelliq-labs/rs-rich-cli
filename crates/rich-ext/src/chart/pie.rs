//! Pie and donut charts: shares of a whole as slices of a disc, with a
//! legend of labels and percentages.

use std::f64::consts::TAU;

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::{
    cells, has_colour, lines_to_segments, series_key, truncate, user_style, Charset, Line,
    ValueFormat,
};

/// Rows for the disc when neither [`PieChart::height`] nor the height it is
/// given says otherwise.
const DEFAULT_ROWS: usize = 8;
/// The tallest disc drawn, in rows.
const MAX_ROWS: usize = 512;
/// The largest share of the radius a donut's hole takes.
const MAX_HOLE: f64 = 0.9;
/// Cell fills that tell slices apart without colour, by slice (they cycle).
const PATTERNS: [char; 6] = ['█', '░', '▓', '▒', '▚', '▞'];
/// The same in ASCII.
const ASCII_PATTERNS: [char; 8] = ['#', '+', '@', ':', '%', '=', 'o', '*'];
/// The legend's marker when colour tells the slices apart.
const MARKER: char = '■';

/// One slice of a [`PieChart`].
#[derive(Clone, Debug, PartialEq)]
#[must_use = "builder methods return a new value; it does nothing until it is rendered or used"]
pub struct Slice {
    /// Its name in the legend.
    pub label: String,
    /// Its share of the whole, against the other slices' values. A value
    /// that is not a finite number above zero takes no part of the disc.
    pub value: f64,
    /// A theme key or style definition, instead of `chart.series.N`.
    pub style: Option<String>,
}

impl Slice {
    /// A slice called `label` of `value`.
    pub fn new(label: impl Into<String>, value: f64) -> Self {
        Slice {
            label: label.into(),
            value,
            style: None,
        }
    }

    /// This slice's style: a theme key or a definition.
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }
}

/// Shares of a whole as slices of a disc, clockwise from the top, with a
/// legend naming each slice and its percentage; with
/// [`donut`](Self::donut), a ring with a hole, and text in the hole.
///
/// - Slice `N` takes the theme style `chart.series.N` (they cycle), or its
///   own [`Slice::style`].
/// - [`Charset::Blocks`] (the default) draws with half blocks (`▀▄█`): two
///   pixels a cell, each slice's colour, so two slices can share a cell.
///   Without colour it draws a cell a slice, each slice in its own fill
///   (`█░▓▒▚▞`), and the legend shows the fills.
/// - [`Charset::Braille`] draws 2×4 dots a cell and leaves the dots on the
///   edges between slices out, so the slices read as wedges without colour
///   too; a cell takes the colour of the slice with most of its dots.
/// - [`Charset::Ascii`] draws a cell a slice in `#+@:%=o*`, and is what an
///   ASCII-only console gets whatever was asked for.
/// - The disc is [`height`](Self::height) rows tall (default: the height it
///   is given, or 8) and twice that wide, so it looks round. The legend goes
///   beside it when there is room, under it otherwise; given less width than
///   the disc, the disc shrinks.
/// - A value that is not a finite number above zero takes no part of the
///   disc; the legend writes `-` for its percentage. A slice too small for
///   a cell is still in the legend.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Charset, PieChart};
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let pie = PieChart::from_pairs([("api", 50.0), ("web", 30.0), ("cron", 20.0)])
///     .height(4)
///     .charset(Charset::Ascii);
/// assert_eq!(
///     console.render_export(&pie),
///     concat!(
///         " @@@###   # api   50.0%\n",
///         "+@@@####  + web   30.0%\n",
///         "++++####  @ cron  20.0%\n",
///         " +++###                \n",
///     )
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
#[must_use = "builder methods return a new value; it does nothing until it is rendered or used"]
pub struct PieChart {
    slices: Vec<Slice>,
    hole: f64,
    rows: Option<usize>,
    charset: Charset,
    legend: bool,
    percentages: bool,
    values: bool,
    format: ValueFormat,
    centre: Option<String>,
}

impl Default for PieChart {
    fn default() -> Self {
        Self::new()
    }
}

/// Which part of the chart a pixel of the disc is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pixel {
    Empty,
    Slice(usize),
}

impl PieChart {
    /// An empty chart; add slices with [`slice`](Self::slice).
    pub fn new() -> Self {
        PieChart {
            slices: Vec::new(),
            hole: 0.0,
            rows: None,
            charset: Charset::Auto,
            legend: true,
            percentages: true,
            values: false,
            format: ValueFormat::Compact,
            centre: None,
        }
    }

    /// A chart of `(label, value)` pairs.
    pub fn from_pairs<L: Into<String>>(pairs: impl IntoIterator<Item = (L, f64)>) -> Self {
        pairs
            .into_iter()
            .fold(Self::new(), |chart, (label, value)| {
                chart.slice(label, value)
            })
    }

    /// Add a slice.
    pub fn slice(self, label: impl Into<String>, value: f64) -> Self {
        self.push(Slice::new(label, value))
    }

    /// Add a [`Slice`] built with its own style.
    pub fn push(mut self, slice: Slice) -> Self {
        self.slices.push(slice);
        self
    }

    /// The slices, in order (clockwise from the top).
    pub fn slices(&self) -> &[Slice] {
        &self.slices
    }

    /// Draw a ring: `hole` is the hole's share of the radius (0 for a pie,
    /// at most 0.9; 0.5 is a common donut).
    pub fn donut(mut self, hole: f64) -> Self {
        self.hole = if hole.is_finite() {
            hole.clamp(0.0, MAX_HOLE)
        } else {
            0.0
        };
        self
    }

    /// Text written in a donut's hole, such as the total, when it fits.
    pub fn centre_text(mut self, text: impl Into<String>) -> Self {
        self.centre = Some(text.into());
        self
    }

    /// Rows for the disc (at least 1); it is twice as many cells wide.
    pub fn height(mut self, rows: usize) -> Self {
        self.rows = Some(rows.clamp(1, MAX_ROWS));
        self
    }

    /// Glyphs to draw with (default [`Charset::Auto`]: half blocks).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// Show the legend (default on).
    pub fn legend(mut self, legend: bool) -> Self {
        self.legend = legend;
        self
    }

    /// Write each slice's percentage in the legend (default on).
    pub fn show_percentages(mut self, show: bool) -> Self {
        self.percentages = show;
        self
    }

    /// Write each slice's value in the legend (default off).
    pub fn show_values(mut self, show: bool) -> Self {
        self.values = show;
        self
    }

    /// How values are written (default [`ValueFormat::Compact`]).
    pub fn format(mut self, format: ValueFormat) -> Self {
        self.format = format;
        self
    }

    /// Whether a slice takes part in the disc.
    fn counts(value: f64) -> bool {
        value.is_finite() && value > 0.0
    }

    fn total(&self) -> f64 {
        self.slices
            .iter()
            .map(|s| s.value)
            .filter(|v| Self::counts(*v))
            .sum()
    }

    /// Each slice's share of the whole (0 to 1), `None` for one that takes
    /// no part.
    pub fn shares(&self) -> Vec<Option<f64>> {
        // Summed over the largest value, so finite values too large to add
        // up (two of `f64::MAX`) still share the disc.
        let largest = self
            .slices
            .iter()
            .map(|s| s.value)
            .filter(|v| Self::counts(*v))
            .fold(0.0, f64::max);
        let scaled: f64 = self
            .slices
            .iter()
            .map(|s| s.value)
            .filter(|v| Self::counts(*v))
            .map(|v| v / largest)
            .sum();
        self.slices
            .iter()
            .map(|s| (Self::counts(s.value) && largest > 0.0).then(|| s.value / largest / scaled))
            .collect()
    }

    /// Where each slice ends, as a share of the turn, for the slices that
    /// take part; the others end where the one before them does.
    fn ends(&self) -> Vec<f64> {
        let mut at = 0.0;
        self.shares()
            .into_iter()
            .map(|share| {
                at += share.unwrap_or(0.0);
                at
            })
            .collect()
    }

    fn style_of(&self, console: &Console, index: usize) -> Style {
        let spec = self.slices[index]
            .style
            .clone()
            .unwrap_or_else(|| series_key(index));
        user_style(console, &spec)
    }

    /// The pixel at `(u, v)`, both from -1 to 1 with `v` down, and how far
    /// it is from the nearest edge between slices, along the arc.
    fn pixel(&self, ends: &[f64], u: f64, v: f64) -> (Pixel, f64) {
        let r = (u * u + v * v).sqrt();
        if r > 1.0 || r < self.hole {
            return (Pixel::Empty, f64::INFINITY);
        }
        let turn = (u.atan2(-v) / TAU).rem_euclid(1.0);
        let shares = self.shares();
        let index = ends
            .iter()
            .enumerate()
            .position(|(i, &end)| shares[i].is_some() && turn < end)
            .or_else(|| shares.iter().rposition(Option::is_some));
        let Some(index) = index else {
            return (Pixel::Empty, f64::INFINITY);
        };
        let several = shares.iter().filter(|s| s.is_some()).count() > 1;
        let edge = if several {
            ends.iter()
                .map(|&end| {
                    let d = (turn - end).abs();
                    d.min(1.0 - d)
                })
                .chain(std::iter::once(turn.min(1.0 - turn)))
                .fold(f64::INFINITY, f64::min)
                * TAU
                * r
        } else {
            f64::INFINITY
        };
        (Pixel::Slice(index), edge)
    }

    /// The disc's rows and the legend's, and whether the legend goes beside
    /// the disc.
    fn fit(&self, width: usize, height: Option<usize>, legend_w: usize) -> (usize, bool) {
        let legend_rows = if self.legend { self.slices.len() } else { 0 };
        let mut rows = self
            .rows
            .or(height)
            .unwrap_or(DEFAULT_ROWS)
            .clamp(1, MAX_ROWS);
        let beside = legend_w > 0 && width >= rows * 2 + 2 + legend_w;
        if !beside && self.rows.is_none() {
            if let Some(h) = height {
                rows = h.saturating_sub(legend_rows).max(1).min(rows);
            }
        }
        rows = rows.min((width / 2).max(1));
        (rows, beside)
    }

    /// The legend's lines: a marker, the label, the percentage and the
    /// value, the columns aligned.
    fn legend_lines(&self, console: &Console, charset: Charset, colour: bool) -> Vec<Line> {
        if !self.legend {
            return Vec::new();
        }
        let ascii = charset == Charset::Ascii;
        let shares = self.shares();
        let percent = |share: Option<f64>| match share {
            Some(s) => format!("{:.1}%", s * 100.0),
            None => "-".to_string(),
        };
        let label_w = self
            .slices
            .iter()
            .map(|s| cells(&truncate(&s.label, usize::MAX, ascii)))
            .max()
            .unwrap_or(0);
        let pct_w = shares
            .iter()
            .map(|s| cells(&percent(*s)))
            .max()
            .unwrap_or(0);
        let values: Vec<String> = self
            .slices
            .iter()
            .map(|s| truncate(&self.format.format(s.value), usize::MAX, ascii))
            .collect();
        let value_w = values.iter().map(|v| cells(v)).max().unwrap_or(0);
        let label_style = colour.then(|| super::theme_style(console, "chart.label"));
        let value_style = colour.then(|| super::theme_style(console, "chart.value"));
        self.slices
            .iter()
            .enumerate()
            .map(|(i, slice)| {
                let mut line = Line::new();
                let marker = self.marker(charset, colour, i);
                line.push(
                    &marker.to_string(),
                    colour.then(|| self.style_of(console, i)),
                );
                line.pad(1);
                let label = truncate(&slice.label, usize::MAX, ascii);
                line.push(&label, label_style.clone());
                line.pad(label_w - cells(&label));
                if self.percentages {
                    let text = percent(shares[i]);
                    line.pad(2 + pct_w - cells(&text));
                    line.push(&text, value_style.clone());
                }
                if self.values {
                    line.pad(2 + value_w - cells(&values[i]));
                    line.push(&values[i], value_style.clone());
                }
                line
            })
            .collect()
    }

    /// What marks slice `index` in the legend.
    fn marker(&self, charset: Charset, colour: bool, index: usize) -> char {
        match charset {
            // The disc is drawn in patterns in ASCII, with colour or not.
            Charset::Ascii => ASCII_PATTERNS[index % ASCII_PATTERNS.len()],
            _ if colour => MARKER,
            Charset::Braille => '⣿',
            _ => PATTERNS[index % PATTERNS.len()],
        }
    }

    fn legend_width(&self, console: &Console, charset: Charset) -> usize {
        self.legend_lines(console, charset, false)
            .iter()
            .map(Line::width)
            .max()
            .unwrap_or(0)
    }

    /// The disc, `rows` tall and `2 * rows` wide.
    fn disc(&self, console: &Console, charset: Charset, colour: bool, rows: usize) -> Vec<Line> {
        let cols = rows * 2;
        let ends = self.ends();
        let styles: Vec<Option<Style>> = (0..self.slices.len())
            .map(|i| colour.then(|| self.style_of(console, i)))
            .collect();
        let at = |n: usize, of: usize| (n as f64 + 0.5) / of as f64 * 2.0 - 1.0;
        let mut out = Vec::with_capacity(rows);
        for row in 0..rows {
            let mut line = Line::new();
            for col in 0..cols {
                let (cell, style) = match charset {
                    Charset::Braille => {
                        // 2×4 dots; a dot within half a dot of an edge
                        // between slices is left out.
                        let gap = 0.5 / cols as f64 * 2.0;
                        let mut bits = 0u8;
                        let mut votes = vec![0usize; self.slices.len()];
                        for dy in 0..4 {
                            for dx in 0..2 {
                                let u = at(col * 2 + dx, cols * 2);
                                let v = at(row * 4 + dy, rows * 4);
                                if let (Pixel::Slice(i), edge) = self.pixel(&ends, u, v) {
                                    if edge >= gap {
                                        bits |= braille_bit(dx, dy);
                                        votes[i] += 1;
                                    }
                                }
                            }
                        }
                        let winner = votes
                            .iter()
                            .enumerate()
                            .filter(|(_, n)| **n > 0)
                            .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(&a.0)))
                            .map(|(i, _)| i);
                        if bits == 0 {
                            (' ', None)
                        } else {
                            let c = char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' ');
                            (c, winner.and_then(|i| styles[i].clone()))
                        }
                    }
                    Charset::Blocks | Charset::Auto if colour => {
                        let u = at(col, cols);
                        let top = self.pixel(&ends, u, at(row * 2, rows * 2)).0;
                        let bottom = self.pixel(&ends, u, at(row * 2 + 1, rows * 2)).0;
                        half_cell(top, bottom, &styles)
                    }
                    _ => {
                        let patterns: &[char] = if charset == Charset::Ascii {
                            &ASCII_PATTERNS
                        } else {
                            &PATTERNS
                        };
                        match self.pixel(&ends, at(col, cols), at(row, rows)).0 {
                            Pixel::Empty => (' ', None),
                            Pixel::Slice(i) => (patterns[i % patterns.len()], styles[i].clone()),
                        }
                    }
                };
                line.push(&cell.to_string(), style);
            }
            out.push(line);
        }
        self.write_centre(&mut out, rows, charset == Charset::Ascii, console, colour);
        out
    }

    /// The centre text over the middle row of the hole, when it fits.
    fn write_centre(
        &self,
        lines: &mut [Line],
        rows: usize,
        ascii: bool,
        console: &Console,
        colour: bool,
    ) {
        let Some(text) = &self.centre else {
            return;
        };
        let room = (self.hole * rows as f64 * 2.0).floor() as usize;
        let text = truncate(text, usize::MAX, ascii);
        if self.hole <= 0.0 || cells(&text) + 1 > room || text.is_empty() {
            return;
        }
        let row = (rows - 1) / 2;
        let cols = rows * 2;
        let start = (cols - cells(&text)) / 2;
        let mut line = Line::new();
        let old = std::mem::take(&mut lines[row]);
        // Rebuild the row: the disc's cells before and after the text.
        let mut cells_: Vec<(String, Option<Style>)> = Vec::new();
        for (t, s) in old.spans {
            for c in t.chars() {
                cells_.push((c.to_string(), s.clone()));
            }
        }
        let style = colour.then(|| super::theme_style(console, "chart.title"));
        for (i, (c, s)) in cells_.iter().enumerate() {
            if i == start {
                line.push(&text, style.clone());
            }
            if i < start || i >= start + cells(&text) {
                line.push(c, s.clone());
            }
        }
        lines[row] = line;
    }

    pub(crate) fn render_lines(&self, console: &Console, options: &ConsoleOptions) -> Vec<Line> {
        let width = options.max_width;
        let charset = self.charset.resolve(console, options, Charset::Blocks);
        let ascii = charset == Charset::Ascii;
        if self.total() <= 0.0 {
            let mut line = Line::new();
            line.push(&truncate("no data", width, ascii), None);
            return vec![line];
        }
        let colour = has_colour(console);
        let legend_w = if self.legend {
            self.legend_width(console, charset)
        } else {
            0
        };
        let (rows, beside) = self.fit(width, options.height, legend_w);
        let disc = self.disc(console, charset, colour, rows);
        let legend = self.legend_lines(console, charset, colour);
        if beside {
            let n = disc.len().max(legend.len());
            // The legend centred against the disc.
            let top = (disc.len().saturating_sub(legend.len())) / 2;
            let mut disc = disc.into_iter();
            let mut legend = legend.into_iter();
            (0..n)
                .map(|row| {
                    let mut line = disc.next().unwrap_or_default();
                    line.pad(rows * 2 - line.width());
                    if row >= top {
                        if let Some(entry) = legend.next() {
                            line.pad(2);
                            line.append(entry);
                        }
                    }
                    line
                })
                .collect()
        } else {
            let mut out = disc;
            out.extend(legend);
            out
        }
    }

    fn natural_width(&self, console: &Console, charset: Charset) -> usize {
        let rows = self.rows.unwrap_or(DEFAULT_ROWS);
        let legend = if self.legend {
            self.legend_width(console, charset)
        } else {
            0
        };
        rows * 2 + if legend > 0 { 2 + legend } else { 0 }
    }
}

/// The bit for dot `(dx, dy)` of a Braille cell.
fn braille_bit(dx: usize, dy: usize) -> u8 {
    const BITS: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
    BITS[dy][dx]
}

/// A cell of two pixels, top and bottom, in half blocks.
fn half_cell(top: Pixel, bottom: Pixel, styles: &[Option<Style>]) -> (char, Option<Style>) {
    match (top, bottom) {
        (Pixel::Empty, Pixel::Empty) => (' ', None),
        (Pixel::Slice(a), Pixel::Slice(b)) if a == b => ('█', styles[a].clone()),
        (Pixel::Slice(a), Pixel::Empty) => ('▀', styles[a].clone()),
        (Pixel::Empty, Pixel::Slice(b)) => ('▄', styles[b].clone()),
        (Pixel::Slice(a), Pixel::Slice(b)) => {
            let below = styles[b].as_ref().and_then(|s| s.color().cloned());
            let style = match (&styles[a], below) {
                (Some(s), Some(bg)) => Some(s.combine(&Style::from_color(None, Some(bg)))),
                (s, _) => s.clone(),
            };
            ('▀', style)
        }
    }
}

impl crate::surface::Draw for PieChart {
    fn draw(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        screen: &mut crate::surface::Screen,
        rect: crate::surface::Rect,
    ) {
        super::draw_lines(
            self.render_lines(console, options),
            options.max_width,
            options,
            screen,
            rect,
        );
    }
}

impl Renderable for PieChart {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        lines_to_segments(self.render_lines(console, options), options.max_width)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        if self.total() <= 0.0 {
            return Measurement::new(7, 7).with_maximum(options.max_width);
        }
        let charset = self.charset.resolve(console, options, Charset::Blocks);
        let max = self.natural_width(console, charset);
        Measurement::new(2.min(max), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// `Pie chart, 3 slices, total 100`, then every slice (up to six) with its
/// value and percentage, or the three largest and the smallest. A donut
/// says `Donut chart`.
impl crate::a11y::AccessibleText for PieChart {
    fn accessible_text(&self, _width: usize) -> String {
        use super::describe::{count, counted, ranked};
        let what = if self.hole > 0.0 {
            "Donut chart"
        } else {
            "Pie chart"
        };
        if self.slices.is_empty() {
            return format!("{what}, no slices");
        }
        let mut head = format!("{what}, {}", counted(self.slices.len(), "slice", "slices"));
        head.push_str(&format!(", total {}", self.format.format(self.total())));
        let missing = self
            .slices
            .iter()
            .filter(|s| !Self::counts(s.value))
            .count();
        if missing > 0 {
            head.push_str(&format!(", {} without a share", count(missing)));
        }
        let shares = self.shares();
        let entries: Vec<(String, String, f64)> = self
            .slices
            .iter()
            .zip(&shares)
            .map(|(slice, share)| {
                let text = match share {
                    Some(s) => format!("{} ({:.1}%)", self.format.format(slice.value), s * 100.0),
                    None => "no share".to_string(),
                };
                let rank = if share.is_some() {
                    slice.value
                } else {
                    f64::NAN
                };
                (slice.label.clone(), text, rank)
            })
            .collect();
        let mut out = vec![head];
        out.extend(ranked(&entries, 6));
        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_skip_values_that_cannot_be_drawn() {
        let pie = PieChart::new()
            .slice("a", 3.0)
            .slice("b", -1.0)
            .slice("c", f64::NAN)
            .slice("d", 1.0);
        assert_eq!(pie.shares(), vec![Some(0.75), None, None, Some(0.25)]);
    }

    #[test]
    fn values_too_large_to_add_up_still_share_the_disc() {
        let pie = PieChart::new().slice("a", 1e308).slice("b", 1e308);
        assert_eq!(pie.shares(), vec![Some(0.5), Some(0.5)]);
        let ends = pie.ends();
        assert_eq!(pie.pixel(&ends, 0.1, -0.9).0, Pixel::Slice(0));
        assert_eq!(pie.pixel(&ends, -0.5, 0.0).0, Pixel::Slice(1));
    }

    #[test]
    fn the_first_slice_starts_at_the_top_and_runs_clockwise() {
        let pie = PieChart::from_pairs([("a", 1.0), ("b", 1.0), ("c", 2.0)]);
        let ends = pie.ends();
        // Just right of the top: the first slice.
        assert_eq!(pie.pixel(&ends, 0.1, -0.9).0, Pixel::Slice(0));
        // Lower right: the second.
        assert_eq!(pie.pixel(&ends, 0.5, 0.5).0, Pixel::Slice(1));
        // The left half: the third.
        assert_eq!(pie.pixel(&ends, -0.5, 0.0).0, Pixel::Slice(2));
        // Outside the disc.
        assert_eq!(pie.pixel(&ends, 0.9, 0.9).0, Pixel::Empty);
        // In a donut's hole.
        let donut = pie.donut(0.5);
        assert_eq!(donut.pixel(&ends, 0.1, 0.1).0, Pixel::Empty);
    }
}
