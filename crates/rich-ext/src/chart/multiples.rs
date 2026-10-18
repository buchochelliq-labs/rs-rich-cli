//! Small multiples: a grid of charts on one scale, so they compare at a
//! glance.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment};

use super::{
    has_colour, lines_to_segments, theme_style, truncate, wrap_entries, BarChart, Charset,
    Chart, Line, LineChart, Scale, Sparkline, ValueFormat,
};

/// The narrowest cell [`SmallMultiples`] picks its columns by, by default.
const DEFAULT_MIN_WIDTH: usize = 24;
/// Cells between two columns, by default.
const DEFAULT_GAP: usize = 2;

/// One chart of a [`SmallMultiples`] grid: a chart whose value scale can
/// be shared. Each chart type converts with `.into()`.
///
/// | Facet | Its value scale | Fixed with |
/// |---|---|---|
/// | [`Sparkline`] | the heights | [`Sparkline::range`] |
/// | [`BarChart`] | the bars' lengths, from zero | [`BarChart::range`] |
/// | [`LineChart`] | the y axis | [`LineChart::y_range`] |
/// | [`Chart`] | the y axis | [`Axis::bounds`](super::Axis::bounds) |
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Facet {
    Sparkline(Sparkline),
    Bars(BarChart),
    Line(LineChart),
    Chart(Chart),
}

impl From<Sparkline> for Facet {
    fn from(chart: Sparkline) -> Self {
        Facet::Sparkline(chart)
    }
}

impl From<BarChart> for Facet {
    fn from(chart: BarChart) -> Self {
        Facet::Bars(chart)
    }
}

impl From<LineChart> for Facet {
    fn from(chart: LineChart) -> Self {
        Facet::Line(chart)
    }
}

impl From<Chart> for Facet {
    fn from(chart: Chart) -> Self {
        Facet::Chart(chart)
    }
}

impl Facet {
    /// The range its value scale shows on its own: the data's (from zero,
    /// for bars), with any bound it fixes. `None` without data.
    pub fn value_range(&self) -> Option<(f64, f64)> {
        match self {
            Facet::Sparkline(c) => c.value_extent(),
            Facet::Bars(c) => c.value_extent(),
            Facet::Line(c) => c.y_extent(),
            Facet::Chart(c) => c.y_extent(),
        }
    }

    /// This chart with its value scale fixed to `min..max`.
    pub fn with_range(self, min: f64, max: f64) -> Facet {
        match self {
            Facet::Sparkline(c) => Facet::Sparkline(c.range(min, max)),
            Facet::Bars(c) => Facet::Bars(c.range(min, max)),
            Facet::Line(c) => Facet::Line(c.y_range(min, max)),
            Facet::Chart(c) => Facet::Chart(c.with_y_bounds(min, max)),
        }
    }

    /// Whether its scale has written labels, so a shared range is best
    /// rounded out.
    fn has_axis(&self) -> bool {
        matches!(self, Facet::Line(_) | Facet::Chart(_))
    }

    fn renderable(&self) -> &dyn Renderable {
        match self {
            Facet::Sparkline(c) => c,
            Facet::Bars(c) => c,
            Facet::Line(c) => c,
            Facet::Chart(c) => c,
        }
    }
}

impl Renderable for Facet {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.renderable().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.renderable().measure(console, options)
    }
}

impl crate::a11y::AccessibleText for Facet {
    fn accessible_text(&self, width: usize) -> String {
        match self {
            Facet::Sparkline(c) => c.accessible_text(width),
            Facet::Bars(c) => c.accessible_text(width),
            Facet::Line(c) => c.accessible_text(width),
            Facet::Chart(c) => c.accessible_text(width),
        }
    }
}

