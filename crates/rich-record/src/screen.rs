//! What the terminal shows at one moment, resolved to colours.

use rich::{Color, Segment, Style};
use rich_ext::frame::Frame;

/// An RGB colour.
pub type Rgb = (u8, u8, u8);

/// The palette a recording is drawn in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub background: Rgb,
    pub foreground: Rgb,
    /// The 16 ANSI colours: 0–7, then their bright forms.
    pub ansi: [Rgb; 16],
}

impl Default for Theme {
    /// rich's SVG export theme (`terminal_theme::SVG_EXPORT_THEME`), so
    /// recordings match the docs' other screenshots.
    fn default() -> Self {
        let theme = rich::terminal_theme::SVG_EXPORT_THEME;
        let rgb = |c: rich::ColorTriplet| (c.red, c.green, c.blue);
        let mut ansi = [(0, 0, 0); 16];
        for (slot, colour) in ansi.iter_mut().zip(theme.ansi) {
            *slot = rgb(colour);
        }
        Theme {
            background: rgb(theme.background),
            foreground: rgb(theme.foreground),
            ansi,
        }
    }
}

impl Theme {
    /// The theme as rich's exporters take it.
    pub fn terminal_theme(&self) -> rich::terminal_theme::TerminalTheme {
        let triplet = |(r, g, b): Rgb| rich::ColorTriplet::new(r, g, b);
        rich::terminal_theme::TerminalTheme {
            background: triplet(self.background),
            foreground: triplet(self.foreground),
            ansi: self.ansi.map(triplet),
        }
    }

    /// A colour from the xterm 256-colour palette: the theme's 16, the
    /// 6×6×6 cube, then the grey ramp.
    pub fn indexed(&self, index: u8) -> Rgb {
        match index {
            0..=15 => self.ansi[index as usize],
            16..=231 => {
                let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                let n = index - 16;
                (level(n / 36), level(n / 6 % 6), level(n % 6))
            }
            _ => {
                let grey = 8 + (index - 232) * 10;
                (grey, grey, grey)
            }
        }
    }

    fn resolve(&self, colour: vt100::Color, default: Rgb, brighten: bool) -> Rgb {
        match colour {
            vt100::Color::Default => default,
            vt100::Color::Idx(index) if brighten && index < 8 => self.ansi[index as usize + 8],
            vt100::Color::Idx(index) => self.indexed(index),
            vt100::Color::Rgb(r, g, b) => (r, g, b),
        }
    }
}

/// One terminal cell. A wide character is followed by a continuation cell
/// with empty text and width 0.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Cell {
    pub text: String,
    pub width: u8,
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Cell {
    fn blank(theme: &Theme) -> Cell {
        Cell {
            text: " ".into(),
            width: 1,
            fg: theme.foreground,
            bg: theme.background,
            bold: false,
            italic: false,
            underline: false,
        }
    }

    pub fn is_continuation(&self) -> bool {
        self.width == 0
    }

    /// Whether two cells differ only in text: runs of such cells are drawn
    /// as one.
    pub fn same_style(&self, other: &Cell) -> bool {
        (self.fg, self.bg, self.bold, self.italic, self.underline)
            == (
                other.fg,
                other.bg,
                other.bold,
                other.italic,
                other.underline,
            )
    }
}

/// The screen: rows of cells and the cursor, when it is shown.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Snapshot {
    pub rows: Vec<Vec<Cell>>,
    pub cursor: Option<(u16, u16)>,
}

impl Snapshot {
    /// Read a vt100 screen through `theme`. Reverse video swaps the colours;
    /// bold turns the eight standard foreground colours bright, as most
    /// terminals do; dim blends the foreground halfway to the background.
    pub fn from_screen(screen: &vt100::Screen, theme: &Theme) -> Snapshot {
        Snapshot::read(screen, theme, std::borrow::Cow::Borrowed)
    }

