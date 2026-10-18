//! Progress gauges laid out as ratatui's: [`FillGauge`], a bar that fills
//! its area with the label on the middle row (ratatui's `Gauge`), and
//! [`LineGauge`], a label and a line (ratatui's `LineGauge`).

use rich::measure::Measurement;
use rich::{Color, Console, ConsoleOptions, Renderable, Segment, Style};

use super::{
    cell_units, cells, has_colour, lines_to_segments, theme_style, user_style, Charset, Line,
};
use crate::fidelity::ascii_text;

/// Left eighths of a cell, from one to seven.
const EIGHTHS: [char; 7] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉'];

/// The ratio a gauge shows: clamped to `0..=1`, and 0 for NaN.
fn clamp_ratio(ratio: f64) -> f64 {
    if ratio.is_nan() {
        0.0
    } else {
        ratio.clamp(0.0, 1.0)
    }
}

/// Whether to draw in ASCII only on `console`.
fn ascii(console: &Console, options: &ConsoleOptions) -> bool {
    Charset::Auto.resolve(console, options, Charset::Blocks) == Charset::Ascii
}

/// A label's text, ASCII on an ASCII console.
fn label_text(text: &str, ascii: bool) -> String {
    if ascii && !text.is_ascii() {
        ascii_text(text)
    } else {
        text.to_string()
    }
}

/// `style` with its colours swapped, as a label over the filled part of a
/// gauge is drawn: the bar's colour behind, its background (the terminal's
/// default without one) in front. A style without a foreground colour is
/// reversed instead.
fn swapped(style: &Style) -> Style {
    match style.color() {
        Some(front) => {
            let back = style
                .bgcolor()
                .cloned()
                .unwrap_or_else(Color::default_color);
            style
                .without_color()
                .combine(&Style::from_color(Some(back), Some(front.clone())))
        }
        None => style.combine(&Style::parse("reverse").unwrap_or_default()),
    }
}

/// A style's foreground colour and attributes, without its background.
fn front(style: &Style) -> Style {
    style
        .without_color()
        .combine(&Style::from_color(style.color().cloned(), None))
}

/// A progress bar that fills its area, as ratatui's `Gauge`.
///
/// - The filled part runs from the left, in whole cells (`█`), or with an
///   eighth of a cell at its edge (`▏` to `▉`) when
///   [`unicode`](Self::unicode) is on. ASCII consoles get `#`, and `=` for
///   a half with `unicode` on.
/// - The label sits centred on the middle row; without one it is the
///   percentage (`42%`). Over the filled part its colours are swapped, so
///   it reads on the bar.
/// - [`style`](Self::style) is ratatui's `gauge_style`: its foreground is
///   the bar's colour and its background the rest's (`"magenta on
///   black"`). Without it the bar is `chart.gauge.filled` and the rest
///   `chart.gauge.unfilled`.
/// - It fills the width it is given (or [`width`](Self::width)) and the
///   height it is given (a `Layout` region, an intuiTUIve node), or is
///   [`height`](Self::height) rows; given no height, one row.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::FillGauge;
///
/// let console = Console::builder().width(20).color_system(None).build();
/// let gauge = FillGauge::new(0.42).height(3);
/// assert_eq!(
///     console.render_export(&gauge),
///     concat!(
///         "████████            \n",
///         "████████42%         \n",
///         "████████            \n",
///     )
/// );
/// // With colour, the label's colours are swapped where it covers the bar.
/// let fine = FillGauge::new(0.42).label("42.00%").unicode(true);
/// assert_eq!(console.render_export(&fine), "███████42.00%       \n");
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct FillGauge {
    ratio: f64,
    label: Option<String>,
    label_style: Option<String>,
    style: Option<String>,
    unicode: bool,
    width: Option<usize>,
    height: Option<usize>,
}

impl FillGauge {
    /// A gauge filled to `ratio`, from 0 to 1; values outside are clamped,
    /// and NaN is 0.
    pub fn new(ratio: f64) -> Self {
        FillGauge {
            ratio: clamp_ratio(ratio),
            label: None,
            label_style: None,
            style: None,
            unicode: false,
            width: None,
            height: None,
        }
    }

    /// A gauge filled to `percent` percent (0 to 100).
    pub fn percent(percent: f64) -> Self {
        Self::new(percent / 100.0)
    }

    /// The ratio shown, from 0 to 1.
    pub fn ratio(&self) -> f64 {
        self.ratio
    }

    /// The label (default: the percentage, `42%`). An empty one shows
    /// nothing.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// A style over the label's own: a theme key or a definition, such as
    /// `"bold italic"`.
    pub fn label_style(mut self, style: impl Into<String>) -> Self {
        self.label_style = Some(style.into());
        self
    }

