//! Bar charts, horizontal and vertical, in groups or not, and histograms.

use std::ops::Range;

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::axis::exact_labels;
use super::scale::{along, clean};
use super::{
    cell_units, cells, has_colour, lines_to_segments, theme_style, truncate, user_style, Charset,
    Line, Scale, ValueFormat,
};

/// Partial cells for [`Charset::Blocks`]: 1/8 to 7/8 of a cell.
const EIGHTHS: [char; 7] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉'];
const FULL_BLOCK: char = '█';
/// [`Charset::Braille`]: a full cell and its left half.
const BRAILLE_FULL: char = '⣿';
const BRAILLE_HALF: char = '⡇';
/// [`Charset::Ascii`]: a full cell and a half.
const ASCII_FULL: char = '#';
const ASCII_HALF: char = '=';

/// Vertical bars, bottom up: 1/8 to 7/8 of a cell.
const LOWER_EIGHTHS: [char; 7] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇'];
/// Vertical [`Charset::Braille`] and [`Charset::Ascii`] half cells.
const BRAILLE_LOWER_HALF: char = '⣤';
const ASCII_LOWER_HALF: char = '.';
/// Rows for the tallest vertical bar when [`BarChart::bar_width`] is not
/// set and the chart is given no height.
const DEFAULT_COLUMN: usize = 8;
/// The tallest vertical bar drawn, in rows, whatever
/// [`BarChart::bar_width`] asks for.
const MAX_COLUMN: usize = u16::MAX as usize;
/// The thickest vertical bar the chart picks itself, in cells.
const MAX_THICKNESS: usize = 3;
/// The thickest bar [`BarChart::thickness`] allows, in cells or rows.
const MAX_SIDE: usize = u16::MAX as usize;

/// Which way a [`BarChart`]'s bars run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Left to right, one bar per line with its label before it and its
    /// value after (the default).
    #[default]
    Horizontal,
    /// Bottom to top, side by side, with labels under the bars and values
    /// just above them (below, for negative values).
    Vertical,
}

/// How a vertical chart fits its width.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Columns {
    /// Cells per bar, its label and its value.
    slot: usize,
    /// Cells between slots.
    gap: usize,
    /// Cells between groups, on top of `gap`.
    group_gap: usize,
    /// Cells of bar glyphs, centred in the slot.
    thickness: usize,
    labels: bool,
    values: bool,
    /// How many bars fit, from the first.
    shown: usize,
}

/// The narrowest bar drawn before labels and values give way.
const MIN_BAR: usize = 4;
/// Bar cells when [`BarChart::bar_width`] is not set.
const DEFAULT_BAR: usize = 40;

/// One bar of a [`BarChart`].
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    /// Written before the bar (under it, when vertical).
    pub label: String,
    /// The bar's length on the chart's scale.
    pub value: f64,
    /// A theme key or style definition for this bar, instead of
    /// `chart.bar` (`chart.negative` below zero).
    pub style: Option<String>,
    /// Written instead of the value, such as `76°` or `8.5M` (ratatui's
    /// `text_value`).
    pub text: Option<String>,
    /// A theme key or style definition for this bar's value, over the
    /// chart's.
    pub value_style: Option<String>,
    /// A theme key or style definition for this bar's label, over the
    /// chart's.
    pub label_style: Option<String>,
}

impl Bar {
    /// A bar called `label` of `value`.
    pub fn new(label: impl Into<String>, value: f64) -> Self {
        Bar {
            label: label.into(),
            value,
            style: None,
            text: None,
            value_style: None,
            label_style: None,
        }
    }

    /// This bar's style: a theme key or a definition.
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// Write `text` instead of the value (ratatui's `text_value`). The
    /// value still sets the bar's length.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// The value's style, over the chart's
    /// [`value_style`](BarChart::value_style).
    pub fn value_style(mut self, style: impl Into<String>) -> Self {
        self.value_style = Some(style.into());
        self
    }

    /// The label's style, over the chart's
    /// [`label_style`](BarChart::label_style).
    pub fn label_style(mut self, style: impl Into<String>) -> Self {
        self.label_style = Some(style.into());
        self
    }
}

/// Bars that belong together, such as one quarter's revenue for each
/// company, with a label: ratatui's `BarGroup`. A vertical chart writes the
/// label centred under the group; a horizontal one on the row after it.
///
/// ```
/// use rich_ext::chart::{Bar, BarChart, BarGroup};
///
/// let chart = BarChart::new()
///     .group(BarGroup::new("Q1").bar(Bar::new("a", 3.0)).bar(Bar::new("b", 5.0)))
///     .group(BarGroup::new("Q2").bars([Bar::new("a", 4.0), Bar::new("b", 2.0)]));
/// assert_eq!(chart.bars().len(), 4);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BarGroup {
    label: String,
    bars: Vec<Bar>,
}

impl BarGroup {
    /// A group called `label` (empty for none), without bars yet.
    pub fn new(label: impl Into<String>) -> Self {
        BarGroup {
            label: label.into(),
            bars: Vec::new(),
        }
    }

    /// Add a bar.
    pub fn bar(mut self, bar: Bar) -> Self {
        self.bars.push(bar);
        self
    }

    /// Add bars.
    pub fn bars(mut self, bars: impl IntoIterator<Item = Bar>) -> Self {
        self.bars.extend(bars);
        self
    }
}

/// A run of a chart's bars in one group.
#[derive(Clone, Debug, PartialEq)]
struct Group {
    label: String,
    len: usize,
    /// Made of bars added one by one, so the next one joins it.
    loose: bool,
}

