//! A full-screen renderable.
//!
//! Port of `rich/screen.py`. [`Screen`] fills the entire console region
//! (width × height), rendering its child into it and cropping/padding to an
//! exact rectangle, optionally under a background [`Style`]. Used with the
//! alternate screen buffer (see [`Control::alt_screen`](crate::control::Control)).

use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::{Style, StyleType};

/// A renderable that fills the screen and crops excess. Mirrors
/// `rich.screen.Screen`.
pub struct Screen {
    /// Upstream's `Group(*renderables)`.
    renderables: Vec<Box<dyn Renderable>>,
    style: Option<StyleType>,
    application_mode: bool,
}

impl Screen {
    /// A screen filled with `renderable`.
    pub fn new(renderable: Box<dyn Renderable>) -> Self {
        Screen::from_renderables(vec![renderable])
    }

    /// A screen of several renderables, one after another: upstream's
    /// `Screen(*renderables)`, which groups them.
    pub fn from_renderables(renderables: Vec<Box<dyn Renderable>>) -> Self {
        Screen {
            renderables,
            style: None,
            application_mode: false,
        }
    }

    /// An empty screen (blank fill).
    pub fn empty() -> Self {
        Screen::from_renderables(Vec::new())
    }

    /// Set a background style applied under the whole screen (upstream
    /// `style=`): a [`Style`](crate::style::Style), or a theme name or style
    /// definition resolved at render time.
    pub fn style(mut self, style: impl Into<StyleType>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// Separate rows with `"\n\r"` rather than `"\n"` (upstream
    /// `application_mode=`), for a terminal in application mode.
    pub fn application_mode(mut self, application_mode: bool) -> Self {
        self.application_mode = application_mode;
        self
    }
}

/// `Group(*renderables)` for [`Screen`]: each renderable rendered in turn,
/// the outputs joined by a line break.
struct ScreenGroup<'a>(&'a [Box<dyn Renderable>]);

impl Renderable for ScreenGroup<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut segments = Vec::new();
        let mut first = true;
        for renderable in self.0 {
            let rendered = console.render(renderable.as_ref(), Some(options));
            if rendered.is_empty() {
                continue;
            }
            if !first {
                segments.push(Segment::line());
            }
            first = false;
            segments.extend(rendered);
        }
        segments
    }
}

impl Renderable for Screen {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let style = self
            .style
            .as_ref()
            .map(|style| console.get_style(style).unwrap_or_default())
            .filter(|style| !style.is_null());
        render_screen_with(
            &ScreenGroup(&self.renderables),
            style,
            self.application_mode,
            console,
            options,
        )
    }

    /// Upstream ends a `Screen` with its last row, without a line break, so a
    /// printed `Screen` leaves the cursor on that row.
    fn print_end_line(&self, _options: &ConsoleOptions) -> bool {
        false
    }
}

/// `Screen.__rich_console__` over a borrowed renderable, so a `Live` display
/// in alternate-screen mode can fill the screen with the renderable it owns.
pub(crate) fn render_screen(
    renderable: Option<&dyn Renderable>,
    style: Option<&Style>,
    console: &Console,
    options: &ConsoleOptions,
) -> Vec<Segment> {
    // `self.renderable or ""`.
    let content: &dyn Renderable = match renderable {
        Some(renderable) => renderable,
        None => &ScreenGroup(&[]),
    };
    render_screen_with(content, style.cloned(), false, console, options)
}

/// The body of `Screen.__rich_console__`, with the style already resolved.
fn render_screen_with(
    content: &dyn Renderable,
    style: Option<Style>,
    application_mode: bool,
    console: &Console,
    options: &ConsoleOptions,
) -> Vec<Segment> {
    // `width, height = options.size`.
    let width = options.size.width;
    let height = options.size.height;
    // `console.render_lines(self.renderable or "", render_options,
    // style=style, pad=True)`.
    let render_options = options.update_dimensions(width, height);
    let lines = console.render_lines_styled(content, &render_options, style.as_ref(), true);
    // `Segment.set_shape(lines, width, height, style=style)`.
    let pad_style = style.clone();
    let mut lines: Vec<Vec<Segment>> = lines
        .into_iter()
        .take(height)
        .map(|line| Segment::adjust_line_length(&line, width, pad_style.clone()))
        .collect();
    while lines.len() < height {
        lines.push(vec![Segment::new(" ".repeat(width), pad_style.clone())]);
    }

    let new_line = if application_mode {
        Segment::new("\n\r", None)
    } else {
        Segment::line()
    };
    let mut segments = Vec::new();
    let last = lines.len().saturating_sub(1);
    for (index, line) in lines.into_iter().enumerate() {
        segments.extend(line);
        if index != last {
            segments.push(new_line.clone());
        }
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use crate::text::Text;

    fn console(width: usize, height: usize) -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(width)
            .height(height)
            .build()
    }

    #[test]
    fn fills_to_width_and_height() {
        let screen = Screen::new(Box::new(Text::new("hi")));
        let out = console(6, 3).capture(|c| c.print(&screen));
        // 3 rows of 6 cells: "hi" then two blank rows, with no line break
        // after the last, as upstream prints a `Screen`.
        assert_eq!(out, "hi    \n      \n      ");
    }

    #[test]
    fn empty_screen_is_blank_rectangle() {
        let out = console(4, 2).capture(|c| c.print(&Screen::empty()));
        assert_eq!(out, "    \n    ");
    }
}
