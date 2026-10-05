//! A labelled grid of values drawn as shades.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::{
    cell_units, cells, entries_width, has_colour, lines_to_segments, theme_style, truncate,
    wrap_entries, Charset, Line, Scale, ValueFormat,
};

/// Levels for blocks, lowest first.
const SHADES: [char; 5] = [' ', '░', '▒', '▓', '█'];
/// Levels for ASCII, lowest first.
const ASCII_SHADES: [char; 10] = [' ', '.', ':', '-', '=', '+', '*', '#', '%', '@'];
/// How many `chart.heat.N` styles there are.
const HEAT_STYLES: usize = 5;

/// A grid of values, one row per label and one column per header, each
/// cell a shade from lowest to highest.
///
/// - Values are scaled from the smallest to the largest (or
///   [`range`](Self::range)) into equal steps: five in blocks (` ░▒▓█`), ten
///   in ASCII (` .:-=+*#%@`). The shade is the value, so the grid reads in
///   black and white; with colour each step also takes a `chart.heat.N`
///   style, cold to hot.
/// - A NaN or infinite value is drawn `·` (`?` in ASCII), so a missing
///   value never passes for a low one.
/// - The legend under the grid shows the steps between the lowest and
///   highest value of the scale.
/// - Each cell is [`cell_width`](Self::cell_width) cells wide (2 by
///   default). Column headers are written at their column's first cell when
///   they fit, leaving a space after the one before, so with narrow cells
///   every second or third header shows.
/// - Given less width, the cells narrow to one cell, then the row labels
///   are cut, then neighbouring columns are merged (each shows the mean of
///   its finite values) so every value still counts.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Charset, Heatmap};
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let map = Heatmap::new()
///     .columns(["mon", "tue", "wed", "thu"])
///     .row("api", [1.0, 4.0, 9.0, 2.0])
///     .row("web", [0.0, 6.0, 8.0, f64::NAN])
///     .cell_width(4)
///     .charset(Charset::Ascii);
/// assert_eq!(
///     console.render_export(&map),
///     concat!(
///         "    mon tue wed thu        \n",
///         "api ....====@@@@::::       \n",
///         "web     ****%%%%????       \n",
///         "0 [ .:-=+*#%@] 9  ? no data\n",
///     )
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Heatmap {
    columns: Vec<String>,
    rows: Vec<(String, Vec<f64>)>,
    min: Option<f64>,
    max: Option<f64>,
    charset: Charset,
    cell_width: usize,
    legend: bool,
    format: ValueFormat,
}

impl Default for Heatmap {
    fn default() -> Self {
        Self::new()
    }
}

/// How a heatmap fits its width.
struct Grid {
    label: usize,
    cell: usize,
    /// For each drawn column, the range of data columns it stands for.
    buckets: Vec<(usize, usize)>,
}

impl Heatmap {
    /// An empty heatmap; add rows with [`row`](Self::row).
    pub fn new() -> Self {
        Heatmap {
            columns: Vec::new(),
            rows: Vec::new(),
            min: None,
            max: None,
            charset: Charset::Auto,
            cell_width: 2,
            legend: true,
            format: ValueFormat::Compact,
        }
    }

    /// The column headers.
    pub fn columns<S: Into<String>>(mut self, headers: impl IntoIterator<Item = S>) -> Self {
        self.columns = headers.into_iter().map(Into::into).collect();
        self
    }

    /// Add a row of values, in column order.
    pub fn row(mut self, label: impl Into<String>, values: impl IntoIterator<Item = f64>) -> Self {
        self.rows.push((label.into(), values.into_iter().collect()));
        self
    }

    /// Fix the scale; values outside it take the end shades.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Glyphs to draw with: [`Charset::Ascii`] asks for the ASCII shades;
    /// anything else draws blocks (ASCII on an ASCII-only console).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// Cells per value (default 2, at least 1).
    pub fn cell_width(mut self, width: usize) -> Self {
        self.cell_width = width.max(1);
        self
    }

    /// Show the legend (default on).
    pub fn legend(mut self, show: bool) -> Self {
        self.legend = show;
        self
    }

    /// How the legend writes the bounds (default [`ValueFormat::Compact`]).
    pub fn format(mut self, format: ValueFormat) -> Self {
        self.format = format;
        self
    }

    /// The scale every value is shaded on.
    pub fn scale(&self) -> Scale {
        let values = self.rows.iter().flat_map(|(_, v)| v.iter().copied());
        Scale::from_values(values).bounds(self.min, self.max)
    }

    fn column_count(&self) -> usize {
        self.rows
            .iter()
            .map(|(_, v)| v.len())
            .max()
            .unwrap_or(0)
            .max(self.columns.len())
    }

    fn label_width(&self) -> usize {
        self.rows.iter().map(|(l, _)| cells(l)).max().unwrap_or(0)
    }

    fn has_missing(&self) -> bool {
        let n = self.column_count();
        self.rows
            .iter()
            .any(|(_, v)| v.len() < n || v.iter().any(|x| !x.is_finite()))
    }

