//! Health levels, and matrices of states such as a CI run's results.

use rich::cells::char_cell_width;
use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::{
    cells, centre, entries_width, has_colour, lines_to_segments, theme_style, truncate, user_style,
    wrap_entries, Charset, Line,
};

/// How healthy a value is, for a [`KpiCard`](super::KpiCard).
///
/// Each status is a symbol, a word and a colour, so it reads without colour:
///
/// | Status | Symbol | ASCII | Word | Theme key |
/// |---|---|---|---|---|
/// | [`Ok`](Self::Ok) | `✓` | `+` | `ok` | `chart.ok` |
/// | [`Warning`](Self::Warning) | `!` | `!` | `warning` | `chart.warning` |
/// | [`Critical`](Self::Critical) | `✗` | `x` | `critical` | `chart.critical` |
/// | [`Unknown`](Self::Unknown) | `?` | `?` | `unknown` | `chart.unknown` |
///
/// ```
/// use rich_ext::chart::Status;
///
/// assert_eq!(Status::Critical.text(false), "✗ critical");
/// assert_eq!(Status::Critical.text(true), "x critical");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    /// Within its limits.
    Ok,
    /// Needs a look.
    Warning,
    /// Out of its limits.
    Critical,
    /// Not known (no data, or not checked).
    Unknown,
}

impl Status {
    /// The symbol, ASCII when `ascii`.
    pub fn symbol(self, ascii: bool) -> char {
        match (self, ascii) {
            (Status::Ok, false) => '✓',
            (Status::Ok, true) => '+',
            (Status::Warning, _) => '!',
            (Status::Critical, false) => '✗',
            (Status::Critical, true) => 'x',
            (Status::Unknown, _) => '?',
        }
    }

    /// The word: `ok`, `warning`, `critical` or `unknown`.
    pub fn word(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warning => "warning",
            Status::Critical => "critical",
            Status::Unknown => "unknown",
        }
    }

    /// The theme key: `chart.ok`, `chart.warning`, `chart.critical` or
    /// `chart.unknown`.
    pub fn key(self) -> &'static str {
        match self {
            Status::Ok => "chart.ok",
            Status::Warning => "chart.warning",
            Status::Critical => "chart.critical",
            Status::Unknown => "chart.unknown",
        }
    }

    /// The symbol and the word: `✓ ok`.
    pub fn text(self, ascii: bool) -> String {
        format!("{} {}", self.symbol(ascii), self.word())
    }
}

/// One state a [`StatusMatrix`] cell can be in: a name, a symbol, an ASCII
/// symbol and a style.
///
/// The built-in states:
///
/// | State | Symbol | ASCII | Theme key |
/// |---|---|---|---|
/// | [`pass`](Self::pass) | `✓` | `+` | `chart.state.pass` |
/// | [`fail`](Self::fail) | `✗` | `X` | `chart.state.fail` |
/// | [`skip`](Self::skip) | `○` | `-` | `chart.state.skip` |
/// | [`flaky`](Self::flaky) | `≈` | `~` | `chart.state.flaky` |
///
/// ```
/// use rich_ext::chart::State;
///
/// let blocked = State::new("blocked", '⊘', '#', "bold magenta");
/// assert_eq!(blocked.symbol(false), '⊘');
/// assert_eq!(blocked.symbol(true), '#');
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct State {
    name: String,
    symbol: char,
    ascii: char,
    style: String,
}

impl State {
    /// A state called `name`, drawn as `symbol` (`ascii` on an ASCII
    /// console) in `style`, a theme key or a definition.
    ///
    /// A symbol must take one cell: a wide or zero-width `symbol` is
    /// replaced by `ascii`, and an `ascii` above U+007F by `?`.
    pub fn new(
        name: impl Into<String>,
        symbol: char,
        ascii: char,
        style: impl Into<String>,
    ) -> Self {
        let ascii = if ascii.is_ascii_graphic() || ascii == ' ' {
            ascii
        } else {
            '?'
        };
        let symbol = if char_cell_width(symbol) == 1 {
            symbol
        } else {
            ascii
        };
        State {
            name: name.into(),
            symbol,
            ascii,
            style: style.into(),
        }
    }

    /// Passed: `✓`, `+` in ASCII, `chart.state.pass`.
    pub fn pass() -> Self {
        State::new("pass", '✓', '+', "chart.state.pass")
    }

    /// Failed: `✗`, `X` in ASCII, `chart.state.fail`.
    pub fn fail() -> Self {
        State::new("fail", '✗', 'X', "chart.state.fail")
    }

    /// Skipped: `○`, `-` in ASCII, `chart.state.skip`.
    pub fn skip() -> Self {
        State::new("skip", '○', '-', "chart.state.skip")
    }

    /// Passed only on a retry: `≈`, `~` in ASCII, `chart.state.flaky`.
    pub fn flaky() -> Self {
        State::new("flaky", '≈', '~', "chart.state.flaky")
    }

