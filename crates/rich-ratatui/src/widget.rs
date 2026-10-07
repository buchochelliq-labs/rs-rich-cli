//! [`RichWidget`]: any rich renderable as a ratatui widget.

use std::cell::OnceCell;

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::widgets::Widget;
use rich::{ColorSystem, Console, Renderable, Segment};

use crate::buffer::lines_to_buffer;

thread_local! {
    /// The console a [`RichWidget`] renders with when given none. Built
    /// once per thread with everything set explicitly, so building it reads
    /// no environment (`NO_COLOR`, terminal detection): the colours a
    /// ratatui app shows are the app's business, decided by its backend.
    static DEFAULT_CONSOLE: OnceCell<Console> = const { OnceCell::new() };
}

fn with_default_console<R>(f: impl FnOnce(&Console) -> R) -> R {
    DEFAULT_CONSOLE.with(|cell| {
        f(cell.get_or_init(|| {
            Console::builder()
                .width(80)
                .height(25)
                .force_terminal(true)
                .color_system(Some(ColorSystem::Truecolor))
                .no_color(false)
                .build()
        }))
    })
}

/// What a [`RichWidget`] draws: a borrowed renderable or one it owns.
enum Content<'a> {
    Borrowed(&'a dyn Renderable),
    Owned(Box<dyn Renderable + 'a>),
}

/// A ratatui [`Widget`] drawing a rich renderable: a `Table`, a `Panel`,
/// `Markdown`, `Syntax`, a `Tree`, markup, or anything else implementing
/// [`Renderable`].
///
/// Use rich in your ratatui app by handing a renderable to
/// `frame.render_widget`, by reference or by value:
///
/// ```
/// use ratatui_core::backend::TestBackend;
/// use ratatui_core::layout::Rect;
/// use ratatui_core::terminal::Terminal;
/// use rich::Table;
/// use rich_ratatui::RichWidget;
///
/// let mut table = Table::new();
/// table.add_column("service");
/// table.add_column("status");
/// table.add_row(&["api", "[green]ok[/]"]);
/// table.add_row(&["worker", "[bold red]failed[/]"]);
///
/// let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
/// terminal
///     .draw(|frame| {
///         // A borrowed renderable: the table outlives the frame.
///         frame.render_widget(&RichWidget::new(&table), Rect::new(0, 0, 40, 6));
///         // An owned one: markup, drawn in the last row.
///         frame.render_widget(RichWidget::markup("[b]q[/] quit"), Rect::new(0, 7, 40, 1));
///     })
///     .unwrap();
///
/// let buffer = terminal.backend().buffer();
/// assert_eq!(buffer[(0, 0)].symbol(), "┏");
/// assert_eq!(buffer[(0, 7)].symbol(), "q");
/// assert!(buffer[(0, 7)].modifier.contains(ratatui_core::style::Modifier::BOLD));
/// ```
///
/// The renderable is rendered at the area's width, with the given
/// [console](Self::console) or a default one; its first `area.height` lines
/// are drawn and the rest are cut off, as ratatui's `Paragraph` cuts off
/// text. Lines are written by [`lines_to_buffer`]: styles patch the cells
/// beneath (a `Block`'s background shows through unstyled text), and each
/// segment sits at the column rich measured for it.
pub struct RichWidget<'a> {
    content: Content<'a>,
    console: Option<&'a Console>,
    fill_height: bool,
}