/// Bars: by default horizontal, a label, a bar and the value on each line;
/// with [`Orientation::Vertical`], columns side by side.
///
/// - Lengths are measured from zero on a scale that runs from the smallest
///   to the largest value and always includes zero (override with
///   [`range`](Self::range)). Negative values extend left of zero, at
///   whole-cell precision, in `chart.negative`; their written value keeps
///   its minus sign.
/// - [`Charset::Blocks`] draws with eighth-cell precision (`█▉▊▋▌▍▎▏`),
///   [`Charset::Braille`] with half cells (`⣿⡇`), [`Charset::Ascii`] with
///   `#` and a half `=`. A value above zero always shows at least a sliver.
/// - It measures to the label, the bar ([`bar_width`](Self::bar_width),
///   40 by default) and the value. Given less, the bar shrinks to 4 cells,
///   then the values go, then the labels are cut (with `…`, or `.` in
///   ASCII), then the bar takes whatever is left.
/// - [`Orientation::Vertical`] draws columns bottom up, as tall as
///   [`bar_width`](Self::bar_width) rows, or the height it is given (a
///   `Layout` region, an intuiTUIve node) less its labels and values, or 8
///   rows. Blocks are eighth-cell (`▁▂▃▄▅▆▇█`), Braille half-cell (`⣤⣿`),
///   ASCII `#` and a half `.`. Each bar has a slot as wide as the widest
///   label or value (or its [`thickness`](Self::thickness)), with a cell
///   between slots ([`bar_gap`](Self::bar_gap)), and is up to 3 cells
///   thick unless a thickness is set. Labels go under the bars and each
///   value just above its bar; negative values hang below the zero line at
///   whole-cell precision, their values under them. Given less width the
///   values go, then the labels are cut (and dropped below two cells),
///   then the gaps, and bars that still do not fit are left off the right.
///
/// What ratatui's `BarChart` has, by our names:
///
/// - [`group`](Self::group) adds a [`BarGroup`]: bars with a label, set
///   apart by [`group_gap`](Self::group_gap) (1 cell vertical, 2 rows
///   horizontal, by default).
/// - [`thickness`](Self::thickness) is ratatui's `bar_width`: how many
///   cells wide a vertical bar is, or rows tall a horizontal one.
///   [`bar_width`](Self::bar_width) keeps its meaning here: the longest
///   bar's length, or the tallest column's height.
/// - [`bar_gap`](Self::bar_gap) is the space between bars (1 cell
///   vertical, no rows horizontal, by default).
/// - [`Bar::text`] writes text instead of the value (`76°`).
/// - [`label_style`](Self::label_style) and
///   [`value_style`](Self::value_style), and [`Bar::label_style`] and
///   [`Bar::value_style`] over them, style the labels and values.
/// - [`values_inside`](Self::values_inside) writes each value on its bar,
///   as ratatui draws them.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{BarChart, Charset};
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let chart = BarChart::new()
///     .bar("api", 31.0)
///     .bar("web", 12.5)
///     .bar("worker", 4.0)
///     .bar_width(8);
/// assert_eq!(
///     console.render_export(&chart),
///     "api    ████████   31\n\
///      web    ███▎     12.5\n\
///      worker █           4\n"
/// );
/// let ascii = chart.charset(Charset::Ascii);
/// assert_eq!(
///     console.render_export(&ascii),
///     "api    ########   31\n\
///      web    ###      12.5\n\
///      worker #           4\n"
/// );
/// ```
///
/// Vertical:
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{BarChart, Orientation};
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let chart = BarChart::from_pairs([("mon", 4.0), ("tue", 7.5), ("wed", -2.0)])
///     .orientation(Orientation::Vertical)
///     .bar_width(4);
/// assert_eq!(
///     console.render_export(&chart),
///     concat!(
///         "    7.5    \n",
///         " 4  ███    \n",
///         "▅▅▅ ███    \n",
///         "███ ███    \n",
///         "        ███\n",
///         "        -2 \n",
///         "mon tue wed\n",
///     )
/// );
/// ```
///
/// Grouped, with the values on the bars:
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Bar, BarChart, BarGroup, Orientation};
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let chart = BarChart::new()
///     .group(BarGroup::new("Q1").bar(Bar::new("a", 4.0)).bar(Bar::new("b", 2.0)))
///     .group(BarGroup::new("Q2").bar(Bar::new("a", 3.0)).bar(Bar::new("b", 1.0)))
///     .orientation(Orientation::Vertical)
///     .thickness(3)
///     .values_inside(true)
///     .bar_width(4);
/// assert_eq!(
///     console.render_export(&chart),
///     concat!(
///         "███             \n",
///         "███      ███    \n",
///         "███ ███  ███    \n",
///         "█4█ █2█  █3█ █1█\n",
///         " a   b    a   b \n",
///         "  Q1       Q2   \n",
///     )
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct BarChart {
    bars: Vec<Bar>,
    groups: Vec<Group>,
    min: Option<f64>,
    max: Option<f64>,
    charset: Charset,
    bar_width: Option<usize>,
    show_values: bool,
    format: ValueFormat,
    style: Option<String>,
    orientation: Orientation,
    thickness: Option<usize>,
    bar_gap: Option<usize>,
    group_gap: Option<usize>,
    label_style: Option<String>,
    value_style: Option<String>,
    values_inside: bool,
}

impl Default for BarChart {
    fn default() -> Self {
        Self::new()
    }
}

impl BarChart {
    /// An empty chart; add rows with [`bar`](Self::bar).
    pub fn new() -> Self {
        BarChart {
            bars: Vec::new(),
            groups: Vec::new(),
            min: None,
            max: None,
            charset: Charset::Auto,
            bar_width: None,
            show_values: true,
            format: ValueFormat::Compact,
            style: None,
            orientation: Orientation::Horizontal,
            thickness: None,
            bar_gap: None,
            group_gap: None,
            label_style: None,
            value_style: None,
            values_inside: false,
        }
    }

    /// A chart of `(label, value)` pairs.
    pub fn from_pairs<L: Into<String>>(pairs: impl IntoIterator<Item = (L, f64)>) -> Self {
        pairs
            .into_iter()
            .fold(Self::new(), |chart, (label, value)| chart.bar(label, value))
    }

    /// Add a bar.
    pub fn bar(self, label: impl Into<String>, value: f64) -> Self {
        self.push(Bar::new(label, value))
    }

    /// Add a [`Bar`] built with its own style, text or value style.
    pub fn push(mut self, bar: Bar) -> Self {
        self.bars.push(bar);
        match self.groups.last_mut() {
            Some(group) if group.loose => group.len += 1,
            Some(_) => self.groups.push(Group {
                label: String::new(),
                len: 1,
                loose: true,
            }),
            None => {}
        }
        self
    }

    /// Add a [`BarGroup`]: its bars, set apart from the others, with its
    /// label. A group without bars is left out, as in ratatui.
    pub fn group(mut self, group: BarGroup) -> Self {
        if group.bars.is_empty() {
            return self;
        }
        if self.groups.is_empty() && !self.bars.is_empty() {
            self.groups.push(Group {
                label: String::new(),
                len: self.bars.len(),
                loose: false,
            });
        }
        self.groups.push(Group {
            label: group.label,
            len: group.bars.len(),
            loose: false,
        });
        self.bars.extend(group.bars);
        self
    }

    /// The bars, in order, across their groups.
    pub fn bars(&self) -> &[Bar] {
        &self.bars
    }

    /// Fix the scale (default: the values' range, stretched to include
    /// zero). Values outside are clamped.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Fix only the top of the scale, such as `100.0` for percentages.
    pub fn max(mut self, max: f64) -> Self {
        self.max = Some(max);
        self
    }

    /// Glyphs to draw with (default [`Charset::Auto`]: blocks).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// Which way the bars run (default [`Orientation::Horizontal`]).
    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Cells for the longest bar (default 40, at least 1); rows for the
    /// tallest when [`Orientation::Vertical`] (default: the height given,
    /// less labels and values, or 8; at most 65535). For how thick a bar
    /// is, ratatui's `bar_width`, see [`thickness`](Self::thickness).
    pub fn bar_width(mut self, width: usize) -> Self {
        self.bar_width = Some(width.max(1));
        self
    }

    /// How thick each bar is (ratatui's `bar_width`): cells across a
    /// vertical bar, rows down a horizontal one (at least 1). By default a
    /// horizontal bar is a row and a vertical one as thick as its slot, up
    /// to 3 cells. A horizontal bar's label and value sit on its middle
    /// row.
    pub fn thickness(mut self, thickness: usize) -> Self {
        self.thickness = Some(thickness.clamp(1, MAX_SIDE));
        self
    }

    /// The space between two bars: cells when vertical (default 1), rows
    /// when horizontal (default none).
    pub fn bar_gap(mut self, gap: usize) -> Self {
        self.bar_gap = Some(gap.min(MAX_SIDE));
        self
    }

    /// The space between two groups, on top of the
    /// [`bar_gap`](Self::bar_gap): cells when vertical (default 1), rows
    /// when horizontal (default 2). A horizontal group's label takes the
    /// first of those rows, or a row of its own when there are none.
    pub fn group_gap(mut self, gap: usize) -> Self {
        self.group_gap = Some(gap.min(MAX_SIDE));
        self
    }