    fn layout(&self, width: usize) -> Grid {
        let n = self.column_count();
        let l = self.label_width();
        let part = |l: usize| l + usize::from(l > 0);
        let each = |cell: usize| Grid {
            label: 0,
            cell,
            buckets: (0..n).map(|i| (i, i + 1)).collect(),
        };
        let room = width.saturating_sub(part(l));
        if room >= n {
            let cell = self.cell_width.min(room / n.max(1)).max(1);
            return Grid {
                label: l,
                ..each(cell)
            };
        }
        let keep = l.min(3);
        if let Some(label) = width.checked_sub(n + 1) {
            if label >= keep && label > 0 {
                return Grid {
                    label: label.min(l),
                    ..each(1)
                };
            }
        }
        // Merge columns into as many as fit.
        let label = if width > part(keep) + 1 { keep } else { 0 };
        let slots = width.saturating_sub(part(label)).max(1);
        let buckets = (0..slots)
            .map(|i| {
                let start = i * n / slots;
                (start, ((i + 1) * n / slots).max(start + 1))
            })
            .collect();
        Grid {
            label,
            cell: 1,
            buckets,
        }
    }

    /// The step of `value` on `scale`, among `levels`.
    fn level(scale: &Scale, value: f64, levels: usize) -> Option<usize> {
        let n = scale.normalize(value)?;
        Some(((n * levels as f64).floor() as usize).min(levels - 1))
    }

    fn level_style(console: &Console, level: usize, levels: usize) -> Style {
        let key = format!("chart.heat.{}", level * HEAT_STYLES / levels + 1);
        theme_style(console, &key)
    }

    fn lines(&self, console: &Console, options: &ConsoleOptions) -> Vec<Line> {
        let width = options.max_width;
        let ascii = self.charset.resolve(console, options, Charset::Blocks) == Charset::Ascii;
        let colour = has_colour(console);
        if self.column_count() == 0 {
            let mut line = Line::new();
            line.push(&truncate("no data", width, ascii), None);
            return vec![line];
        }
        let shades: &[char] = if ascii { &ASCII_SHADES } else { &SHADES };
        let missing = if ascii { '?' } else { '·' };
        let levels = shades.len();
        let scale = self.scale();
        let grid = self.layout(width);
        let label_style = colour.then(|| theme_style(console, "chart.label"));
        let lead = if grid.label > 0 { grid.label + 1 } else { 0 };
        let mut out = Vec::new();

        // Headers at their column's first cell, where they fit.
        if self.columns.iter().any(|c| !c.is_empty()) {
            let plot = grid.buckets.len() * grid.cell;
            let mut row = vec![" ".to_string(); plot];
            let mut free = 0;
            for (i, (start, _)) in grid.buckets.iter().enumerate() {
                let Some(header) = self.columns.get(*start) else {
                    continue;
                };
                let header = truncate(header, usize::MAX, ascii);
                let at = i * grid.cell;
                let len = cells(&header);
                if at < free || at + len > plot || len == 0 {
                    continue;
                }
                for (k, unit) in cell_units(&header).into_iter().enumerate() {
                    row[at + k] = unit;
                }
                free = at + len + 1;
            }
            if row.iter().any(|c| c != " ") {
                let mut line = Line::new();
                line.pad(lead);
                line.push(&row.concat(), label_style.clone());
                out.push(line);
            }
        }

        for (label, values) in &self.rows {
            let mut line = Line::new();
            if grid.label > 0 {
                let text = truncate(label, grid.label, ascii);
                let pad = grid.label - cells(&text);
                line.push(&text, label_style.clone());
                line.pad(pad + 1);
            }
            for &(start, end) in &grid.buckets {
                let bucket: Vec<f64> = (start..end)
                    .filter_map(|i| values.get(i).copied())
                    .filter(|v| v.is_finite())
                    .collect();
                let value = if bucket.is_empty() {
                    f64::NAN
                } else {
                    bucket.iter().sum::<f64>() / bucket.len() as f64
                };
                let (glyph, style) = match Self::level(&scale, value, levels) {
                    Some(level) => (
                        shades[level],
                        colour.then(|| Self::level_style(console, level, levels)),
                    ),
                    None => (missing, None),
                };
                line.push(&glyph.to_string().repeat(grid.cell), style);
            }
            out.push(line);
        }
        if self.legend {
            out.extend(wrap_entries(
                self.legend_entries(console, ascii, colour),
                width,
            ));
        }
        out
    }

    fn legend_entries(
        &self,
        console: &Console,
        ascii: bool,
        colour: bool,
    ) -> Vec<Vec<(String, Option<Style>)>> {
        let shades: &[char] = if ascii { &ASCII_SHADES } else { &SHADES };
        let scale = self.scale();
        let label_style = colour.then(|| theme_style(console, "chart.label"));
        let mut ramp = vec![(
            format!("{} [", self.format.format(scale.min())),
            label_style.clone(),
        )];
        for (level, c) in shades.iter().enumerate() {
            let style = colour.then(|| Self::level_style(console, level, shades.len()));
            ramp.push((c.to_string(), style));
        }
        ramp.push((
            format!("] {}", self.format.format(scale.max())),
            label_style.clone(),
        ));
        let mut entries = vec![ramp];
        if self.has_missing() {
            let missing = if ascii { "?" } else { "·" };
            entries.push(vec![(format!("{missing} no data"), label_style)]);
        }
        entries
    }
}

impl Renderable for Heatmap {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        lines_to_segments(self.lines(console, options), options.max_width)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let n = self.column_count();
        if n == 0 {
            return Measurement::new(7, 7).with_maximum(options.max_width);
        }
        let ascii = self.charset.resolve(console, options, Charset::Blocks) == Charset::Ascii;
        let l = self.label_width();
        let grid = (l + usize::from(l > 0)).saturating_add(n.saturating_mul(self.cell_width));
        let legend = if self.legend {
            entries_width(&self.legend_entries(console, ascii, false))
        } else {
            0
        };
        let max = grid.max(legend);
        Measurement::new(n.min(max), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}