impl<'a> RichWidget<'a> {
    /// Draw a borrowed renderable.
    pub fn new(renderable: &'a dyn Renderable) -> RichWidget<'a> {
        RichWidget {
            content: Content::Borrowed(renderable),
            console: None,
            fill_height: false,
        }
    }

    /// Draw a renderable the widget owns.
    pub fn owned(renderable: impl Renderable + 'a) -> RichWidget<'a> {
        RichWidget {
            content: Content::Owned(Box::new(renderable)),
            console: None,
            fill_height: false,
        }
    }

    /// Draw console markup (`[bold red]hi[/] there`). Markup that does not
    /// parse is drawn as plain text, as rich-interact's `Context::markup`
    /// does.
    pub fn markup(markup: &str) -> RichWidget<'static> {
        let text = rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup));
        RichWidget::owned(text)
    }

    /// Render with `console`: its theme, highlighting, emoji and box
    /// settings. Its width is ignored (the area's is used).
    ///
    /// Without one, a per-thread default console is used: the default
    /// theme, truecolor, highlighting on, as upstream's `Console()`
    /// defaults, built without reading the environment, so `NO_COLOR` and
    /// terminal detection do not reach into a ratatui app.
    pub fn console(mut self, console: &'a Console) -> RichWidget<'a> {
        self.console = Some(console);
        self
    }

    /// Also tell the renderable the area's height, so renderables that use
    /// it (`Panel` with a height, `Layout`) fill the area instead of taking
    /// their natural height. Off by default, matching rich-interact's
    /// `Context::lines`.
    pub fn fill_height(mut self, fill: bool) -> RichWidget<'a> {
        self.fill_height = fill;
        self
    }

    fn renderable(&self) -> &dyn Renderable {
        match &self.content {
            Content::Borrowed(r) => *r,
            Content::Owned(r) => r.as_ref(),
        }
    }

    /// The rich lines this widget draws into `area`: at most `area.height`
    /// of them, rendered at `area.width` (at least 1).
    pub fn lines(&self, area: Rect) -> Vec<Vec<Segment>> {
        let render = |console: &Console| {
            let width = usize::from(area.width).max(1);
            let options = if self.fill_height {
                console
                    .options()
                    .update_dimensions(width, usize::from(area.height))
            } else {
                console.options().update_width(width)
            };
            let mut lines = console.render_lines(self.renderable(), &options, false);
            lines.truncate(usize::from(area.height));
            lines
        };
        match self.console {
            Some(console) => render(console),
            None => with_default_console(render),
        }
    }
}

impl Widget for &RichWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        lines_to_buffer(&self.lines(area), area, buf);
    }
}