    /// The name cells refer to it by.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The symbol, ASCII when `ascii`.
    pub fn symbol(&self, ascii: bool) -> char {
        if ascii {
            self.ascii
        } else {
            self.symbol
        }
    }

    /// The style: a theme key or a definition.
    pub fn style(&self) -> &str {
        &self.style
    }
}

/// How a matrix fits its width.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grid {
    /// Cells for the row labels (0: none).
    label: usize,
    /// Cells per column.
    column: usize,
    /// Cells between columns.
    gap: usize,
    /// Columns that fit, from the first.
    shown: usize,
    /// Whether the column headers are written.
    headers: bool,
}

/// Rows and columns of states: a test suite across platforms, services
/// across regions, checks across hosts.
///
/// Each cell is a [`State`] named by a string. `pass`, `fail`, `skip` and
/// `flaky` are built in; [`state`](Self::state) adds your own or restyles a
/// built-in one. Every state is a **symbol and a colour**, never colour
/// alone, and the legend under the grid names each symbol with how many
/// cells are in that state. A name with no state is drawn `?` in
/// `chart.state.unknown` and named in the legend as it is.
///
/// - Columns are as wide as their widest header, with a cell between them,
///   and the symbol is centred. Given less width, headers are cut (with
///   `…`, or `.` in ASCII, and left out below three cells) down to the
///   symbol, then the row labels are cut,
///   then the gaps go; columns that still do not fit are left off the
///   right.
/// - A row shorter than the headers leaves its last cells blank.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::StatusMatrix;
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let matrix = StatusMatrix::new()
///     .columns(["linux", "macos", "win"])
///     .row("unit", ["pass", "pass", "fail"])
///     .row("e2e", ["pass", "flaky", "skip"]);
/// assert_eq!(
///     console.render_export(&matrix),
///     concat!(
///         "     linux macos  win                  \n",
///         "unit   ✓     ✓     ✗                   \n",
///         "e2e    ✓     ≈     ○                   \n",
///         "✓ pass 3  ✗ fail 1  ○ skip 1  ≈ flaky 1\n",
///     )
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct StatusMatrix {
    columns: Vec<String>,
    rows: Vec<(String, Vec<String>)>,
    states: Vec<State>,
    charset: Charset,
    legend: bool,
}

impl Default for StatusMatrix {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusMatrix {
    /// An empty matrix that knows `pass`, `fail`, `skip` and `flaky`.
    pub fn new() -> Self {
        StatusMatrix {
            columns: Vec::new(),
            rows: Vec::new(),
            states: vec![State::pass(), State::fail(), State::skip(), State::flaky()],
            charset: Charset::Auto,
            legend: true,
        }
    }

    /// The column headers.
    pub fn columns<S: Into<String>>(mut self, headers: impl IntoIterator<Item = S>) -> Self {
        self.columns = headers.into_iter().map(Into::into).collect();
        self
    }

    /// Add a row: its label and the name of each cell's state, in column
    /// order.
    pub fn row<S: Into<String>>(
        mut self,
        label: impl Into<String>,
        states: impl IntoIterator<Item = S>,
    ) -> Self {
        self.rows
            .push((label.into(), states.into_iter().map(Into::into).collect()));
        self
    }

    /// Add a state, or replace the one with the same name. The legend lists
    /// states in the order they were added (built-in ones first).
    pub fn state(mut self, state: State) -> Self {
        match self.states.iter_mut().find(|s| s.name == state.name) {
            Some(existing) => *existing = state,
            None => self.states.push(state),
        }
        self
    }

    /// Show the legend (default on).
    pub fn legend(mut self, show: bool) -> Self {
        self.legend = show;
        self
    }

    /// Glyphs to draw with: [`Charset::Ascii`] asks for the ASCII symbols;
    /// anything else draws the states' own symbols (ASCII on an ASCII-only
    /// console).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    fn find(&self, name: &str) -> Option<&State> {
        self.states.iter().find(|s| s.name == name)
    }