    /// Write each value after its bar (default on).
    pub fn show_values(mut self, show: bool) -> Self {
        self.show_values = show;
        self
    }

    /// Write each value on its bar, as ratatui draws values: from the start
    /// of a horizontal bar, past its end when it is longer; on the bottom
    /// row of a vertical one, centred and cut to its thickness. Values on
    /// bars take no room of their own. Without a value style they are the
    /// bar's colour reversed, so they read on the bar.
    pub fn values_inside(mut self, inside: bool) -> Self {
        self.values_inside = inside;
        self
    }

    /// How values are written (default [`ValueFormat::Compact`]).
    pub fn format(mut self, format: ValueFormat) -> Self {
        self.format = format;
        self
    }

    /// The style of every bar: a theme key or a definition (default
    /// `chart.bar`).
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// The labels' style, over `chart.label`: a theme key or a definition.
    /// It styles the group labels too.
    pub fn label_style(mut self, style: impl Into<String>) -> Self {
        self.label_style = Some(style.into());
        self
    }

    /// The values' style, over `chart.value`: a theme key or a definition.
    pub fn value_style(mut self, style: impl Into<String>) -> Self {
        self.value_style = Some(style.into());
        self
    }

    /// The groups, each a label (empty for none) and its bars' indices.
    /// Without groups, all the bars are one.
    fn spans(&self) -> Vec<(&str, Range<usize>)> {
        let mut out = Vec::new();
        let mut at = 0;
        for group in &self.groups {
            let end = (at + group.len).min(self.bars.len());
            out.push((group.label.as_str(), at..end));
            at = end;
        }
        if at < self.bars.len() {
            out.push(("", at..self.bars.len()));
        }
        out.retain(|(_, range)| !range.is_empty());
        out
    }

    fn scale(&self) -> Scale {
        let values = self.bars.iter().map(|b| b.value);
        let data = Scale::from_values(values);
        let data = if self.bars.iter().any(|b| b.value.is_finite()) {
            data.include_zero()
        } else {
            data
        };
        data.bounds(self.min, self.max)
    }

    fn label_width(&self) -> usize {
        self.bars.iter().map(|b| cells(&b.label)).max().unwrap_or(0)
    }

    /// What a bar's value is written as.
    fn value_text(&self, bar: &Bar) -> String {
        bar.text
            .clone()
            .unwrap_or_else(|| self.format.format(bar.value))
    }

    fn value_width(&self) -> usize {
        if !self.show_values {
            return 0;
        }
        self.bars
            .iter()
            .map(|b| cells(&self.value_text(b)))
            .max()
            .unwrap_or(0)
    }

    /// The room values take beside or above the bars: none when they are
    /// on the bars.
    fn value_room(&self) -> usize {
        if self.values_inside {
            0
        } else {
            self.value_width()
        }
    }

    fn natural_bar(&self) -> usize {
        self.bar_width.unwrap_or(match self.orientation {
            Orientation::Horizontal => DEFAULT_BAR,
            Orientation::Vertical => DEFAULT_COLUMN,
        })
    }

    fn gap(&self) -> usize {
        self.bar_gap.unwrap_or(match self.orientation {
            Orientation::Horizontal => 0,
            Orientation::Vertical => 1,
        })
    }

    /// The space between groups, on top of the gap; none with one group.
    fn group_space(&self, groups: usize) -> usize {
        if groups < 2 {
            return 0;
        }
        self.group_gap.unwrap_or(match self.orientation {
            Orientation::Horizontal => 2,
            Orientation::Vertical => 1,
        })
    }

    /// The vertical layout for `width` cells.
    fn columns(&self, width: usize) -> Columns {
        let n = self.bars.len().max(1);
        let groups = self.spans().len().max(1);
        let label = self.label_width();
        let value = self.value_room();
        let gap = self.gap();
        let group_gap = self.group_space(groups);
        let floor = self.thickness.unwrap_or(1);
        let gaps = (n - 1)
            .saturating_mul(gap)
            .saturating_add((groups - 1).saturating_mul(group_gap));
        let fits = |slot: usize| n.saturating_mul(slot).saturating_add(gaps) <= width;
        let full = |slot: usize, labels: bool, values: bool| Columns {
            slot,
            gap,
            group_gap,
            thickness: self
                .thickness
                .map_or(slot.min(MAX_THICKNESS), |t| t.min(slot)),
            labels,
            values,
            shown: n,
        };
        let values = self.value_width() > 0;
        let with_values = label.max(value).max(floor);
        if fits(with_values) {
            return full(with_values, label > 0, values);
        }
        let without = label.max(floor);
        if fits(without) {
            return full(without, label > 0, values && self.values_inside);
        }
        if width >= gaps.saturating_add(n) {
            let slot = (width - gaps) / n;
            // A label cut to one cell would be only the `…`.
            return full(slot, label > 0 && slot > 1, false);
        }
        Columns {
            slot: 1,
            gap: 0,
            group_gap: 0,
            thickness: 1,
            labels: false,
            values: false,
            shown: n.min(width.max(1)),
        }
    }

    /// A vertical bar's cells, bottom up, for `length` rows (a fraction
    /// of a row allowed).
    fn column_glyphs(charset: Charset, length: f64, sliver: bool) -> Vec<char> {
        let full = length.floor() as usize;
        let rest = length - full as f64;
        let mut out = Vec::new();
        match charset {
            Charset::Ascii | Charset::Braille => {
                let (whole, half) = if charset == Charset::Ascii {
                    (ASCII_FULL, ASCII_LOWER_HALF)
                } else {
                    (BRAILLE_FULL, BRAILLE_LOWER_HALF)
                };
                let halves = (rest * 2.0).round() as usize;
                out.extend(std::iter::repeat_n(whole, full + halves / 2));
                if halves == 1 || (out.is_empty() && sliver) {
                    out.push(half);
                }
            }
            _ => {
                let eighths = (rest * 8.0).round() as usize;
                out.extend(std::iter::repeat_n(FULL_BLOCK, full + eighths / 8));
                if !eighths.is_multiple_of(8) {
                    out.push(LOWER_EIGHTHS[eighths % 8 - 1]);
                } else if out.is_empty() && sliver {
                    out.push(LOWER_EIGHTHS[0]);
                }
            }
        }
        out
    }

    fn bar_style(&self, console: &Console, bar: &Bar, negative: bool) -> Style {
        match (&bar.style, &self.style) {
            (Some(s), _) => user_style(console, s),
            (None, _) if negative => theme_style(console, "chart.negative"),
            (None, Some(s)) => user_style(console, s),
            (None, None) => theme_style(console, "chart.bar"),
        }
    }

    /// `base` with the chart's style spec and then the bar's over it.
    fn layered(
        console: &Console,
        base: &str,
        chart: &Option<String>,
        bar: Option<&Option<String>>,
    ) -> Style {
        let mut style = theme_style(console, base);
        for spec in [Some(chart), bar].into_iter().flatten().flatten() {
            style = style.combine(&user_style(console, spec));
        }
        style
    }

    fn label_style_of(&self, console: &Console, bar: &Bar) -> Style {
        Self::layered(
            console,
            "chart.label",
            &self.label_style,
            Some(&bar.label_style),
        )
    }