    /// The gauge's style, as ratatui's `gauge_style`: a theme key or a
    /// definition whose foreground colours the bar and whose background
    /// the rest. Its attributes go on the label.
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// Draw the bar's edge in eighths of a cell (default off: whole
    /// cells, as ratatui's `use_unicode`).
    pub fn unicode(mut self, unicode: bool) -> Self {
        self.unicode = unicode;
        self
    }

    /// Width in cells (default: the width it is given).
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// Height in rows, whatever height it is given (at most 65535).
    /// Without it, the gauge fills the height it is given, or takes one
    /// row.
    pub fn height(mut self, rows: usize) -> Self {
        self.height = Some(rows.min(u16::MAX as usize));
        self
    }

    fn label_or_percent(&self) -> String {
        self.label
            .clone()
            .unwrap_or_else(|| format!("{}%", (self.ratio * 100.0).round()))
    }

    fn lines(&self, console: &Console, options: &ConsoleOptions) -> Vec<Line> {
        let width = self
            .width
            .map_or(options.max_width, |w| w.min(options.max_width));
        let height = self
            .height
            .or(options.height)
            .unwrap_or(1)
            .min(u16::MAX as usize);
        let ascii = ascii(console, options);
        let colour = has_colour(console);
        let (filled, unfilled) = match &self.style {
            Some(spec) => {
                let style = user_style(console, spec);
                (style.clone(), style)
            }
            None => (
                theme_style(console, "chart.gauge.filled"),
                theme_style(console, "chart.gauge.unfilled"),
            ),
        };
        let extra = self
            .label_style
            .as_deref()
            .map(|spec| user_style(console, spec))
            .unwrap_or_default();
        let styles = colour.then(|| GaugeStyles {
            label_on_bar: swapped(&filled).combine(&extra),
            label_on_rest: unfilled.combine(&front(&filled)).combine(&extra),
            filled,
            unfilled,
        });

        // The filled cells, and the glyph of a part-filled one after them.
        let exact = width as f64 * self.ratio;
        let (end, edge) = if self.unicode {
            let end = exact.floor() as usize;
            let rest = exact - end as f64;
            let edge = if end >= width {
                None
            } else if ascii {
                match (rest * 2.0).round() as usize {
                    0 => None,
                    1 => Some('='),
                    _ => Some('#'),
                }
            } else {
                match (rest * 8.0).round() as usize {
                    0 => None,
                    8 => Some('█'),
                    n => Some(EIGHTHS[n - 1]),
                }
            };
            (end.min(width), edge)
        } else {
            ((exact.round() as usize).min(width), None)
        };
        let full = if ascii { '#' } else { '█' };

        let text = label_text(&self.label_or_percent(), ascii);
        let mut units = cell_units(&text);
        units.truncate(width);
        if units.last().is_some_and(|u| !u.is_empty()) && cells(units.last().unwrap()) > 1 {
            // A wide character cut in two at the edge.
            *units.last_mut().unwrap() = " ".to_string();
        }
        let label_col = (width - units.len()) / 2;
        let label_row = height / 2;

        (0..height)
            .map(|row| {
                let mut line = Line::new();
                for col in 0..width {
                    let on_bar = col < end;
                    let pick = |s: &GaugeStyles, label: bool| -> Style {
                        match (label, on_bar) {
                            (true, true) => s.label_on_bar.clone(),
                            (true, false) => s.label_on_rest.clone(),
                            (false, true) => s.filled.clone(),
                            (false, false) if col == end && edge.is_some() => s.filled.clone(),
                            (false, false) => s.unfilled.clone(),
                        }
                    };
                    let in_label =
                        row == label_row && col >= label_col && col < label_col + units.len();
                    if in_label {
                        let unit = &units[col - label_col];
                        // The second cell of a wide character is part of the
                        // one before it.
                        if !unit.is_empty() {
                            line.push(unit, styles.as_ref().map(|s| pick(s, true)));
                        }
                        continue;
                    }
                    let glyph = if on_bar {
                        full
                    } else if col == end {
                        edge.unwrap_or(' ')
                    } else {
                        ' '
                    };
                    line.push(&glyph.to_string(), styles.as_ref().map(|s| pick(s, false)));
                }
                line
            })
            .collect()
    }
}

/// The four looks of a [`FillGauge`]'s cells.
struct GaugeStyles {
    filled: Style,
    unfilled: Style,
    label_on_bar: Style,
    label_on_rest: Style,
}

