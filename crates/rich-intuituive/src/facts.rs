//! A description list: [`facts`], keys and their values in two columns.

use rich::{Console, Segment, Style};

use crate::node::{Axis, Node};
use crate::widget::{markup_line, markup_width, theme_style, Canvas, DrawCx, MeasureCx, Widget};

/// What [`facts`] draws.
struct Facts {
    rows: Box<dyn Fn() -> Vec<(String, String)>>,
    /// The keys' style (or theme style name), when not the theme's
    /// `facts.key`.
    key_style: Option<String>,
}

/// A description list: each key (console markup) in a column as wide as
/// the widest (at most half the width), its value (console markup) beside
/// it, wrapped under itself when it is long: a file's details, a
/// process's, a record's fields (Yazi's spotter, a `DataTable` of two
/// columns in Textual). The keys are in the theme's `facts.key` style
/// (bold), or [`key_style`](Node::key_style)'s, under their own markup.
/// Sized [`Size::Auto`](crate::Size::Auto), it is as tall as its rows
/// and as wide as its widest. Read aloud as `key: value` pairs.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::facts;
///
/// let app = App::new(|| {
///     facts([("Name", "notes.txt"), ("Size", "12 KB"), ("Modified", "today")])
///         .key_style("blue")
/// });
/// let mut driver = app.driver(24, 3);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "Name     notes.txt");
/// assert_eq!(driver.screen().plain()[2].trim_end(), "Modified today");
/// ```
pub fn facts<K: Into<String>, V: Into<String>>(rows: impl IntoIterator<Item = (K, V)>) -> Node {
    let rows: Vec<(String, String)> = rows
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect();
    facts_of(move || rows.clone())
}

/// [`facts`] from a closure, which may read signals: the details of
/// whatever is hovered.
pub fn facts_of(rows: impl Fn() -> Vec<(String, String)> + 'static) -> Node {
    crate::widget::widget(Facts {
        rows: Box::new(rows),
        key_style: None,
    })
    .what("facts")
}

impl Facts {
    /// The key column's width in `width`: the widest key, at most half.
    fn key_width(rows: &[(String, String)], width: u16) -> u16 {
        let widest = rows.iter().map(|(k, _)| markup_width(k)).max().unwrap_or(0);
        if width == 0 {
            widest
        } else {
            widest.min(width / 2)
        }
    }

    /// Value `markup`'s lines, wrapped to `width`.
    fn value_lines(console: &Console, markup: &str, width: u16) -> Vec<Vec<Segment>> {
        let text =
            rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup.to_string()));
        let options = console.options().update_width(width.max(1) as usize);
        let lines = console.render_lines(&text, &options, false);
        if lines.is_empty() {
            vec![Vec::new()]
        } else {
            lines
        }
    }

    /// Every row's lines, at `width`.
    fn lines(&self, console: &Console, key_style: &Style, width: u16) -> Vec<Vec<Segment>> {
        let rows = (self.rows)();
        let keys = Facts::key_width(&rows, width);
        let room = width.saturating_sub(keys + 1).max(1);
        let mut out = Vec::new();
        for (key, value) in &rows {
            let mut key_line: Vec<Segment> = markup_line(console, key, keys)
                .into_iter()
                .map(|piece| {
                    let style = match &piece.style {
                        Some(own) => key_style.combine(own),
                        None => key_style.clone(),
                    };
                    Segment::new(piece.text, Some(style))
                })
                .collect();
            let used: usize = key_line.iter().map(Segment::cell_length).sum();
            key_line.push(Segment::new(
                " ".repeat(keys as usize + 1 - used.min(keys as usize)),
                None,
            ));
            for (n, line) in Facts::value_lines(console, value, room)
                .into_iter()
                .enumerate()
            {
                let mut row = if n == 0 {
                    key_line.clone()
                } else {
                    vec![Segment::new(" ".repeat(keys as usize + 1), None)]
                };
                row.extend(line);
                out.push(row);
            }
        }
        out
    }

    fn key_style(&self, console: &Console) -> Style {
        match &self.key_style {
            Some(style) => theme_style(console, style, style),
            None => theme_style(console, "facts.key", "bold"),
        }
    }
}

impl Widget for Facts {
    fn name(&self) -> &'static str {
        "facts"
    }

    fn access_text(&self) -> Option<String> {
        let plain = |markup: &str| {
            rich::Text::from_markup(markup)
                .map(|t| t.plain().to_string())
                .unwrap_or_else(|_| markup.to_string())
        };
        let said: Vec<String> = (self.rows)()
            .iter()
            .map(|(k, v)| format!("{}: {}", plain(k), plain(v)))
            .collect();
        Some(said.join(". "))
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        let console = cx.console();
        match axis {
            Axis::Horizontal => {
                let rows = (self.rows)();
                let keys = Facts::key_width(&rows, 0);
                let values = rows
                    .iter()
                    .flat_map(|(_, v)| {
                        rich::Text::from_markup(v)
                            .map(|t| t.plain().to_string())
                            .unwrap_or_else(|_| v.clone())
                            .split('\n')
                            .map(rich::cells::cell_len)
                            .collect::<Vec<_>>()
                    })
                    .max()
                    .unwrap_or(0)
                    .min(u16::MAX as usize) as u16;
                let wanted = keys.saturating_add(1).saturating_add(values);
                if width == 0 {
                    wanted
                } else {
                    wanted.min(width)
                }
            }
            Axis::Vertical => {
                let style = self.key_style(console);
                let lines = self.lines(console, &style, width.max(2)).len();
                let lines = lines.min(u16::MAX as usize) as u16;
                if height == 0 {
                    lines
                } else {
                    lines.min(height)
                }
            }
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let console = cx.console();
        let style = self.key_style(console);
        let lines = self.lines(console, &style, canvas.width());
        canvas.lines(&lines);
    }
}

impl Node {
    /// For [`facts`]: the keys' style (or theme style name) instead of the
    /// theme's `facts.key`. Ignored on other nodes.
    pub fn key_style(self, style: &str) -> Node {
        let style = style.to_string();
        self.widget_mut(|facts: &mut Facts| facts.key_style = Some(style));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_value_wraps_under_itself_and_keys_are_styled() {
        let app = crate::App::new(|| {
            facts([
                ("Path", "/home/user/some/deep/folder"),
                ("Size", "[i]4[/] KB"),
            ])
            .key_style("blue")
        });
        let mut driver = app.driver(20, 4);
        driver.settle();
        let rows = driver.screen().plain();
        assert_eq!(rows[0].trim_end(), "Path /home/user/some");
        assert_eq!(rows[1].trim_end(), "     /deep/folder");
        assert_eq!(rows[2].trim_end(), "Size 4 KB");
        let styled = crate::testing::styled(&driver);
        assert!(styled.contains("[blue]Path"), "{styled}");
        assert_eq!(
            driver.accessibility()[0].name,
            "Path: /home/user/some/deep/folder. Size: 4 KB"
        );
    }
}