    /// [`Snapshot::from_screen`], with each cell's text passed through
    /// `text` (see [`crate::terminal::Terminal`]).
    pub(crate) fn read<'a>(
        screen: &'a vt100::Screen,
        theme: &Theme,
        text: impl Fn(&'a str) -> std::borrow::Cow<'a, str>,
    ) -> Snapshot {
        let (height, width) = screen.size();
        let mut rows = Vec::with_capacity(height as usize);
        for y in 0..height {
            let mut row = Vec::with_capacity(width as usize);
            for x in 0..width {
                let Some(cell) = screen.cell(y, x) else {
                    row.push(Cell::blank(theme));
                    continue;
                };
                let mut fg = theme.resolve(cell.fgcolor(), theme.foreground, cell.bold());
                let mut bg = theme.resolve(cell.bgcolor(), theme.background, false);
                if cell.inverse() {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if cell.dim() {
                    fg = (
                        ((fg.0 as u16 + bg.0 as u16) / 2) as u8,
                        ((fg.1 as u16 + bg.1 as u16) / 2) as u8,
                        ((fg.2 as u16 + bg.2 as u16) / 2) as u8,
                    );
                }
                let (text, width) = if cell.is_wide_continuation() {
                    (String::new(), 0)
                } else if cell.has_contents() {
                    (
                        text(cell.contents()).into_owned(),
                        if cell.is_wide() { 2 } else { 1 },
                    )
                } else {
                    (" ".to_string(), 1)
                };
                row.push(Cell {
                    text,
                    width,
                    fg,
                    bg,
                    bold: cell.bold(),
                    italic: cell.italic(),
                    underline: cell.underline(),
                });
            }
            rows.push(row);
        }
        let cursor = (!screen.hide_cursor()).then(|| {
            let (row, column) = screen.cursor_position();
            (column, row)
        });
        Snapshot { rows, cursor }
    }

    pub fn columns(&self) -> usize {
        self.rows.first().map_or(0, Vec::len)
    }

    /// The screen as rich segments, one run per stretch of equal style and
    /// a line break between rows: what [`Snapshot::to_frame`] builds on.
    pub fn to_segments(&self) -> Vec<Segment> {
        let mut segments = Vec::new();
        for (y, row) in self.rows.iter().enumerate() {
            if y > 0 {
                segments.push(Segment::line());
            }
            let mut run = String::new();
            let mut style: Option<&Cell> = None;
            for cell in row.iter().filter(|cell| !cell.is_continuation()) {
                if style.is_some_and(|s| !s.same_style(cell)) {
                    segments.push(Segment::new(
                        std::mem::take(&mut run),
                        style.map(rich_style),
                    ));
                }
                style = Some(cell);
                run.push_str(&cell.text);
            }
            if let Some(style) = style {
                segments.push(Segment::new(run, Some(rich_style(style))));
            }
        }
        segments
    }

    /// The screen as a [`Frame`], for cell diffs, snapshots and the text
    /// grid.
    pub fn to_frame(&self) -> Frame {
        Frame::from_segments(&self.to_segments())
    }

    /// The screen as a [`Frame`] for rich-ext's exporters: the theme's own
    /// foreground and background are left unset, so an export draws them as
    /// its theme's defaults (no background rectangle behind every cell).
    pub fn export_frame(&self, theme: &Theme) -> Frame {
        Frame::from_segments(&self.export_segments(theme))
    }

    fn export_segments(&self, theme: &Theme) -> Vec<Segment> {
        let mut segments = Vec::new();
        for (y, row) in self.rows.iter().enumerate() {
            if y > 0 {
                segments.push(Segment::line());
            }
            let mut run = String::new();
            let mut style: Option<&Cell> = None;
            for cell in row.iter().filter(|cell| !cell.is_continuation()) {
                if style.is_some_and(|s| !s.same_style(cell)) {
                    let done = style.map(|cell| export_style(cell, theme));
                    segments.push(Segment::new(std::mem::take(&mut run), done));
                }
                style = Some(cell);
                run.push_str(&cell.text);
                // The exporters place text by rich's cell widths, the grid by
                // the emulator's; they disagree on some characters (skin-tone
                // modifiers, Indic marks). Pad a cell rich measures narrower,
                // so every column after it lands where the grid has it.
                let measured = rich::cells::cell_len(&cell.text);
                for _ in measured..usize::from(cell.width) {
                    run.push(' ');
                }
            }
            if let Some(style) = style {
                segments.push(Segment::new(run, Some(export_style(style, theme))));
            }
        }
        segments
    }

    /// The text of every row, trailing spaces removed: what `--check`
    /// compares.
    pub fn text_grid(&self) -> String {
        let frame = self.to_frame();
        let mut out = String::new();
        for line in frame.plain().split('\n') {
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }
}

/// [`rich_style`], without the theme's default colours.
fn export_style(cell: &Cell, theme: &Theme) -> Style {
    let mut style = attributes(cell);
    if cell.fg != theme.foreground {
        let (r, g, b) = cell.fg;
        style = style.with_color(Color::from_rgb(r, g, b));
    }
    if cell.bg != theme.background {
        let (r, g, b) = cell.bg;
        style = style.with_bgcolor(Color::from_rgb(r, g, b));
    }
    style
}

fn attributes(cell: &Cell) -> Style {
    let mut attrs = Vec::new();
    if cell.bold {
        attrs.push("bold");
    }
    if cell.italic {
        attrs.push("italic");
    }
    if cell.underline {
        attrs.push("underline");
    }
    if attrs.is_empty() {
        Style::new()
    } else {
        Style::parse(&attrs.join(" ")).expect("known attributes")
    }
}

fn rich_style(cell: &Cell) -> Style {
    let base = attributes(cell);
    let (r, g, b) = cell.fg;
    let (br, bg, bb) = cell.bg;
    base.with_color(Color::from_rgb(r, g, b))
        .with_bgcolor(Color::from_rgb(br, bg, bb))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(bytes: &[u8], rows: u16, columns: u16) -> Snapshot {
        let mut parser = vt100::Parser::new(rows, columns, 0);
        parser.process(bytes);
        Snapshot::from_screen(parser.screen(), &Theme::default())
    }

    #[test]
    fn colours_attributes_and_wide_characters() {
        let theme = Theme::default();
        let shot = snapshot(
            "\x1b[1;31mA\x1b[0m\x1b[7mB\x1b[0m漢\x1b[38;2;1;2;3mC".as_bytes(),
            2,
            8,
        );
        let row = &shot.rows[0];
        assert_eq!(row[0].fg, theme.ansi[9], "bold red is bright red");
        assert!(row[0].bold);
        assert_eq!((row[1].fg, row[1].bg), (theme.background, theme.foreground));
        assert_eq!((row[2].text.as_str(), row[2].width), ("漢", 2));
        assert!(row[3].is_continuation());
        assert_eq!(row[4].fg, (1, 2, 3));
    }

    #[test]
    fn text_grid_comes_from_the_frame() {
        let shot = snapshot(b"hello\r\n  world  ", 3, 10);
        assert_eq!(shot.text_grid(), "hello\n  world\n\n");
        assert_eq!(shot.to_frame().height(), 3);
        assert_eq!(shot.cursor, Some((9, 1)));
    }

    #[test]
    fn exports_keep_the_grid_columns_where_rich_measures_differently() {
        // rich gives a skin-tone modifier and a Devanagari vowel sign no
        // width; the emulator gives them cells. The export must still put
        // `XY` in the grid's column, or the SVG draws it (and its red
        // background) cells too far left.
        let theme = Theme::default();
        for line in ["👍🏽ab \x1b[41mXY\x1b[0m|", "नाम ABCDEFGH \x1b[41mXY\x1b[0m|"] {
            let shot = snapshot(line.as_bytes(), 2, 40);
            let grid = shot.rows[0].iter().position(|c| c.text == "X").unwrap();
            let frame = shot.export_frame(&theme);
            let mut column = 0;
            let mut exported = None;
            for run in frame.row(0) {
                let text = frame.run_text(run);
                if let Some(offset) = text.find("XY") {
                    exported = Some(column + rich::cells::cell_len(&text[..offset]));
                    break;
                }
                column += run.cells();
            }
            assert_eq!(exported, Some(grid), "{line:?}");
        }
    }

    #[test]
    fn indexed_palette() {
        let theme = Theme::default();
        assert_eq!(theme.indexed(16), (0, 0, 0));
        assert_eq!(theme.indexed(231), (255, 255, 255));
        assert_eq!(theme.indexed(232), (8, 8, 8));
        assert_eq!(theme.indexed(1), theme.ansi[1]);
    }
}