/// A grid of charts, each with a title, on one value scale and fitted to
/// the width: small multiples, as Tufte named them.
///
/// ```text
/// api                       web                       worker
/// ▁▂▂▃▄▆▅▄▇█▇▅▄▆██▇▆▅▄▃▂▂▁   ▁▁▁▂▂▃▃▂▃▄▄▃▃▃▄▄▄▃▃▂▂▂▁▁   ▁▁▁▁▁▁▁▁▂▂▂▂▁▁▂▂▂▂▂▁▁▁▁▁
/// scale 12 to 52 in every chart
/// ```
///
/// - **Columns:** [`columns(n)`](Self::columns), or as many as fit cells
///   of at least [`min_cell_width`](Self::min_cell_width) (24 by default)
///   with [`gap`](Self::gap) cells between them (2 by default), and never
///   more than there are charts. Every cell is as wide; what is left over
///   stays at the right. Charts fill the grid row by row.
/// - **The shared scale:** every chart's value scale (a sparkline's
///   heights, the bars' lengths, a line chart's or a `Chart`'s y axis) is
///   fixed to the range of them all, so a taller bar is a larger value
///   whichever chart it is in. When a chart has axis labels the range is
///   rounded out to round numbers. [`range`](Self::range) fixes it;
///   [`shared_scale(false)`](Self::shared_scale) leaves each chart its own.
/// - **One note** under the grid says the scale (`scale 0 to 60 in every
///   chart`) and holds the one legend: the series of the line charts,
///   whose own legends are left out. [`note(false)`](Self::note) leaves it
///   out.
/// - **Height:** [`height(rows)`](Self::height) makes each chart that many
///   rows; without it, a grid given a height (a `Layout` region, an
///   intuiTUIve node) shares it among its rows of charts, less the titles,
///   the gaps and the note; printed on its own, each chart keeps its
///   default height.
/// - Titles are cut to the cell (with `…`, or `.` in ASCII) in
///   `chart.title`; a blank line separates the rows of charts.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{SmallMultiples, Sparkline};
///
/// let console = Console::builder().width(30).color_system(None).build();
/// let grid = SmallMultiples::new([
///     ("low", Sparkline::new([1.0, 2.0, 3.0])),
///     ("high", Sparkline::new([7.0, 8.0, 9.0])),
/// ])
/// .min_cell_width(10);
/// assert_eq!(
///     console.render_export(&grid),
///     concat!(
///         "low        high      \n",
///         "▁▂▃        ▆▇█       \n",
///         "scale 1 to 9 in every chart   \n",
///     )
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct SmallMultiples {
    items: Vec<(String, Facet)>,
    columns: Option<usize>,
    min_width: usize,
    gap: usize,
    shared: bool,
    range: Option<(f64, f64)>,
    height: Option<usize>,
    note: bool,
    charset: Charset,
}

impl Default for SmallMultiples {
    fn default() -> Self {
        Self::new(Vec::<(String, Facet)>::new())
    }
}

impl SmallMultiples {
    /// A grid of `(title, chart)` pairs, in order.
    pub fn new<T: Into<String>, F: Into<Facet>>(items: impl IntoIterator<Item = (T, F)>) -> Self {
        SmallMultiples {
            items: items
                .into_iter()
                .map(|(t, f)| (t.into(), f.into()))
                .collect(),
            columns: None,
            min_width: DEFAULT_MIN_WIDTH,
            gap: DEFAULT_GAP,
            shared: true,
            range: None,
            height: None,
            note: true,
            charset: Charset::Auto,
        }
    }

    /// Add a chart with its title.
    pub fn push(mut self, title: impl Into<String>, chart: impl Into<Facet>) -> Self {
        self.items.push((title.into(), chart.into()));
        self
    }

    /// Lay the charts out in `n` columns (at least 1), whatever the width.
    pub fn columns(mut self, n: usize) -> Self {
        self.columns = Some(n.max(1));
        self
    }

    /// The narrowest cell when the columns are picked by the width (default
    /// 24, at least 1).
    pub fn min_cell_width(mut self, cells: usize) -> Self {
        self.min_width = cells.max(1);
        self
    }

    /// Cells between two columns (default 2).
    pub fn gap(mut self, cells: usize) -> Self {
        self.gap = cells;
        self
    }

    /// Put every chart on one value scale (default on).
    pub fn shared_scale(mut self, shared: bool) -> Self {
        self.shared = shared;
        self
    }