    fn value_style_of(&self, console: &Console, bar: &Bar) -> Style {
        Self::layered(
            console,
            "chart.value",
            &self.value_style,
            Some(&bar.value_style),
        )
    }

    /// The style of a value written on its bar: the value styles when any
    /// is set, the bar's reversed otherwise.
    fn inside_style_of(&self, console: &Console, bar: &Bar, bar_style: &Style) -> Style {
        if self.value_style.is_none() && bar.value_style.is_none() {
            bar_style.combine(&Style::parse("reverse").unwrap_or_default())
        } else {
            self.value_style_of(console, bar)
        }
    }

    fn group_style(&self, console: &Console) -> Style {
        Self::layered(console, "chart.label", &self.label_style, None)
    }

    /// The vertical chart for `width` cells, as tall as `bar_width`, or as
    /// `height` less its labels and values, or 8 rows.
    fn vertical_fit(
        &self,
        console: &Console,
        width: usize,
        charset: Charset,
        height: Option<usize>,
    ) -> Vec<Line> {
        match (self.bar_width, height) {
            (Some(rows), _) => self.vertical_lines(console, width, charset, rows.min(MAX_COLUMN)),
            (None, None) => self.vertical_lines(console, width, charset, DEFAULT_COLUMN),
            (None, Some(total)) => {
                let total = total.min(MAX_COLUMN);
                let mut rows = total.max(1);
                loop {
                    let lines = self.vertical_lines(console, width, charset, rows);
                    if lines.len() <= total || rows == 1 {
                        return lines;
                    }
                    rows -= (lines.len() - total).min(rows - 1);
                }
            }
        }
    }

    fn vertical_lines(
        &self,
        console: &Console,
        width: usize,
        charset: Charset,
        height: usize,
    ) -> Vec<Line> {
        let ascii = charset == Charset::Ascii;
        let colour = has_colour(console);
        let layout = self.columns(width);
        let spans = self.spans();
        // Each shown bar's left column.
        let mut lefts = Vec::with_capacity(layout.shown);
        for (group, (_, range)) in spans.iter().enumerate() {
            for index in range.clone().take_while(|i| *i < layout.shown) {
                lefts.push(index * (layout.slot + layout.gap) + group * layout.group_gap);
            }
        }
        let width = lefts.last().map_or(0, |l| l + layout.slot).min(width);
        let scale = self.scale();
        let zero = scale.normalize(0.0).unwrap_or(0.0) * height as f64;
        let zero_row = (zero.round() as usize).min(height);
        let full = match charset {
            Charset::Ascii => ASCII_FULL,
            Charset::Braille => BRAILLE_FULL,
            _ => FULL_BLOCK,
        };

        // Rows counted from the bottom of the plot; a value may sit one row
        // above it or one below.
        let (bottom, top) = (-1i64, height as i64);
        let rows = (top - bottom + 1) as usize;
        let mut grid: Vec<Vec<(String, Option<Style>)>> =
            vec![vec![(" ".to_string(), None); width]; rows];
        let mut put = |row: i64, col: usize, cell: &str, style: Option<Style>| {
            if (bottom..=top).contains(&row) && col < width {
                grid[(top - row) as usize][col] = (cell.to_string(), style);
            }
        };
        let mut used = (0i64, height as i64 - 1);
        for (index, bar) in self.bars.iter().take(layout.shown).enumerate() {
            let left = lefts[index];
            let bar_left = left + (layout.slot - layout.thickness) / 2;
            let negative = bar.value < 0.0;
            let style = colour.then(|| self.bar_style(console, bar, negative));
            // Draw the bar; the row its value goes in, and the row of its
            // base (next to zero), when it has one.
            let (value_row, base_row) = match scale.normalize(bar.value) {
                None => (zero_row as i64, None),
                Some(n) if negative => {
                    let start = (n * height as f64).round() as usize;
                    let len = zero_row.saturating_sub(start).max(1).min(zero_row);
                    let glyph = full.to_string();
                    for row in zero_row - len..zero_row {
                        for col in bar_left..bar_left + layout.thickness {
                            put(row as i64, col, &glyph, style.clone());
                        }
                    }
                    let base = (len > 0).then_some(zero_row as i64 - 1);
                    ((zero_row - len) as i64 - 1, base)
                }
                Some(n) => {
                    let length = (n * height as f64 - zero).max(0.0);
                    let glyphs = Self::column_glyphs(charset, length, bar.value > 0.0);
                    let drawn = glyphs.len().min(height - zero_row);
                    for (i, glyph) in glyphs.iter().take(drawn).enumerate() {
                        let glyph = glyph.to_string();
                        for col in bar_left..bar_left + layout.thickness {
                            put((zero_row + i) as i64, col, &glyph, style.clone());
                        }
                    }
                    let base = (drawn > 0).then_some(zero_row as i64);
                    ((zero_row + drawn) as i64, base)
                }
            };
            if !layout.values {
                continue;
            }
            let value = truncate(&self.value_text(bar), usize::MAX, ascii);
            if self.values_inside {
                // On the bar's base, centred and cut to its thickness.
                let Some(row) = base_row else {
                    continue;
                };
                let text = if cells(&value) <= layout.thickness {
                    value
                } else if layout.thickness > 1 {
                    truncate(&value, layout.thickness, ascii)
                } else {
                    continue;
                };
                let start = bar_left + (layout.thickness - cells(&text)) / 2;
                let value_style = colour.then(|| {
                    self.inside_style_of(console, bar, style.as_ref().unwrap_or(&Style::new()))
                });
                for (i, unit) in cell_units(&text).iter().enumerate() {
                    put(row, start + i, unit, value_style.clone());
                }
            } else {
                let start = left + (layout.slot - cells(&value)) / 2;
                let value_style = colour.then(|| self.value_style_of(console, bar));
                for (i, unit) in cell_units(&value).iter().enumerate() {
                    put(value_row, start + i, unit, value_style.clone());
                }
                used = (used.0.min(value_row), used.1.max(value_row));
            }
        }

        // Only the rows above and below the plot that hold a value stay.
        let mut out: Vec<Line> = grid
            .into_iter()
            .enumerate()
            .filter(|(i, _)| (used.0..=used.1).contains(&(top - *i as i64)))
            .map(|(_, row)| {
                let mut line = Line::new();
                for (cell, style) in row {
                    line.push(&cell, style);
                }
                line
            })
            .collect();
        if layout.labels {
            let mut line = Line::new();
            for (index, bar) in self.bars.iter().take(layout.shown).enumerate() {
                line.pad(lefts[index].saturating_sub(line.width()));
                let label = truncate(&bar.label, layout.slot, ascii);
                let pad = layout.slot - cells(&label);
                line.pad(pad / 2);
                line.push(&label, colour.then(|| self.label_style_of(console, bar)));
                line.pad(pad - pad / 2);
            }
            line.pad(width.saturating_sub(line.width()));
            out.push(line);
        }
        if spans.iter().any(|(label, _)| !label.is_empty()) {
            let style = colour.then(|| self.group_style(console));
            let mut line = Line::new();
            for (label, range) in &spans {
                let shown: Vec<usize> = range.clone().take_while(|i| *i < layout.shown).collect();
                let (Some(&first), Some(&last)) = (shown.first(), shown.last()) else {
                    continue;
                };
                let (start, end) = (lefts[first], lefts[last] + layout.slot);
                let room = end - start;
                if room < 2 && cells(label) > room {
                    continue;
                }
                let text = truncate(label, room, ascii);
                line.pad(start.saturating_sub(line.width()));
                let pad = room - cells(&text);
                line.pad(pad / 2);
                line.push(&text, style.clone());
                line.pad(pad - pad / 2);
            }
            line.pad(width.saturating_sub(line.width()));
            out.push(line);
        }
        out
    }