    /// How many cells are in each state that appears, in legend order:
    /// known states in the order they were added, then unknown names in the
    /// order they first appear.
    pub fn counts(&self) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> =
            self.states.iter().map(|s| (s.name.clone(), 0)).collect();
        for (_, cells) in &self.rows {
            for name in cells {
                match out.iter_mut().find(|(n, _)| n == name) {
                    Some((_, count)) => *count += 1,
                    None => out.push((name.clone(), 1)),
                }
            }
        }
        out.retain(|(_, count)| *count > 0);
        out
    }

    fn column_count(&self) -> usize {
        self.rows
            .iter()
            .map(|(_, c)| c.len())
            .max()
            .unwrap_or(0)
            .max(self.columns.len())
    }

    fn header(&self, i: usize) -> &str {
        self.columns.get(i).map(String::as_str).unwrap_or("")
    }

    fn label_width(&self) -> usize {
        self.rows.iter().map(|(l, _)| cells(l)).max().unwrap_or(0)
    }

    fn header_width(&self) -> usize {
        self.columns.iter().map(|h| cells(h)).max().unwrap_or(0)
    }

    fn natural_width(&self) -> usize {
        let n = self.column_count();
        let l = self.label_width();
        let c = self.header_width().max(1);
        l + usize::from(l > 0) + n * c + n.saturating_sub(1)
    }

    fn layout(&self, width: usize) -> Grid {
        let n = self.column_count();
        let l = self.label_width();
        let h = self.header_width();
        let natural = h.max(1);
        let part = |l: usize| l + usize::from(l > 0);
        let total = |l: usize, c: usize, gap: usize| part(l) + n * c + n.saturating_sub(1) * gap;
        if total(l, natural, 1) <= width {
            return Grid {
                label: l,
                column: natural,
                gap: 1,
                shown: n,
                headers: h > 0,
            };
        }
        // Narrow the columns, cutting the headers.
        let room = (width.saturating_sub(part(l)) + 1) / n.max(1);
        if room >= 2 {
            let column = room - 1;
            return Grid {
                label: l,
                column,
                gap: 1,
                shown: n,
                headers: h > 0 && (column >= 3 || h <= column),
            };
        }
        // Cut the row labels, keeping three cells of them.
        let keep = l.min(3);
        let headers = h == 1;
        if let Some(label) = width.checked_sub(total(0, 1, 1)) {
            let label = label.saturating_sub(1).min(l);
            if label >= keep && (label > 0 || l == 0) {
                return Grid {
                    label,
                    column: 1,
                    gap: 1,
                    shown: n,
                    headers,
                };
            }
        }
        // No gaps, then as many columns as fit.
        let label = if width > part(keep) { keep } else { 0 };
        let shown = n.min(width.saturating_sub(part(label)));
        Grid {
            label,
            column: 1,
            gap: 0,
            shown,
            headers,
        }
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
        let grid = self.layout(width);
        let label_style = colour.then(|| theme_style(console, "chart.label"));
        let unknown = colour.then(|| theme_style(console, "chart.state.unknown"));
        let state_style = |state: &State| colour.then(|| user_style(console, &state.style));
        let mut out = Vec::new();
        let lead = |line: &mut Line| {
            if grid.label > 0 {
                line.pad(grid.label + 1);
            }
        };
        if grid.headers && grid.shown > 0 {
            let mut line = Line::new();
            lead(&mut line);
            for i in 0..grid.shown {
                if i > 0 {
                    line.pad(grid.gap);
                }
                centre(
                    &mut line,
                    self.header(i),
                    grid.column,
                    label_style.clone(),
                    ascii,
                );
            }
            out.push(line);
        }
        for (label, states) in &self.rows {
            let mut line = Line::new();
            if grid.label > 0 {
                let text = truncate(label, grid.label, ascii);
                let pad = grid.label - cells(&text);
                line.push(&text, label_style.clone());
                line.pad(pad + 1);
            }
            for i in 0..grid.shown {
                if i > 0 {
                    line.pad(grid.gap);
                }
                let (symbol, style) = match states.get(i) {
                    None => (' ', None),
                    Some(name) => match self.find(name) {
                        Some(state) => (state.symbol(ascii), state_style(state)),
                        None => ('?', unknown.clone()),
                    },
                };
                let pad = grid.column - 1;
                line.pad(pad / 2);
                line.push(&symbol.to_string(), style);
                line.pad(pad - pad / 2);
            }
            out.push(line);
        }
        if self.legend {
            out.extend(wrap_entries(
                self.legend_entries(ascii, colour, console),
                width,
            ));
        }
        out
    }

    fn legend_entries(
        &self,
        ascii: bool,
        colour: bool,
        console: &Console,
    ) -> Vec<Vec<(String, Option<Style>)>> {
        let label_style = colour.then(|| theme_style(console, "chart.label"));
        self.counts()
            .into_iter()
            .map(|(name, count)| {
                let (symbol, style) = match self.find(&name) {
                    Some(state) => (
                        state.symbol(ascii),
                        colour.then(|| user_style(console, &state.style)),
                    ),
                    None => (
                        '?',
                        colour.then(|| theme_style(console, "chart.state.unknown")),
                    ),
                };
                let name = if ascii {
                    crate::fidelity::ascii_text(&name)
                } else {
                    name
                };
                vec![
                    (symbol.to_string(), style),
                    (format!(" {name} {count}"), label_style.clone()),
                ]
            })
            .collect()
    }
}

impl Renderable for StatusMatrix {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        lines_to_segments(self.lines(console, options), options.max_width)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let n = self.column_count();
        if n == 0 {
            return Measurement::new(7, 7).with_maximum(options.max_width);
        }
        let ascii = self.charset.resolve(console, options, Charset::Blocks) == Charset::Ascii;
        let legend = if self.legend {
            entries_width(&self.legend_entries(ascii, false, console))
        } else {
            0
        };
        let max = self.natural_width().max(legend);
        let min = (2 * n - 1).min(max);
        Measurement::new(min, max)
            .with_maximum(options.max_width)
            .normalize()
    }
}