impl Renderable for FillGauge {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        lines_to_segments(self.lines(console, options), options.max_width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let max = self.width.unwrap_or(options.max_width);
        Measurement::new(max.min(4), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// The glyphs of a [`LineGauge`]'s line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineSet {
    /// `─` (ratatui's default).
    #[default]
    Thin,
    /// `━`.
    Thick,
    /// `═`.
    Double,
}

impl LineSet {
    fn glyph(self, ascii: bool) -> char {
        match self {
            _ if ascii => '-',
            LineSet::Thin => '─',
            LineSet::Thick => '━',
            LineSet::Double => '═',
        }
    }
}

/// A label and a line, the part up to the ratio in one style and the rest
/// in another, as ratatui's `LineGauge`.
///
/// - The label comes first (default: the percentage, three wide: ` 42%`),
///   then a space, then the line to the edge of the width it is given (or
///   [`width`](Self::width)).
/// - [`line`](Self::line) picks the glyphs (`─`, `━` or `═`), and
///   [`symbols`](Self::symbols) any two of your own (`▰` and `▱`). An
///   ASCII console gets `-`.
/// - [`filled_style`](Self::filled_style) and
///   [`unfilled_style`](Self::unfilled_style) default to
///   `chart.gauge.filled` and `chart.gauge.unfilled`.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{LineGauge, LineSet};
///
/// let console = Console::builder().width(30).color_system(None).build();
/// let gauge = LineGauge::new(0.42).label("Downloading: 42%").line(LineSet::Thick);
/// assert_eq!(console.render_export(&gauge), "Downloading: 42% ━━━━━━━━━━━━━\n");
/// let plain = LineGauge::new(0.5).width(12);
/// assert_eq!(console.render_export(&plain), " 50% ───────\n");
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct LineGauge {
    ratio: f64,
    label: Option<String>,
    label_style: Option<String>,
    filled_style: Option<String>,
    unfilled_style: Option<String>,
    line: LineSet,
    symbols: Option<(char, char)>,
    width: Option<usize>,
}

impl LineGauge {
    /// A gauge at `ratio`, from 0 to 1; values outside are clamped, and NaN
    /// is 0.
    pub fn new(ratio: f64) -> Self {
        LineGauge {
            ratio: clamp_ratio(ratio),
            label: None,
            label_style: None,
            filled_style: None,
            unfilled_style: None,
            line: LineSet::Thin,
            symbols: None,
            width: None,
        }
    }

    /// A gauge at `percent` percent (0 to 100).
    pub fn percent(percent: f64) -> Self {
        Self::new(percent / 100.0)
    }

    /// The ratio shown, from 0 to 1.
    pub fn ratio(&self) -> f64 {
        self.ratio
    }

    /// The label (default: the percentage, ` 42%`).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The label's style: a theme key or a definition (default
    /// `chart.label`).
    pub fn label_style(mut self, style: impl Into<String>) -> Self {
        self.label_style = Some(style.into());
        self
    }

    /// The style of the line up to the ratio (default
    /// `chart.gauge.filled`).
    pub fn filled_style(mut self, style: impl Into<String>) -> Self {
        self.filled_style = Some(style.into());
        self
    }

    /// The style of the rest of the line (default `chart.gauge.unfilled`).
    pub fn unfilled_style(mut self, style: impl Into<String>) -> Self {
        self.unfilled_style = Some(style.into());
        self
    }

    /// The line's glyphs (default [`LineSet::Thin`]).
    pub fn line(mut self, line: LineSet) -> Self {
        self.line = line;
        self
    }

    /// Glyphs of your own for the filled part and the rest, instead of a
    /// [`LineSet`]; one that is not one cell wide, or not ASCII on an ASCII
    /// console, falls back to the set's.
    pub fn symbols(mut self, filled: char, unfilled: char) -> Self {
        self.symbols = Some((filled, unfilled));
        self
    }

    /// Width in cells (default: the width it is given).
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    fn line_of(&self, console: &Console, options: &ConsoleOptions) -> Line {
        let width = self
            .width
            .map_or(options.max_width, |w| w.min(options.max_width));
        let ascii = ascii(console, options);
        let colour = has_colour(console);
        let style = |spec: &Option<String>, key: &str| {
            colour.then(|| match spec {
                Some(spec) => user_style(console, spec),
                None => theme_style(console, key),
            })
        };
        let set = self.line.glyph(ascii);
        let fit = |c: char| {
            (rich::cells::char_cell_width(c) == 1 && (!ascii || c.is_ascii_graphic())).then_some(c)
        };
        let (filled_glyph, unfilled_glyph) = match self.symbols {
            Some((filled, unfilled)) => (fit(filled).unwrap_or(set), fit(unfilled).unwrap_or(set)),
            None => (set, set),
        };
        let label = self
            .label
            .clone()
            .unwrap_or_else(|| format!("{:3.0}%", self.ratio * 100.0));
        let label = super::truncate(&label_text(&label, ascii), width, ascii);
        let mut line = Line::new();
        line.push(&label, style(&self.label_style, "chart.label"));
        let start = line.width() + 1;
        if start >= width {
            return line;
        }
        line.pad(1);
        let room = width - start;
        let filled = ((room as f64) * self.ratio).floor() as usize;
        line.push(
            &filled_glyph.to_string().repeat(filled),
            style(&self.filled_style, "chart.gauge.filled"),
        );
        line.push(
            &unfilled_glyph.to_string().repeat(room - filled),
            style(&self.unfilled_style, "chart.gauge.unfilled"),
        );
        line
    }
}

impl Renderable for LineGauge {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        lines_to_segments(vec![self.line_of(console, options)], options.max_width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let max = self.width.unwrap_or(options.max_width);
        Measurement::new(max.min(4), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// `Gauge 42 of 100 GB: 42 percent`: the label, if it has one, and how full.
fn gauge_text(label: Option<&str>, ratio: f64) -> String {
    let percent = super::describe::percent(ratio);
    match label.map(str::trim).filter(|l| !l.is_empty()) {
        Some(label) => format!("Gauge {label}: {percent}"),
        None => format!("Gauge: {percent}"),
    }
}

/// `Gauge: 42 percent`, or `Gauge <label>: 42 percent` with a label.
impl crate::a11y::AccessibleText for FillGauge {
    fn accessible_text(&self, _width: usize) -> String {
        gauge_text(self.label.as_deref(), self.ratio)
    }
}

/// `Gauge: 42 percent`, or `Gauge <label>: 42 percent` with a label.
impl crate::a11y::AccessibleText for LineGauge {
    fn accessible_text(&self, _width: usize) -> String {
        gauge_text(self.label.as_deref(), self.ratio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessible_text_says_how_full() {
        use crate::a11y::AccessibleText;
        assert_eq!(FillGauge::new(0.424).accessible_text(80), "Gauge: 42 percent");
        assert_eq!(
            FillGauge::percent(50.0)
                .label("50/100 GB")
                .accessible_text(80),
            "Gauge 50/100 GB: 50 percent"
        );
        assert_eq!(
            LineGauge::new(1.0).label("done").accessible_text(80),
            "Gauge done: 100 percent"
        );
    }

    fn plain(width: usize) -> Console {
        Console::builder().width(width).color_system(None).build()
    }

    fn coloured(width: usize) -> Console {
        Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(rich::ColorSystem::Truecolor))
            .no_color(false)
            .build()
    }

    fn rows(console: &Console, r: &dyn Renderable) -> Vec<String> {
        console
            .render_export(r)
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn fill_gauges_fill_whole_cells_or_eighths() {
        let gauge = |ratio: f64| FillGauge::new(ratio).label("");
        // 0.42 of 10 cells is 4.2: four whole cells, rounded.
        assert_eq!(rows(&plain(10), &gauge(0.42)), ["████      "]);
        assert_eq!(rows(&plain(10), &gauge(0.46)), ["█████     "]);
        // In eighths, 4.2 cells are four and a quarter.
        assert_eq!(rows(&plain(10), &gauge(0.42).unicode(true)), ["████▎     "]);
        assert_eq!(rows(&plain(10), &gauge(1.0).unicode(true)), ["██████████"]);
        assert_eq!(rows(&plain(10), &gauge(0.0).unicode(true)), ["          "]);
        // Out of range and NaN are clamped.
        assert_eq!(gauge(7.0).ratio(), 1.0);
        assert_eq!(gauge(-1.0).ratio(), 0.0);
        assert_eq!(gauge(f64::NAN).ratio(), 0.0);
    }

    #[test]
    fn fill_gauges_at_tiny_widths() {
        for (width, want) in [(0, ""), (1, "4"), (2, "42"), (3, "42%")] {
            let console = plain(width.max(1));
            let gauge = FillGauge::new(0.42).width(width);
            let out = console.render_export(&gauge);
            assert_eq!(out.trim_end_matches('\n'), want, "width {width}");
        }
        // Wider: the label is centred, over the bar.
        assert_eq!(rows(&plain(4), &FillGauge::new(0.5)), ["50% "]);
        assert_eq!(rows(&plain(5), &FillGauge::new(0.5)), ["█50% "]);
    }

    #[test]
    fn fill_gauges_take_the_height_given() {
        let console = plain(10);
        let options = console.options().update_height(5);
        let lines = console.render_lines(&FillGauge::new(0.3), &options, false);
        assert_eq!(lines.len(), 5);
        // The label is on the middle row.
        let text: Vec<String> = lines
            .iter()
            .map(|l| l.iter().map(|s| s.text.as_str()).collect())
            .collect();
        assert_eq!(text[2], "███30%    ");
        assert_eq!(text[0], "███       ");
        // An explicit height wins.
        let lines = console.render_lines(&FillGauge::new(0.3).height(2), &options, false);
        assert_eq!(lines.iter().filter(|l| !l.is_empty()).count(), 2);
    }

    #[test]
    fn the_label_swaps_its_colours_over_the_bar() {
        let console = coloured(10);
        let gauge = FillGauge::new(0.5).style("magenta on black italic bold");
        let lines = console.render_lines(&gauge, &console.options(), false);
        let at = |col: usize| -> (String, Option<Style>) {
            let mut seen = 0;
            for s in &lines[0] {
                let n = cells(&s.text);
                if col < seen + n {
                    return (s.text.clone(), s.style.clone());
                }
                seen += n;
            }
            panic!("no cell {col}")
        };
        // "█████" then "  " ... the label "50%" at columns 3 to 5.
        assert_eq!(at(0).1, Style::parse("magenta on black italic bold").ok());
        assert_eq!(at(3).1, Style::parse("black on magenta italic bold").ok());
        assert_eq!(at(5).1, Style::parse("magenta on black italic bold").ok());
        assert_eq!(at(9).1, Style::parse("magenta on black italic bold").ok());
        // Without a background colour, the label is in the default colour.
        let gauge = FillGauge::new(0.5).style("green");
        let lines = console.render_lines(&gauge, &console.options(), false);
        let style = lines[0]
            .iter()
            .find(|s| s.text.contains('5'))
            .and_then(|s| s.style.clone())
            .unwrap();
        assert_eq!(style.bgcolor(), Style::parse("on green").unwrap().bgcolor());
        assert!(style.color().is_some_and(|c| c.is_default()));
    }

    #[test]
    fn ascii_consoles_draw_hashes_and_halves() {
        let console = Console::builder()
            .width(10)
            .color_system(None)
            .ascii_only(true)
            .build();
        let gauge = FillGauge::new(0.45).label("").unicode(true);
        assert_eq!(console.render_export(&gauge), "####=     \n");
        let gauge = FillGauge::new(0.45).label("é");
        assert_eq!(console.render_export(&gauge), "####?     \n");
        let line = LineGauge::new(0.5).line(LineSet::Double);
        assert_eq!(console.render_export(&line), " 50% -----\n");
    }

    #[test]
    fn line_gauges_fill_after_the_label() {
        let console = plain(20);
        assert_eq!(
            rows(&console, &LineGauge::new(0.5).label("cpu")),
            ["cpu ────────────────"]
        );
        // Floor, as ratatui: 16 cells at 0.99 is 15 filled.
        let gauge = LineGauge::new(0.99).label("cpu").symbols('▰', '▱');
        assert_eq!(rows(&console, &gauge), ["cpu ▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▱"]);
        // No room for a line: the label alone, cut to the width.
        assert_eq!(
            rows(&plain(4), &LineGauge::new(0.5).label("download")),
            ["dow…"]
        );
        assert_eq!(rows(&plain(5), &LineGauge::new(0.0)), ["  0%"]);
        assert_eq!(rows(&plain(6), &LineGauge::new(1.0)), ["100% ─"]);
        // A wide symbol falls back to the set's glyph.
        let wide = LineGauge::new(0.5).label("").symbols('日', '▱').width(5);
        assert_eq!(rows(&console, &wide), [" ──▱▱"]);
    }

    #[test]
    fn line_gauges_style_the_two_parts() {
        let console = coloured(9);
        let gauge = LineGauge::new(0.5)
            .label("ab")
            .filled_style("red")
            .unfilled_style("blue")
            .label_style("bold");
        let lines = console.render_lines(&gauge, &console.options(), false);
        let runs: Vec<(String, Option<Style>)> = lines[0]
            .iter()
            .map(|s| (s.text.clone(), s.style.clone()))
            .collect();
        assert_eq!(
            runs,
            vec![
                ("ab".to_string(), Style::parse("bold").ok()),
                (" ".to_string(), None),
                ("───".to_string(), Style::parse("red").ok()),
                ("───".to_string(), Style::parse("blue").ok()),
            ]
        );
    }
}