    /// Fix the shared scale (default: the range of every chart's values).
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.range = Some((min, max));
        self
    }

    /// Each chart's height in rows (at least 1), its title not counted.
    pub fn height(mut self, rows: usize) -> Self {
        self.height = Some(rows.max(1));
        self
    }

    /// Write the note under the grid: the shared scale and the legend
    /// (default on).
    pub fn note(mut self, show: bool) -> Self {
        self.note = show;
        self
    }

    /// Glyphs for the titles and the note: [`Charset::Ascii`] asks for
    /// ASCII (an ASCII-only console always gets it). Each chart keeps its
    /// own charset.
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// The charts, with their titles.
    pub fn items(&self) -> &[(String, Facet)] {
        &self.items
    }

    /// How many columns the grid has at `width` cells.
    pub fn columns_at(&self, width: usize) -> usize {
        let n = self.items.len().max(1);
        let gap = self.gap;
        let wanted = self
            .columns
            .unwrap_or_else(|| ((width + gap) / (self.min_width + gap)).max(1));
        // Never so many that a cell has no room.
        let fits = ((width + gap) / (1 + gap)).max(1);
        wanted.min(n).min(fits)
    }

    /// The shared scale: the fixed range, else every chart's range (rounded
    /// out when a chart has axis labels). `None` when not shared or there
    /// is no data.
    pub fn scale(&self) -> Option<(f64, f64)> {
        if !self.shared {
            return None;
        }
        if let Some((a, b)) = self.range {
            let s = Scale::new(a, b);
            return Some((s.min(), s.max()));
        }
        let ends = self
            .items
            .iter()
            .filter_map(|(_, f)| f.value_range())
            .flat_map(|(lo, hi)| [lo, hi]);
        let (lo, hi) = super::describe::extent(ends)?;
        let scale = Scale::new(lo, hi);
        let scale = if self.items.iter().any(|(_, f)| f.has_axis()) {
            scale.nice(5)
        } else {
            scale
        };
        Some((scale.min(), scale.max()))
    }

    /// The charts as drawn: on the shared scale, line charts without their
    /// own legends.
    fn facets(&self) -> Vec<Facet> {
        let scale = self.scale();
        self.items
            .iter()
            .map(|(_, f)| {
                let f = match scale {
                    Some((lo, hi)) => f.clone().with_range(lo, hi),
                    None => f.clone(),
                };
                match f {
                    Facet::Line(c) if self.note => Facet::Line(c.legend(false)),
                    f => f,
                }
            })
            .collect()
    }

    /// The legend's entries: each line chart series' marker and name, by
    /// position, from the first chart that names it.
    fn legend(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        ascii: bool,
    ) -> Vec<Vec<(String, Option<rich::Style>)>> {
        let label_style = has_colour(console).then(|| theme_style(console, "chart.label"));
        let mut names: Vec<String> = Vec::new();
        let mut entries = Vec::new();
        for (_, facet) in &self.items {
            let Facet::Line(chart) = facet else {
                continue;
            };
            if !chart.has_legend() {
                continue;
            }
            for (index, series) in chart.all_series().iter().enumerate() {
                if index < names.len() || series.name().is_empty() {
                    continue;
                }
                while names.len() < index {
                    names.push(String::new());
                }
                names.push(series.name().to_string());
                let (marker, style) = chart.legend_entry(console, options, index);
                entries.push(vec![
                    (marker.to_string(), style),
                    (" ".to_string(), None),
                    (truncate(series.name(), 40, ascii), label_style.clone()),
                ]);
            }
        }
        entries
    }

    /// The note's lines at `width`.
    fn note_lines(&self, console: &Console, options: &ConsoleOptions, ascii: bool) -> Vec<Line> {
        if !self.note {
            return Vec::new();
        }
        let mut entries = Vec::new();
        if let Some((lo, hi)) = self.scale() {
            let format = ValueFormat::Compact;
            let style = has_colour(console).then(|| theme_style(console, "chart.axis"));
            entries.push(vec![(
                format!(
                    "scale {} to {} in every chart",
                    format.format(lo),
                    format.format(hi)
                ),
                style,
            )]);
        }
        entries.extend(self.legend(console, options, ascii));
        wrap_entries(entries, options.max_width)
    }

    /// Each chart's height: as set, or a share of `given` rows.
    fn chart_height(&self, given: Option<usize>, rows: usize, note: usize) -> Option<usize> {
        if self.height.is_some() {
            return self.height;
        }
        let given = given?;
        // A title a row, a blank line between rows, then the note.
        let chrome = rows + rows.saturating_sub(1) + note;
        Some((given.saturating_sub(chrome) / rows.max(1)).max(1))
    }
}

