//! Big text: console markup drawn in FIGlet letters by rs-rich-art, as
//! tui-big-text draws a clock or a readout.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text};
use rich_art::figlet::{self, FigletFont};
use rich_art::Justify;

use crate::node::{described, Node};

/// Console markup in FIGlet letters (rs-rich-art's bundled `standard`
/// font, six rows a line), from a closure that may read signals: a large
/// clock, a level readout, a title. Each run of the markup keeps its style
/// (`"[green]12[/]:[red]34"`), so letters of different styles do not
/// smush into each other as FIGlet's do within a run; each line of the
/// markup is a line of letters. What does not fit the width is cut. Its
/// accessible name is the text itself.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::big_text;
///
/// let app = App::new(|| big_text(|| "[b]42".into()).on_key("q", |cx| cx.quit()));
/// let screen = app.render_with(&["q"], 12, 6).unwrap();
/// assert_eq!(screen[0].trim_end(), " _  _  ____");
/// assert_eq!(screen[1].trim_end(), "| || ||___ \\");
/// ```
pub fn big_text(markup: impl Fn() -> String + 'static) -> Node {
    let font = std::rc::Rc::new(FigletFont::standard());
    described(move || BigText {
        markup: markup(),
        font: font.clone(),
    })
}

/// What [`big_text`] draws.
struct BigText {
    markup: String,
    font: std::rc::Rc<FigletFont>,
}

impl BigText {
    /// The markup's lines, each as runs of text in one style.
    fn runs(&self, console: &Console) -> Vec<Vec<(String, Option<Style>)>> {
        let text =
            Text::from_markup(&self.markup).unwrap_or_else(|_| Text::new(self.markup.as_str()));
        let mut options = console.options().update_width(usize::from(u16::MAX));
        options.no_wrap = Some(true);
        let mut lines = vec![Vec::new()];
        for segment in console.render(&text, Some(&options)) {
            if segment.control {
                continue;
            }
            let mut parts = segment.text.split('\n');
            if let Some(first) = parts.next() {
                push_run(lines.last_mut(), first, &segment.style);
            }
            for part in parts {
                lines.push(Vec::new());
                push_run(lines.last_mut(), part, &segment.style);
            }
        }
        // The renderer's padding to the width, and an empty last line.
        for line in &mut lines {
            while line
                .last()
                .is_some_and(|(t, s): &(String, _)| s.is_none() && t.trim().is_empty())
            {
                line.pop();
            }
            if let Some((last, _)) = line.last_mut() {
                let kept = last.trim_end().len();
                last.truncate(kept);
            }
        }
        while lines.len() > 1 && lines.last().is_some_and(Vec::is_empty) {
            lines.pop();
        }
        lines
    }

    /// The letters, as rows of styled cells.
    fn rows(&self, console: &Console) -> Vec<Vec<(String, Option<Style>)>> {
        let height = self.font.height();
        let mut out = Vec::new();
        for line in self.runs(console) {
            let mut rows: Vec<Vec<(String, Option<Style>)>> = vec![Vec::new(); height];
            for (text, style) in line {
                if text.is_empty() {
                    continue;
                }
                let drawn = figlet::render(&text, &self.font, usize::MAX / 2, Justify::Left);
                let lines: Vec<&str> = drawn.lines().collect();
                let width = lines.iter().map(|l| cell_width(l)).max().unwrap_or(0);
                for (r, row) in rows.iter_mut().enumerate() {
                    let part = lines.get(r).copied().unwrap_or_default();
                    let padded = format!("{part}{}", " ".repeat(width - cell_width(part)));
                    row.push((padded, style.clone()));
                }
            }
            out.extend(rows);
        }
        out
    }
}

fn cell_width(text: &str) -> usize {
    rich::cells::cell_len(text)
}

fn push_run(line: Option<&mut Vec<(String, Option<Style>)>>, text: &str, style: &Option<Style>) {
    if let (Some(line), false) = (line, text.is_empty()) {
        line.push((text.to_string(), style.clone()));
    }
}

impl Renderable for BigText {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let mut out = Vec::new();
        for (index, row) in self.rows(console).into_iter().enumerate() {
            if index > 0 {
                out.push(Segment::line());
            }
            let mut used = 0;
            for (text, style) in row {
                if used >= width {
                    break;
                }
                let room = width - used;
                let cut: String = if cell_width(&text) > room {
                    rich::cells::set_cell_size(&text, room)
                } else {
                    text
                };
                used += cell_width(&cut);
                out.push(Segment::new(cut, style));
            }
        }
        out
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let widest = self
            .rows(console)
            .iter()
            .map(|row| row.iter().map(|(t, _)| cell_width(t)).sum::<usize>())
            .max()
            .unwrap_or(0);
        Measurement::new(widest.min(1), widest).with_maximum(options.max_width)
    }
}

impl rich_ext::a11y::AccessibleText for BigText {
    fn accessible_text(&self, _width: usize) -> String {
        Text::from_markup(&self.markup)
            .map(|t| t.plain().to_string())
            .unwrap_or_else(|_| self.markup.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::signal;

    #[test]
    fn each_line_of_markup_is_a_line_of_letters() {
        let app = crate::App::new(|| {
            let n = signal(1);
            big_text(move || format!("{}\n2", n.get()))
                .on_key("+", move |_| n.set(7))
                .on_key("q", |cx| cx.quit())
        });
        let screen = app.render_with(&["+", "q"], 10, 12).unwrap();
        assert_eq!(screen[0].trim_end(), " _____");
        assert_eq!(screen[1].trim_end(), "|___  |");
        assert_eq!(screen[4].trim_end(), " /_/");
        // The second line six rows down.
        assert_eq!(screen[6].trim_end(), " ____");
        assert_eq!(screen[10].trim_end(), "|_____|");
    }

    #[test]
    fn runs_keep_their_styles_and_the_width_cuts() {
        let console = Console::builder()
            .width(8)
            .force_terminal(true)
            .color_system(Some(rich::ColorSystem::Standard))
            .build();
        let big = BigText {
            markup: "[red]1[/][green]1".into(),
            font: std::rc::Rc::new(FigletFont::standard()),
        };
        let out = console.render_export(&big);
        assert!(out.contains("\u{1b}[31m"), "{out:?}");
        assert!(out.contains("\u{1b}[32m"), "{out:?}");
        let rows: Vec<Vec<(String, Option<Style>)>> = big.rows(&console);
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0].len(), 2);
        let plain = Console::builder().width(3).color_system(None).build();
        assert!(plain
            .render_export(&big)
            .lines()
            .all(|l| l.chars().count() <= 3));
    }

    #[test]
    fn it_reads_as_its_text() {
        use rich_ext::a11y::AccessibleText;
        let big = BigText {
            markup: "[b]12:34".into(),
            font: std::rc::Rc::new(FigletFont::standard()),
        };
        assert_eq!(big.accessible_text(80), "12:34");
    }
}