    /// (label, bar, value) widths for `width` cells, after shrinking.
    fn layout(&self, width: usize) -> (usize, usize, usize) {
        let mut label = self.label_width();
        let mut value = self.value_room();
        let gap = |w: usize| usize::from(w > 0);
        let fixed = |l: usize, v: usize| l + gap(l) + v + gap(v);
        let natural = self.natural_bar();
        let floor = MIN_BAR.min(natural);
        if width >= fixed(label, value) + floor {
            let bar = natural.min(width - fixed(label, value));
            return (label, bar, value);
        }
        value = 0;
        if width >= fixed(label, 0) + floor {
            let bar = natural.min(width - fixed(label, 0));
            return (label, bar, value);
        }
        // Cut labels down, leaving the bar its floor.
        label = width.saturating_sub(floor + 1).min(label);
        let bar = width
            .saturating_sub(fixed(label, 0))
            .max(1)
            .min(width.max(1));
        (label, bar, value)
    }

    /// The glyphs for a bar of `length` cells (a fraction of a cell
    /// allowed).
    pub(crate) fn glyphs(charset: Charset, length: f64, sliver: bool) -> String {
        let full = length.floor() as usize;
        let rest = length - full as f64;
        let mut out = String::new();
        match charset {
            Charset::Ascii | Charset::Braille => {
                let (whole, half) = if charset == Charset::Ascii {
                    (ASCII_FULL, ASCII_HALF)
                } else {
                    (BRAILLE_FULL, BRAILLE_HALF)
                };
                let halves = (rest * 2.0).round() as usize;
                let full = full + halves / 2;
                out.extend(std::iter::repeat_n(whole, full));
                if halves == 1 {
                    out.push(half);
                }
                if out.is_empty() && sliver {
                    out.push(half);
                }
            }
            _ => {
                let eighths = (rest * 8.0).round() as usize;
                let full = full + eighths / 8;
                out.extend(std::iter::repeat_n(FULL_BLOCK, full));
                if !eighths.is_multiple_of(8) {
                    out.push(EIGHTHS[eighths % 8 - 1]);
                }
                if out.is_empty() && sliver {
                    out.push(EIGHTHS[0]);
                }
            }
        }
        out
    }

    /// A horizontal bar's `bar_w` cells, where it starts, and how many
    /// whole cells it covers.
    fn bar_cells(
        &self,
        bar: &Bar,
        scale: &Scale,
        charset: Charset,
        (bar_w, zero, zero_cell): (usize, f64, usize),
        style: Option<Style>,
    ) -> (Vec<(String, Option<Style>)>, usize, usize) {
        let blank = || (" ".to_string(), None);
        let mut row = vec![blank(); bar_w];
        match scale.normalize(bar.value) {
            None => (row, zero_cell.min(bar_w), 0),
            Some(n) if bar.value < 0.0 => {
                let start = (n * bar_w as f64).round() as usize;
                let len = zero_cell.saturating_sub(start).max(1).min(zero_cell);
                let start = zero_cell - len;
                let glyph = match charset {
                    Charset::Ascii => ASCII_FULL,
                    Charset::Braille => BRAILLE_FULL,
                    _ => FULL_BLOCK,
                };
                for cell in &mut row[start..zero_cell.min(bar_w)] {
                    *cell = (glyph.to_string(), style.clone());
                }
                (row, start, len)
            }
            Some(n) => {
                let length = (n * bar_w as f64 - zero).max(0.0);
                let glyphs = Self::glyphs(charset, length, bar.value > 0.0);
                let start = zero_cell.min(bar_w);
                let drawn = cells(&glyphs).min(bar_w - start);
                for (i, glyph) in glyphs.chars().take(drawn).enumerate() {
                    row[start + i] = (glyph.to_string(), style.clone());
                }
                (row, start, (length.floor() as usize).min(drawn))
            }
        }
    }

    fn horizontal_lines(&self, console: &Console, width: usize, charset: Charset) -> Vec<Line> {
        let ascii = charset == Charset::Ascii;
        let colour = has_colour(console);
        let (label_w, bar_w, value_w) = self.layout(width);
        let scale = self.scale();
        let zero = scale.normalize(0.0).unwrap_or(0.0) * bar_w as f64;
        let zero_cell = zero.round() as usize;
        let thickness = self.thickness.unwrap_or(1);
        let middle = thickness / 2;
        let spans = self.spans();
        let gap = self.gap();
        let group_gap = self.group_space(spans.len());
        let mut out = Vec::new();
        for (group, (group_label, range)) in spans.iter().enumerate() {
            for (nth, bar) in self.bars[range.clone()].iter().enumerate() {
                if nth > 0 {
                    out.extend((0..gap).map(|_| Line::new()));
                }
                let negative = bar.value < 0.0;
                let style = colour.then(|| self.bar_style(console, bar, negative));
                let (cells_, start, whole) = self.bar_cells(
                    bar,
                    &scale,
                    charset,
                    (bar_w, zero, zero_cell),
                    style.clone(),
                );
                for row in 0..thickness {
                    let here = row == middle;
                    let mut line = Line::new();
                    if label_w > 0 {
                        if here {
                            let label = truncate(&bar.label, label_w, ascii);
                            let pad = label_w - cells(&label);
                            line.push(&label, colour.then(|| self.label_style_of(console, bar)));
                            line.pad(pad + 1);
                        } else {
                            line.pad(label_w + 1);
                        }
                    }
                    let mut cells_ = cells_.clone();
                    if here && self.values_inside && self.show_values && bar.value.is_finite() {
                        let text = truncate(&self.value_text(bar), usize::MAX, ascii);
                        let on_bar = colour.then(|| {
                            self.inside_style_of(
                                console,
                                bar,
                                style.as_ref().unwrap_or(&Style::new()),
                            )
                        });
                        let units = cell_units(&text);
                        for (i, unit) in units.iter().enumerate() {
                            let col = start + i;
                            if col >= bar_w {
                                break;
                            }
                            let wide =
                                units.get(i + 1).is_some_and(String::is_empty) && !unit.is_empty();
                            let cell_style = if i < whole {
                                on_bar.clone()
                            } else {
                                style.clone()
                            };
                            if wide && col + 1 >= bar_w {
                                cells_[col] = (" ".to_string(), cell_style);
                                break;
                            }
                            cells_[col] = (unit.clone(), cell_style);
                        }
                    }
                    for (cell, cell_style) in cells_ {
                        line.push(&cell, cell_style);
                    }
                    if value_w > 0 {
                        if here {
                            // Right-aligned, so the digits line up.
                            let value = truncate(&self.value_text(bar), value_w, ascii);
                            line.pad(1 + value_w - cells(&value));
                            line.push(&value, colour.then(|| self.value_style_of(console, bar)));
                        } else {
                            line.pad(1 + value_w);
                        }
                    }
                    out.push(line);
                }
            }
            // The group's label on the row after its bars, then the space
            // to the next group.
            let mut space = if group + 1 < spans.len() {
                gap + group_gap
            } else {
                0
            };
            if !group_label.is_empty() {
                let mut line = Line::new();
                if label_w > 0 {
                    line.pad(label_w + 1);
                }
                let room = bar_w + if value_w > 0 { value_w + 1 } else { 0 };
                line.push(
                    &truncate(group_label, room, ascii),
                    colour.then(|| self.group_style(console)),
                );
                out.push(line);
                space = space.saturating_sub(1);
            }
            out.extend((0..space).map(|_| Line::new()));
        }
        out
    }