/// `lines` as one [`Line`] each, control segments left out.
fn to_lines(lines: Vec<Vec<Segment>>) -> Vec<Line> {
    lines
        .into_iter()
        .map(|segments| {
            let mut line = Line::new();
            for s in segments.into_iter().filter(|s| !s.control) {
                line.push(&s.text, s.style);
            }
            line
        })
        .collect()
}

impl Renderable for SmallMultiples {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let ascii = self.charset.resolve(console, options, Charset::Blocks) == Charset::Ascii;
        if self.items.is_empty() {
            let mut line = Line::new();
            line.push(&truncate("no data", width, ascii), None);
            return lines_to_segments(vec![line], width);
        }
        let columns = self.columns_at(width);
        let cell = (width.saturating_sub(self.gap * (columns - 1)) / columns).max(1);
        let rows = self.items.len().div_ceil(columns);
        let note = self.note_lines(console, options, ascii);
        let height = self.chart_height(options.height, rows, note.len());
        let title_style = has_colour(console).then(|| theme_style(console, "chart.title"));
        let facets = self.facets();

        let mut out: Vec<Line> = Vec::new();
        for (row, chunk) in facets.chunks(columns).enumerate() {
            if row > 0 {
                out.push(Line::new());
            }
            let first = row * columns;
            let mut blocks: Vec<Vec<Line>> = Vec::new();
            for (k, facet) in chunk.iter().enumerate() {
                let mut opts = options.update_width(cell);
                opts.height = height;
                // A line chart's height is its plot's: less the axis line
                // and the x labels.
                let facet = match (facet, height) {
                    (Facet::Line(c), Some(h)) => Facet::Line(c.clone().height(h.saturating_sub(2))),
                    (f, _) => f.clone(),
                };
                let mut block = vec![{
                    let mut title = Line::new();
                    let text = truncate(&self.items[first + k].0, cell, ascii);
                    title.push(&text, title_style.clone());
                    title
                }];
                block.extend(to_lines(console.render_lines(&facet, &opts, false)));
                blocks.push(block);
            }
            let tall = blocks.iter().map(Vec::len).max().unwrap_or(0);
            for i in 0..tall {
                let mut line = Line::new();
                for (k, block) in blocks.iter_mut().enumerate() {
                    if k > 0 {
                        line.pad(self.gap);
                    }
                    let mut part = block.get_mut(i).map(std::mem::take).unwrap_or_default();
                    part.crop(cell);
                    let fill = cell - part.width();
                    line.append(part);
                    line.pad(fill);
                }
                out.push(line);
            }
        }
        out.extend(note);
        lines_to_segments(out, width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let min = self.min_width.min(options.max_width);
        Measurement::new(min, options.max_width)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// `Small multiples, 3 charts on a shared scale from 0 to 60`, then each
/// chart's title and text, its lines joined with `; `.
impl crate::a11y::AccessibleText for SmallMultiples {
    fn accessible_text(&self, width: usize) -> String {
        if self.items.is_empty() {
            return "Small multiples, no charts".to_string();
        }
        let mut head = format!(
            "Small multiples, {}",
            super::describe::counted(self.items.len(), "chart", "charts")
        );
        if let Some((lo, hi)) = self.scale() {
            head.push_str(&format!(
                " on a shared scale {}",
                super::describe::range(lo, hi, ValueFormat::Compact)
            ));
        }
        let mut out = vec![head];
        for ((title, _), facet) in self.items.iter().zip(self.facets()) {
            let text = facet.accessible_text(width).replace('\n', "; ");
            if title.is_empty() {
                out.push(text);
            } else {
                out.push(format!("{title}: {text}"));
            }
        }
        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a11y::AccessibleText;
    use crate::chart::{Series, SeriesKind};

    fn rows(width: usize, r: &dyn Renderable) -> Vec<String> {
        Console::builder()
            .width(width)
            .color_system(None)
            .build()
            .render_export(r)
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn sparks() -> SmallMultiples {
        SmallMultiples::new([
            ("api", Sparkline::new([10.0, 20.0, 30.0, 40.0])),
            ("web", Sparkline::new([5.0, 5.0, 10.0, 15.0])),
            ("worker", Sparkline::new([40.0, 30.0, 20.0, 50.0])),
            ("cron", Sparkline::new([0.0, 1.0, 0.0, 2.0])),
        ])
    }

    #[test]
    fn columns_follow_the_width() {
        let grid = sparks();
        assert_eq!(grid.columns_at(40), 1);
        assert_eq!(grid.columns_at(80), 3);
        assert_eq!(grid.columns_at(120), 4);
        assert_eq!(grid.clone().columns(2).columns_at(120), 2);
        assert_eq!(grid.clone().columns(9).columns_at(120), 4);
        assert_eq!(grid.clone().min_cell_width(10).columns_at(40), 3);
    }

    #[test]
    fn a_grid_at_40_80_and_120() {
        let grid = sparks();
        assert_eq!(
            rows(40, &grid),
            vec![
                "api                                     ",
                "▂▄▆█                                    ",
                "                                        ",
                "web                                     ",
                "▁▁▂▃                                    ",
                "                                        ",
                "worker                                  ",
                "█▆▄█                                    ",
                "                                        ",
                "cron                                    ",
                "▁▁▁▁                                    ",
                "scale 0 to 50 in every chart            ",
            ]
        );
        assert_eq!(rows(80, &grid).len(), 6);
        assert_eq!(
            rows(120, &grid),
            vec![
                "api                            web                            worker                         cron                           ",
                "▂▄▆█                           ▁▁▂▃                           █▆▄█                           ▁▁▁▁                           ",
                "scale 0 to 50 in every chart                                                                                            ",
            ]
        );
    }

    #[test]
    fn the_scale_is_shared_unless_asked_not_to() {
        let grid = sparks();
        assert_eq!(grid.scale(), Some((0.0, 50.0)));
        let own = grid.clone().shared_scale(false);
        assert_eq!(own.scale(), None);
        // Each on its own: the low series fills its height.
        assert_eq!(rows(120, &own)[1].split_whitespace().last(), Some("▁▄▁█"));
        assert_eq!(grid.clone().range(0.0, 100.0).scale(), Some((0.0, 100.0)));
        // Bars include zero; a line chart's axis rounds out.
        let mixed = SmallMultiples::new([
            ("bars", Facet::from(BarChart::new().bar("a", 12.0))),
            (
                "line",
                Facet::from(LineChart::new().series(Series::from_values("p50", [13.0, 47.0]))),
            ),
        ]);
        assert_eq!(mixed.scale(), Some((0.0, 50.0)));
    }

    #[test]
    fn line_charts_share_one_legend() {
        let chart = |ys: [f64; 3]| {
            LineChart::new()
                .series(Series::from_values("p50", ys))
                .series(Series::from_values("p99", ys.map(|y| y * 2.0)).kind(SeriesKind::Scatter))
                .charset(Charset::Ascii)
                .height(3)
        };
        let grid = SmallMultiples::new([("a", chart([1.0, 2.0, 3.0])), ("b", chart([2.0, 2.0, 2.0]))])
            .min_cell_width(16);
        let out = rows(48, &grid);
        assert_eq!(
            out.last().map(|l| l.trim_end()),
            Some("scale 0 to 6 in every chart  * p50  + p99")
        );
        // No chart drew its own legend.
        assert!(!out[..out.len() - 1].iter().any(|l| l.contains("p50")), "{out:#?}");
    }

    #[test]
    fn a_height_is_shared_among_the_rows() {
        let grid = sparks().columns(2).note(false);
        let console = Console::builder().width(40).color_system(None).build();
        let lines = console.render_lines(&grid, &console.options().update_height(9), false);
        // Two rows of charts: 2 titles, a blank line, 3 rows each.
        assert_eq!(lines.len(), 9);
        let fixed = grid.height(2);
        assert_eq!(rows(40, &fixed).len(), 2 * 3 + 1);
    }

    #[test]
    fn accessible_text_reads_each_chart() {
        let grid = SmallMultiples::new([
            ("api", Sparkline::new([10.0, 20.0, 30.0, 40.0])),
            ("cron", Sparkline::new([0.0, 1.0, 0.0, 2.0])),
        ]);
        assert_eq!(
            grid.accessible_text(80),
            "Small multiples, 2 charts on a shared scale from 0 to 40\n\
             api: Sparkline, 4 values: min 10, max 40, last 40, trend up\n\
             cron: Sparkline, 4 values: min 0, max 2, last 2, trend up"
        );
        assert_eq!(
            SmallMultiples::default().accessible_text(80),
            "Small multiples, no charts"
        );
    }
}