impl Widget for RichWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::buffer_to_lines;
    use ratatui_core::style::{Color as RColor, Style as RStyle};
    use rich::color::ColorType;
    use rich::Panel;

    /// A console like the default one, but fixed at `width` for the
    /// reference renderings.
    fn console(width: usize) -> Console {
        Console::builder()
            .width(width)
            .height(25)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .build()
    }

    fn plain(lines: &[Vec<Segment>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect())
            .collect()
    }

    fn buffer_rows(buffer: &Buffer) -> Vec<String> {
        plain(&buffer_to_lines(buffer))
    }

    /// Lines as (text, SGR) runs, adjacent runs with the same SGR merged:
    /// what a terminal would show, whatever the segmentation and colour
    /// names.
    fn normalise(lines: &[Vec<Segment>]) -> Vec<Vec<(String, String)>> {
        lines
            .iter()
            .map(|line| {
                let mut runs: Vec<(String, String)> = Vec::new();
                for segment in line.iter().filter(|s| !s.control && !s.text.is_empty()) {
                    let sgr = segment
                        .style
                        .as_ref()
                        .map(|s| s.ansi_codes(ColorSystem::Truecolor))
                        .unwrap_or_default();
                    match runs.last_mut() {
                        Some((text, last)) if *last == sgr => text.push_str(&segment.text),
                        _ => runs.push((segment.text.clone(), sgr)),
                    }
                }
                runs
            })
            .collect()
    }

    #[test]
    fn rich_widget_matches_richs_plain_rendering() {
        let mut table = rich::Table::new();
        table.add_column("name");
        table.add_column("qty");
        table.add_row(&["apples", "3"]);
        table.add_row(&["pears", "12"]);
        let area = Rect::new(0, 0, 30, 6);
        let mut buffer = Buffer::empty(area);
        RichWidget::new(&table).render(area, &mut buffer);

        let expected = console(30).export_text(|c| c.print(&table));
        let expected: Vec<&str> = expected.lines().collect();
        assert_eq!(expected.len(), 6, "{expected:?}");
        // The table is not expanded, so rich's lines are 16 cells and the
        // buffer's other 14 columns are untouched blanks.
        let rows = buffer_rows(&buffer);
        assert_eq!(
            rows.iter().map(|r| r.trim_end()).collect::<Vec<_>>(),
            expected
        );

        // A panel expands: full-width lines, compared as they are.
        let panel = Panel::new(Box::new(table)).title("stock");
        let mut buffer = Buffer::empty(area);
        RichWidget::new(&panel).render(area, &mut buffer);
        let expected = console(30).export_text(|c| c.print(&panel));
        let expected: Vec<&str> = expected.lines().take(6).collect();
        assert_eq!(buffer_rows(&buffer), expected);
    }

    #[test]
    fn rich_widget_crops_to_the_area_and_patches_styles() {
        let panel = Panel::new(Box::new(rich::Text::new("one\ntwo\nthree\nfour")));
        // Offset area inside a larger buffer with a blue background: the
        // panel shows its first 3 lines only, and keeps the background.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 5));
        buffer.set_style(buffer.area, RStyle::default().bg(RColor::Blue));
        RichWidget::new(&panel).render(Rect::new(1, 1, 10, 3), &mut buffer);
        let rows = buffer_rows(&buffer);
        assert_eq!(rows[0], " ".repeat(12));
        assert_eq!(rows[1], " ╭────────╮ ");
        assert_eq!(rows[3], " │ two    │ ");
        assert_eq!(rows[4], " ".repeat(12));
        assert_eq!(buffer[(5, 2)].bg, RColor::Blue);
    }

    #[test]
    fn wide_characters_round_trip() {
        let text = rich::Text::new("日本語 ok 🎉");
        let area = Rect::new(0, 0, 20, 1);
        let mut buffer = Buffer::empty(area);
        RichWidget::new(&text).render(area, &mut buffer);
        // The wide chars' trailing cells are blank in the buffer...
        assert_eq!(buffer[(0, 0)].symbol(), "日");
        assert_eq!(buffer[(1, 0)].symbol(), " ");
        // ...and skipped coming back: same text, exactly 20 cells.
        let lines = buffer_to_lines(&buffer);
        let row = &plain(&lines)[0];
        assert_eq!(row.trim_end(), "日本語 ok 🎉");
        assert_eq!(rich::cells::cell_len(row), 20);
    }

    #[test]
    fn panel_round_trips_with_styles() {
        let body = rich::Text::from_markup(
            "[bold red]alert[/] [italic on blue]note[/] [color(200)]日本[/] [#ff8001 u]rgb[/]",
        )
        .unwrap();
        let panel = Panel::new(Box::new(body))
            .title("[b]demo[/]")
            .border_style("green");
        let console = console(30);
        let options = console.options().update_width(30);
        let original = console.render_lines(&panel, &options, false);

        let area = Rect::new(0, 0, 30, original.len() as u16);
        let mut buffer = Buffer::empty(area);
        RichWidget::new(&panel)
            .console(&console)
            .render(area, &mut buffer);
        let back = buffer_to_lines(&buffer);
        let expected = normalise(&original);
        // The comparison has teeth: styled runs, borders included.
        let styled: Vec<&str> = expected
            .iter()
            .flatten()
            .filter(|(_, sgr)| !sgr.is_empty())
            .map(|(_, sgr)| sgr.as_str())
            .collect();
        for sgr in ["1;31", "3;44", "38;5;200", "4;38;2;255;128;1", "32"] {
            assert!(styled.contains(&sgr), "{sgr} missing from {styled:?}");
        }
        assert_eq!(normalise(&back), expected);
        // An 8-bit colour comes back as 8-bit.
        let indexed = back
            .iter()
            .flatten()
            .filter_map(|s| s.style.as_ref()?.color())
            .any(|c| c.kind == ColorType::EightBit && c.number == Some(200));
        assert!(indexed);
    }

    #[test]
    fn markup_fill_height_and_empty_areas() {
        // Markup, owned.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        RichWidget::markup("[bold]hi[/] [").render(buffer.area, &mut buffer);
        assert_eq!(buffer_rows(&buffer), ["hi [      "]);
        // Unparseable markup is plain text.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        RichWidget::markup("[/oops]").render(buffer.area, &mut buffer);
        assert_eq!(buffer_rows(&buffer)[0].trim_end(), "[/oops]");

        // A panel with fill_height takes the whole area.
        let panel = Panel::new(Box::new(rich::Text::new("x")));
        let area = Rect::new(0, 0, 8, 5);
        let natural = RichWidget::new(&panel).lines(area);
        let filled = RichWidget::new(&panel).fill_height(true).lines(area);
        assert_eq!(natural.len(), 3);
        assert_eq!(filled.len(), 5);

        // Empty areas and areas outside the buffer draw nothing, and do
        // not panic.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        RichWidget::new(&panel).render(Rect::new(0, 0, 0, 2), &mut buffer);
        RichWidget::new(&panel).render(Rect::new(10, 10, 4, 2), &mut buffer);
        assert_eq!(buffer, Buffer::empty(Rect::new(0, 0, 4, 2)));
    }
}