    pub(crate) fn render_lines(&self, console: &Console, options: &ConsoleOptions) -> Vec<Line> {
        let width = options.max_width;
        let charset = self.charset.resolve(console, options, Charset::Blocks);
        let ascii = charset == Charset::Ascii;
        if self.bars.is_empty() {
            let mut line = Line::new();
            line.push(&truncate("no data", width, ascii), None);
            return vec![line];
        }
        match self.orientation {
            Orientation::Vertical => self.vertical_fit(console, width, charset, options.height),
            Orientation::Horizontal => self.horizontal_lines(console, width, charset),
        }
    }

    fn natural_width(&self) -> usize {
        if self.orientation == Orientation::Vertical {
            let n = self.bars.len();
            let groups = self.spans().len().max(1);
            let slot = self
                .label_width()
                .max(self.value_room())
                .max(self.thickness.unwrap_or(1));
            let gaps = n
                .saturating_sub(1)
                .saturating_mul(self.gap())
                .saturating_add((groups - 1).saturating_mul(self.group_space(groups)));
            return n.saturating_mul(slot).saturating_add(gaps);
        }
        let l = self.label_width();
        let v = self.value_room();
        (l + usize::from(l > 0) + v + usize::from(v > 0)).saturating_add(self.natural_bar())
    }
}

impl Renderable for BarChart {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        lines_to_segments(self.render_lines(console, options), options.max_width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        if self.bars.is_empty() {
            return Measurement::new(7, 7).with_maximum(options.max_width);
        }
        let max = self.natural_width();
        let min = match self.orientation {
            Orientation::Horizontal => MIN_BAR,
            Orientation::Vertical => self.bars.len(),
        };
        Measurement::new(min.min(max), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// `Bar chart, 4 bars, values from 12 to 412`, then every bar (up to six)
/// or the three largest and the smallest. A grouped bar is named with its
/// group's label first.
impl crate::a11y::AccessibleText for BarChart {
    fn accessible_text(&self, _width: usize) -> String {
        self.facts("Bar chart", ("bar", "bars"))
    }
}

impl BarChart {
    /// The scale's range (zero included, fixed bounds kept), when there is
    /// a value or a bound: what a shared scale unifies.
    pub(crate) fn value_extent(&self) -> Option<(f64, f64)> {
        let any = self.bars.iter().any(|b| b.value.is_finite());
        if !any && self.min.is_none() && self.max.is_none() {
            return None;
        }
        let scale = self.scale();
        Some((scale.min(), scale.max()))
    }

    /// `what`, the count, the range and the bars ranked.
    fn facts(&self, what: &str, noun: (&str, &str)) -> String {
        use super::describe::{counted, extent, range, ranked};
        if self.bars.is_empty() {
            return format!("{what}, no {}", noun.1);
        }
        let mut head = format!("{what}, {}", counted(self.bars.len(), noun.0, noun.1));
        let groups: Vec<(&str, Range<usize>)> = self
            .spans()
            .into_iter()
            .filter(|_| !self.groups.is_empty())
            .collect();
        if groups.len() > 1 {
            head.push_str(&format!(" in {} groups", groups.len()));
        }
        if let Some((lo, hi)) = extent(self.bars.iter().map(|b| b.value)) {
            head.push_str(&format!(", values {}", range(lo, hi, self.format)));
        }
        let missing = self.bars.iter().filter(|b| !b.value.is_finite()).count();
        if missing > 0 {
            head.push_str(&format!(", {} without a value", super::describe::count(missing)));
        }
        let group_of = |i: usize| {
            groups
                .iter()
                .find(|(_, r)| r.contains(&i))
                .map_or("", |(label, _)| label)
        };
        let entries: Vec<(String, String, f64)> = self
            .bars
            .iter()
            .enumerate()
            .map(|(i, bar)| {
                let name = match group_of(i) {
                    "" => bar.label.clone(),
                    group => format!("{group} {}", bar.label).trim_end().to_string(),
                };
                let text = if bar.value.is_finite() {
                    self.value_text(bar)
                } else {
                    "no value".to_string()
                };
                (name, text, bar.value)
            })
            .collect();
        let mut out = vec![head];
        out.extend(ranked(&entries, 6));
        out.join("\n")
    }
}

/// Raw values counted into equal-width bins and drawn as a [`BarChart`],
/// one bar per bin labelled with its range.
///
/// Bins split the values' range (or [`range`](Self::range)) into
/// [`bins`](Self::bins) equal parts (10 by default). Each label is
/// `[low, high)`, the last `[low, high]`, so every value falls in exactly
/// one, the one its label names; NaN and infinite values are not counted,
/// and values outside an explicit range are left out. The edges are
/// written exactly: in full (`[1000, 1250)`) when the compact form would
/// round one of them.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::Histogram;
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let latencies = [12.0, 14.0, 15.0, 21.0, 22.0, 23.0, 24.0, 38.0];
/// let histogram = Histogram::new(latencies).bins(3).range(10.0, 40.0).bar_width(8);
/// assert_eq!(
///     console.render_export(&histogram),
///     "[10, 20) ██████   3\n\
///      [20, 30) ████████ 4\n\
///      [30, 40] ██       1\n"
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    values: Vec<f64>,
    bins: usize,
    min: Option<f64>,
    max: Option<f64>,
    chart: BarChart,
}

impl Histogram {
    /// A histogram of `values`.
    pub fn new(values: impl IntoIterator<Item = f64>) -> Self {
        Histogram {
            values: values.into_iter().collect(),
            bins: 10,
            min: None,
            max: None,
            chart: BarChart::new(),
        }
    }

    /// How many bins (default 10, at least 1).
    pub fn bins(mut self, bins: usize) -> Self {
        self.bins = bins.max(1);
        self
    }

    /// The range the bins cover (default: the values' range).
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Glyphs to draw with (default [`Charset::Auto`]: blocks).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.chart = self.chart.charset(charset);
        self
    }

    /// Which way the bars run (default [`Orientation::Horizontal`]).
    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.chart = self.chart.orientation(orientation);
        self
    }

    /// Cells for the tallest bin (default 40); rows when vertical
    /// (default 8).
    pub fn bar_width(mut self, width: usize) -> Self {
        self.chart = self.chart.bar_width(width);
        self
    }

    /// Write each count after its bar (default on).
    pub fn show_values(mut self, show: bool) -> Self {
        self.chart = self.chart.show_values(show);
        self
    }

