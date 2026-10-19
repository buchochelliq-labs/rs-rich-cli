//! Big digits drawn in box-drawing glyphs, three rows tall: [`digits`],
//! Textual's `Digits`, in every build (no FIGlet font needed).

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text};

use crate::node::{described, Node};

/// The rows a glyph takes.
const ROWS: usize = 3;

/// The three rows of character `c`'s glyph, or `None` for a character
/// without one.
fn glyph(c: char) -> Option<[&'static str; ROWS]> {
    Some(match c.to_ascii_uppercase() {
        '0' => ["┌─┐", "│ │", "└─┘"],
        '1' => [" ┐ ", " │ ", "╶┴╴"],
        '2' => ["╶─┐", "┌─┘", "└─╴"],
        '3' => ["╶─┐", " ─┤", "╶─┘"],
        '4' => ["╷ ╷", "└─┤", "  ╵"],
        '5' => ["┌─╴", "└─┐", "╶─┘"],
        '6' => ["┌─╴", "├─┐", "└─┘"],
        '7' => ["╶─┐", "  │", "  ╵"],
        '8' => ["┌─┐", "├─┤", "└─┘"],
        '9' => ["┌─┐", "└─┤", "╶─┘"],
        'A' => ["┌─┐", "├─┤", "╵ ╵"],
        'B' => ["┌┐ ", "├┴┐", "└─┘"],
        'C' => ["┌─╴", "│  ", "└─╴"],
        'D' => ["┌┐ ", "│└┐", "└─┘"],
        'E' => ["┌─╴", "├─ ", "└─╴"],
        'F' => ["┌─╴", "├─ ", "╵  "],
        'G' => ["┌─╴", "│╶┐", "└─┘"],
        '#' => ["┼┼", "┼┼", "  "],
        '+' => ["   ", "╶┼╴", "   "],
        '-' => ["   ", "╶─╴", "   "],
        '=' => ["   ", "╶─╴", "╶─╴"],
        '/' => ["  ╱", " ╱ ", "╱  "],
        '%' => ["╷ ╱", " ╱ ", "╱ ╵"],
        '(' => ["┌", "│", "└"],
        ')' => ["┐", "│", "┘"],
        ':' => [" ", ":", " "],
        '.' => [" ", " ", "."],
        ',' => [" ", " ", ","],
        ' ' => [" ", " ", " "],
        _ => return None,
    })
}

/// What [`digits`] draws.
struct Digits {
    markup: String,
}

impl Digits {
    /// The markup's characters, line by line, each with its style.
    fn chars(&self, console: &Console) -> Vec<Vec<(char, Option<Style>)>> {
        let text =
            Text::from_markup(&self.markup).unwrap_or_else(|_| Text::new(self.markup.as_str()));
        let mut lines = vec![Vec::new()];
        for segment in text.render(console.theme(), &Style::default()) {
            if segment.text == "\n" {
                lines.push(Vec::new());
                continue;
            }
            for c in segment.text.chars() {
                if c == '\n' {
                    lines.push(Vec::new());
                } else if let Some(line) = lines.last_mut() {
                    line.push((c, segment.style.clone()));
                }
            }
        }
        lines
    }

    /// The glyphs, as rows of styled cells: three rows for each line.
    fn rows(&self, console: &Console) -> Vec<Vec<(String, Option<Style>)>> {
        let mut out = Vec::new();
        for line in self.chars(console) {
            let mut rows: Vec<Vec<(String, Option<Style>)>> = vec![Vec::new(); ROWS];
            for (c, style) in line {
                let shown = glyph(c).map(|g| g.map(String::from)).unwrap_or_else(|| {
                    // No glyph: the character itself, on the middle row.
                    let blank = " ".repeat(rich::cells::cell_len(&c.to_string()));
                    [blank.clone(), c.to_string(), blank]
                });
                for (row, part) in rows.iter_mut().zip(shown) {
                    row.push((part, style.clone()));
                }
            }
            out.extend(rows);
        }
        out
    }
}

impl Renderable for Digits {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let mut out = Vec::new();
        for (index, row) in self.rows(console).into_iter().enumerate() {
            if index > 0 {
                out.push(Segment::line());
            }
            let mut used = 0;
            for (text, style) in row {
                let cells = rich::cells::cell_len(&text);
                if used + cells > width {
                    break;
                }
                used += cells;
                out.push(Segment::new(text, style));
            }
        }
        out
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let widest = self
            .rows(console)
            .iter()
            .map(|row| {
                row.iter()
                    .map(|(t, _)| rich::cells::cell_len(t))
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(0);
        Measurement::new(widest.min(1), widest).with_maximum(options.max_width)
    }
}

impl rich_ext::a11y::AccessibleText for Digits {
    fn accessible_text(&self, _width: usize) -> String {
        Text::from_markup(&self.markup)
            .map(|t| t.plain().to_string())
            .unwrap_or_else(|_| self.markup.clone())
    }
}

/// Console markup drawn three rows tall in box-drawing glyphs, from a
/// closure that may read signals: a clock, a counter, a readout, as
/// Textual's `Digits`. It has glyphs for `0`-`9`, `A`-`G` (and `a`-`g`:
/// hex digits, and the notes with `#`), `+ - = / % ( ) : . , #` and the
/// space; any other character is drawn as
/// it is, on the middle row. Each run of the markup keeps its style
/// (`"[green]12[/]:[red]34"`); each line of the markup is three rows. It
/// needs no font, so it works in every build, where
/// `big_text` needs the `art` feature. A glyph that
/// does not fit the width is left out. Its accessible name is the text.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::digits;
///
/// let app = App::new(|| digits(|| "12:30".into()));
/// let mut driver = app.driver(14, 3);
/// driver.settle();
/// let rows = driver.screen().plain();
/// assert_eq!(rows[0].trim_end(), " ┐ ╶─┐ ╶─┐┌─┐");
/// assert_eq!(rows[1].trim_end(), " │ ┌─┘: ─┤│ │");
/// assert_eq!(rows[2].trim_end(), "╶┴╴└─╴ ╶─┘└─┘");
/// ```
pub fn digits(markup: impl Fn() -> String + 'static) -> Node {
    described(move || Digits { markup: markup() }).what("digits")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_three_rows_of_one_width() {
        for c in "0123456789ABCDEFG+-=/%():.,# ".chars() {
            let rows = glyph(c).unwrap();
            let width = rich::cells::cell_len(rows[0]);
            assert!(
                rows.iter().all(|r| rich::cells::cell_len(r) == width),
                "{c}"
            );
        }
        assert_eq!(glyph('a'), glyph('A'));
        assert_eq!(glyph('x'), None);
    }

    #[test]
    fn runs_keep_their_styles_and_lines_stack() {
        let console = Console::builder().width(20).build();
        let shown = Digits {
            markup: "[red]1[/]x\n2".into(),
        };
        let rows = shown.rows(&console);
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[1][1].0, "x");
        assert!(rows[0][0].1.as_ref().is_some_and(|s| s.color().is_some()));
        assert_eq!(rows[3][0].0, "╶─┐");
    }
}