    /// The bars' style: a theme key or a definition (default `chart.bar`).
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.chart = self.chart.style(style);
        self
    }

    /// The bin edges, `bins + 1` of them, low to high, without
    /// floating-point noise (`0.3`, not `0.30000000000000004`).
    ///
    /// With an explicit [`range`](Self::range) the bins split it exactly.
    /// Otherwise the bin width is rounded up to 1, 2, 2.5 or 5 times a power
    /// of ten and the first edge down to a multiple of it, so the labels are
    /// round numbers; the last bins may then be empty. A range too wide for
    /// round bins (near `±f64::MAX`) is split exactly.
    pub fn edges(&self) -> Vec<f64> {
        let data = Scale::from_values(self.values.iter().copied());
        let bins = self.bins as f64;
        let split = |scale: Scale| -> Vec<f64> {
            (0..=self.bins)
                .map(|i| clean(scale.lerp(i as f64 / bins)))
                .collect()
        };
        if self.min.is_some() || self.max.is_some() {
            let scale = data.bounds(self.min, self.max);
            let step = scale.span() / bins;
            if (scale.max() - scale.min()).is_finite() {
                return (0..=self.bins)
                    .map(|i| clean(scale.min() + step * i as f64))
                    .collect();
            }
            return split(scale);
        }
        let mut step = bin_step(data.span() / bins);
        let mut start = (data.min() / step).floor() * step;
        // Rounding the start down can leave the top uncovered: widen.
        while step.is_finite() && along(start, bins, step) < data.max() {
            step = bin_step(step * 1.001);
            start = (data.min() / step).floor() * step;
        }
        if !(start.is_finite() && along(start, bins, step).is_finite()) {
            return split(data);
        }
        (0..=self.bins)
            .map(|i| clean(along(start, i as f64, step)))
            .collect()
    }

    /// How many values fall in each bin: `[low, high)`, the last
    /// `[low, high]`, against the edges [`edges`](Self::edges) gives.
    pub fn counts(&self) -> Vec<usize> {
        let edges = self.edges();
        let (lo, hi) = (edges[0], edges[self.bins]);
        let inner = &edges[1..self.bins];
        let mut counts = vec![0; self.bins];
        for v in self.values.iter().copied().filter(|v| v.is_finite()) {
            if v < lo || v > hi {
                continue;
            }
            // The bins whose low edge is at or below `v`, past the first.
            let bin = inner.partition_point(|&edge| edge <= v);
            counts[bin] += 1;
        }
        counts
    }

    /// The [`BarChart`] this histogram draws.
    pub fn to_bar_chart(&self) -> BarChart {
        if !self.values.iter().any(|v| v.is_finite()) {
            return BarChart {
                bars: Vec::new(),
                ..self.chart.clone()
            };
        }
        let edges = self.edges();
        let written = edge_labels(&edges);
        let counts = self.counts();
        let mut chart = BarChart {
            bars: Vec::new(),
            ..self.chart.clone()
        };
        for (i, count) in counts.into_iter().enumerate() {
            let close = if i + 1 == self.bins { ']' } else { ')' };
            let label = format!("[{}, {}{close}", written[i], written[i + 1]);
            chart.bars.push(Bar::new(label, count as f64));
        }
        chart
    }
}

impl Renderable for Histogram {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.to_bar_chart().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.to_bar_chart().measure(console, options)
    }
}

/// The bin edges as the labels write them: exactly, in full when the
/// compact form would round one.
fn edge_labels(edges: &[f64]) -> Vec<String> {
    exact_labels(ValueFormat::Compact, edges)
        .map(|(labels, _)| labels)
        .unwrap_or_else(|| {
            edges
                .iter()
                .map(|&e| ValueFormat::Compact.format(e))
                .collect()
        })
}

/// `Histogram, 8 values in 3 bins from 10 to 40`, then each bin's count
/// (up to six bins) or the three largest bins and the smallest.
impl crate::a11y::AccessibleText for Histogram {
    fn accessible_text(&self, _width: usize) -> String {
        use super::describe::{count, counted, ranked};
        let finite = self.values.iter().filter(|v| v.is_finite()).count();
        let ignored = self.values.len() - finite;
        let mut head = format!("Histogram, {}", counted(finite, "value", "values"));
        if finite == 0 {
            if ignored > 0 {
                head.push_str(&format!(", {} ignored", count(ignored)));
            }
            return head;
        }
        let written = edge_labels(&self.edges());
        let chart = self.to_bar_chart();
        head.push_str(&format!(
            " in {} from {} to {}",
            counted(self.bins, "bin", "bins"),
            written[0],
            written[self.bins]
        ));
        if ignored > 0 {
            head.push_str(&format!(", {} ignored", count(ignored)));
        }
        let entries: Vec<(String, String, f64)> = chart
            .bars
            .iter()
            .map(|b| (b.label.clone(), count(b.value as usize), b.value))
            .collect();
        let mut out = vec![head];
        out.extend(ranked(&entries, 6));
        out.join("\n")
    }
}

/// The smallest of 1, 2, 2.5 and 5 times a power of ten that is at least `x`.
fn bin_step(x: f64) -> f64 {
    if !(x.is_finite() && x > 0.0) {
        return 1.0;
    }
    let power = 10f64.powf(x.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .map(|m| m * power)
        .find(|step| *step >= x * (1.0 - 1e-12))
        .unwrap_or(10.0 * power)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(width: usize, chart: &BarChart) -> Vec<String> {
        let console = Console::builder().width(width).color_system(None).build();
        console
            .render_export(chart)
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn coloured(width: usize) -> Console {
        Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(rich::ColorSystem::Truecolor))
            .no_color(false)
            .build()
    }

    /// The style of the cell at `col` of a rendered line.
    fn style_at(line: &[Segment], col: usize) -> Option<Style> {
        let mut seen = 0;
        for segment in line {
            let n = cells(&segment.text);
            if col < seen + n {
                return segment.style.clone();
            }
            seen += n;
        }
        None
    }

    #[test]
    fn grouped_vertical_bars_have_group_labels() {
        let chart = BarChart::new()
            .group(
                BarGroup::new("Q1")
                    .bar(Bar::new("a", 2.0))
                    .bar(Bar::new("b", 4.0)),
            )
            .group(
                BarGroup::new("Q2")
                    .bar(Bar::new("a", 3.0))
                    .bar(Bar::new("b", 1.0)),
            )
            .orientation(Orientation::Vertical)
            .bar_width(2);
        assert_eq!(
            rows(40, &chart),
            vec!["  4  3  ", "2 █  ▄ 1", "█ █  █ ▄", "a b  a b", "Q1   Q2 "]
        );
        // Bars added on their own after a group make a group of their own.
        let mixed = BarChart::new()
            .bar("x", 1.0)
            .group(BarGroup::new("g").bar(Bar::new("y", 1.0)))
            .bar("z", 1.0)
            .orientation(Orientation::Vertical)
            .show_values(false)
            .max(1.0)
            .bar_width(1);
        assert_eq!(rows(40, &mixed), vec!["█  █  █", "x  y  z", "   g   "]);
        // An empty group is left out.
        let empty = BarChart::new().group(BarGroup::new("none"));
        assert!(empty.bars().is_empty());
        assert_eq!(rows(10, &empty), vec!["no data"]);
    }

    #[test]
    fn values_inside_horizontal_bars_as_demo2_draws_the_seasons() {
        let seasons = BarChart::new()
            .push(Bar::new("", 51.0).text("Winter 37-51"))
            .push(Bar::new("", 65.0).text("Spring 40-65"))
            .push(Bar::new("", 77.0).text("Summer 54-77"))
            .push(Bar::new("", 71.0).text("Fall 41-71").value_style("bold"))
            .bar_gap(1)
            .style("#203060")
            .value_style("grey70 on #203060")
            .values_inside(true)
            .bar_width(20);
        assert_eq!(
            rows(40, &seasons),
            vec![
                "Winter 37-51█▎      ",
                "                    ",
                "Spring 40-65████▉   ",
                "                    ",
                "Summer 54-77████████",
                "                    ",
                "Fall 41-71████████▌ ",
            ]
        );
        let console = coloured(40);
        let lines = console.render_lines(&seasons, &console.options(), false);
        assert_eq!(
            style_at(&lines[0], 0),
            Style::parse("grey70 on #203060").ok()
        );
        assert_eq!(style_at(&lines[0], 12), Style::parse("#203060").ok());
        assert_eq!(
            style_at(&lines[6], 0),
            Style::parse("grey70 on #203060 bold").ok()
        );
        // Text longer than its bar runs on past it in the bar's colour.
        let short = BarChart::new()
            .push(Bar::new("", 1.0).text("long text"))
            .push(Bar::new("", 10.0))
            .style("green")
            .values_inside(true)
            .bar_width(10);
        let lines = console.render_lines(&short, &console.options(), false);
        let text: String = lines[0].iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "long text ");
        assert_eq!(style_at(&lines[0], 0), Style::parse("green reverse").ok());
        assert_eq!(style_at(&lines[0], 4), Style::parse("green").ok());
    }

    #[test]
    fn text_values_wider_than_a_bar_are_centred_and_cut() {
        let chart = BarChart::new()
            .push(Bar::new("a", 4.0).text("1234"))
            .push(Bar::new("b", 4.0).text("7"))
            .orientation(Orientation::Vertical)
            .thickness(3)
            .values_inside(true)
            .max(4.0)
            .bar_width(2);
        assert_eq!(rows(40, &chart), vec!["███ ███", "12… █7█", " a   b "]);
        // A bar one cell thick has no room for a cut value: none is drawn.
        let thin = chart.clone().thickness(1);
        assert_eq!(rows(40, &thin), vec!["█ █", "█ 7", "a b"]);
        // Above the bars, the slot is as wide as the widest value.
        let above = chart.thickness(1).values_inside(false);
        assert_eq!(
            rows(40, &above),
            vec!["1234  7  ", " █    █  ", " █    █  ", " a    b  "]
        );
    }

    #[test]
    fn thick_bars() {
        let vertical = BarChart::from_pairs([("x", 2.0), ("y", 1.0)])
            .orientation(Orientation::Vertical)
            .thickness(5)
            .bar_width(2);
        assert_eq!(
            rows(40, &vertical),
            vec!["  2        ", "█████   1  ", "█████ █████", "  x     y  "]
        );
        // Horizontal: five rows, the label and value on the middle one.
        let horizontal = BarChart::new()
            .bar("a", 2.0)
            .bar("b", 1.0)
            .thickness(5)
            .bar_width(4);
        assert_eq!(
            rows(40, &horizontal),
            vec![
                "  ████  ",
                "  ████  ",
                "a ████ 2",
                "  ████  ",
                "  ████  ",
                "  ██    ",
                "  ██    ",
                "b ██   1",
                "  ██    ",
                "  ██    ",
            ]
        );
    }

    #[test]
    fn gaps_between_bars_and_groups() {
        let group = |a: &str, b: &str| BarGroup::new("").bars([Bar::new(a, 1.0), Bar::new(b, 1.0)]);
        let vertical = BarChart::new()
            .group(group("a", "b"))
            .group(group("c", "d"))
            .orientation(Orientation::Vertical)
            .bar_gap(2)
            .group_gap(3)
            .show_values(false)
            .max(1.0)
            .bar_width(1);
        assert_eq!(rows(40, &vertical), vec!["█  █     █  █", "a  b     c  d"]);
        let horizontal = BarChart::new()
            .group(
                BarGroup::new("one")
                    .bar(Bar::new("a", 1.0))
                    .bar(Bar::new("b", 2.0)),
            )
            .group(BarGroup::new("two").bar(Bar::new("c", 3.0)))
            .bar_gap(1)
            .group_gap(1)
            .show_values(false)
            .bar_width(3);
        assert_eq!(
            rows(40, &horizontal),
            vec![
                "a █  ",
                "     ",
                "b ██ ",
                "  one",
                "     ",
                "c ███",
                "  two"
            ]
        );
        // The default space between horizontal groups: the label's row
        // and one more.
        let default = horizontal.clone();
        let default = BarChart {
            bar_gap: None,
            group_gap: None,
            ..default
        };
        assert_eq!(
            rows(40, &default),
            vec!["a █  ", "b ██ ", "  one", "     ", "c ███", "  two"]
        );
    }

    #[test]
    fn labels_and_values_take_the_chart_and_bar_styles() {
        let chart = BarChart::new()
            .push(Bar::new("Mon", 76.0).text("76°").value_style("bold"))
            .push(Bar::new("Tue", 65.0).label_style("underline"))
            .label_style("yellow")
            .value_style("italic")
            .bar_width(10);
        let console = coloured(40);
        let lines = console.render_lines(&chart, &console.options(), false);
        assert_eq!(style_at(&lines[0], 0), Style::parse("yellow").ok());
        assert_eq!(
            style_at(&lines[1], 0),
            Style::parse("yellow underline").ok()
        );
        let value_at = |line: &[Segment], text: &str| {
            line.iter()
                .find(|s| s.text.contains(text))
                .and_then(|s| s.style.clone())
        };
        assert_eq!(value_at(&lines[0], "76°"), Style::parse("italic bold").ok());
        assert_eq!(value_at(&lines[1], "65"), Style::parse("italic").ok());
        // Text values are written in place of the number, and in ASCII on
        // an ASCII console.
        assert_eq!(rows(40, &chart)[0], "Mon ██████████ 76°");
        let ascii = Console::builder()
            .width(40)
            .color_system(None)
            .ascii_only(true)
            .build();
        assert_eq!(
            ascii.render_export(&chart).lines().next(),
            Some("Mon ########## 76?")
        );
    }

    #[test]
    fn vertical_bars_fill_the_height_they_are_given() {
        let chart = BarChart::from_pairs([("mon", 4.0), ("tue", 7.5), ("wed", -2.0)])
            .orientation(Orientation::Vertical);
        let console = Console::builder().width(40).color_system(None).build();
        let lines = console.render_lines(&chart, &console.options().update_height(7), false);
        let text: Vec<String> = lines
            .iter()
            .map(|l| l.iter().map(|s| s.text.as_str()).collect())
            .collect();
        // Four rows of bars and their values and labels: as `bar_width(4)`.
        assert_eq!(
            text,
            vec![
                "    7.5    ",
                " 4  ███    ",
                "▅▅▅ ███    ",
                "███ ███    ",
                "        ███",
                "        -2 ",
                "mon tue wed",
            ]
        );
        // `bar_width` wins; with no height at all, eight rows of bars.
        let fixed = chart.clone().bar_width(2);
        let lines = console.render_lines(&fixed, &console.options().update_height(7), false);
        assert_eq!(lines.iter().filter(|l| !l.is_empty()).count(), 5);
        assert_eq!(console.render_export(&chart).lines().count(), 11);
        // Horizontal charts keep a row a bar.
        let horizontal = BarChart::from_pairs([("a", 1.0)]);
        let lines = console.render_lines(&horizontal, &console.options().update_height(7), false);
        assert_eq!(lines.iter().filter(|l| !l.is_empty()).count(), 1);
    }
}
