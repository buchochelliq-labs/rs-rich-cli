//! Horizontal alignment.
//!
//! Port of upstream `rich/align.py`. [`Align`] pads a child renderable to fill
//! the available width, positioning it left, center, or right, and optionally
//! within a height (top, middle, bottom); [`VerticalCenter`] is upstream's
//! deprecated vertical-only form.

use crate::console::{Console, ConsoleOptions};
use crate::measure::Measurement;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::StyleType;

/// Where to position content within an available width. Shared by [`Align`],
/// `Rule` titles, and `Panel` titles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HorizontalAlign {
    Left,
    #[default]
    Center,
    Right,
}

/// Where to position content within an available height. Mirrors
/// `rich.align.VerticalAlignMethod`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlign {
    Top,
    Middle,
    Bottom,
}

/// Aligns a child renderable within the available width (and, with a
/// vertical alignment, height). Mirrors `rich.align.Align`.
pub struct Align {
    child: Box<dyn Renderable>,
    align: HorizontalAlign,
    style: Option<StyleType>,
    vertical: Option<VerticalAlign>,
    pad: bool,
    width: Option<usize>,
    height: Option<usize>,
}

impl Align {
    /// Align `child` horizontally. Port of `Align(renderable, align)`; the
    /// other keywords are builder methods.
    pub fn new(child: Box<dyn Renderable>, align: HorizontalAlign) -> Self {
        Align {
            child,
            align,
            style: None,
            vertical: None,
            pad: true,
            width: None,
            height: None,
        }
    }

    /// Left-align the child (pads on the right).
    pub fn left(child: Box<dyn Renderable>) -> Self {
        Align::new(child, HorizontalAlign::Left)
    }

    /// Center the child (pads both sides, extra cell on the right).
    pub fn center(child: Box<dyn Renderable>) -> Self {
        Align::new(child, HorizontalAlign::Center)
    }

    /// Right-align the child (pads on the left).
    pub fn right(child: Box<dyn Renderable>) -> Self {
        Align::new(child, HorizontalAlign::Right)
    }

    /// The background style of the padding (upstream `style=`), applied under
    /// the whole output: a [`Style`](crate::style::Style), or a theme name or
    /// style definition resolved at render time.
    pub fn style(mut self, style: impl Into<StyleType>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// Align vertically within `height` (or the options' height). Upstream
    /// `vertical=`.
    pub fn vertical(mut self, vertical: VerticalAlign) -> Self {
        self.vertical = Some(vertical);
        self
    }

    /// Pad the right-hand side and blank lines (upstream `pad=`, default on).
    pub fn pad(mut self, pad: bool) -> Self {
        self.pad = pad;
        self
    }

    /// Constrain the child's width (upstream `width=`).
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// The height to align within (upstream `height=`), else the options'.
    pub fn height(mut self, height: usize) -> Self {
        self.height = Some(height);
        self
    }
}

impl Align {
    /// `Align.__rich_console__` over a borrowed child: what
    /// [`Console::print_with`] wraps a non-`Text` renderable in for
    /// `justify="left"|"center"|"right"` (upstream's `_collect_renderables`).
    pub fn render_child(
        child: &dyn Renderable,
        align: HorizontalAlign,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Vec<Segment> {
        AlignSpec {
            align,
            style: None,
            vertical: None,
            pad: true,
            width: None,
            height: None,
        }
        .render(child, console, options)
    }

    fn spec(&self) -> AlignSpec {
        AlignSpec {
            align: self.align,
            style: self.style.clone(),
            vertical: self.vertical,
            pad: self.pad,
            width: self.width,
            height: self.height,
        }
    }
}

/// Every `Align` option but the child.
struct AlignSpec {
    align: HorizontalAlign,
    style: Option<StyleType>,
    vertical: Option<VerticalAlign>,
    pad: bool,
    width: Option<usize>,
    height: Option<usize>,
}

impl AlignSpec {
    fn render(
        &self,
        child: &dyn Renderable,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Vec<Segment> {
        // Upstream measures the child, renders it through `Constrain` at that
        // width, and squares the lines off with `Segment.set_shape`, so the
        // rendered *block* is aligned as a whole (#443).
        let measured = Measurement::get(console, options, child).maximum;
        let block_width = match self.width {
            Some(width) => measured.min(width),
            None => measured,
        };
        let mut child_options = options.update_width(block_width.min(options.max_width));
        child_options.height = None;
        let lines = console.render_lines(child, &child_options, false);
        let width = lines
            .iter()
            .map(|line| line.iter().map(Segment::cell_length).sum::<usize>())
            .max()
            .unwrap_or(0);
        let height = lines.len();
        let lines: Vec<Vec<Segment>> = lines
            .iter()
            .map(|line| Segment::adjust_line_length(line, width, None))
            .collect();

        let excess = options.max_width.saturating_sub(width);
        // `console.get_style(self.style) if self.style is not None else None`.
        let style = self
            .style
            .as_ref()
            .map(|style| console.get_style(style).unwrap_or_default());
        let pad_style = Some(style.clone().unwrap_or_default());
        let (left_pad, right_pad) = match self.align {
            HorizontalAlign::Left => (0, if self.pad { excess } else { 0 }),
            HorizontalAlign::Right => (excess, 0),
            HorizontalAlign::Center => (excess / 2, if self.pad { excess - excess / 2 } else { 0 }),
        };

        let mut rows: Vec<Vec<Segment>> = Vec::with_capacity(lines.len());
        for line in lines {
            let mut row = Vec::new();
            if left_pad > 0 {
                row.push(Segment::new(" ".repeat(left_pad), pad_style.clone()));
            }
            row.extend(line);
            if right_pad > 0 {
                row.push(Segment::new(" ".repeat(right_pad), pad_style.clone()));
            }
            rows.push(row);
        }

        // `blank_line`: a full-width row of the padding style, or bare.
        let blank = || {
            if self.pad {
                // `self.width or options.max_width`: a zero width is unset.
                vec![Segment::new(
                    " ".repeat(
                        self.width
                            .filter(|&width| width > 0)
                            .unwrap_or(options.max_width),
                    ),
                    pad_style.clone(),
                )]
            } else {
                Vec::new()
            }
        };
        // `self.height or options.height`: a zero height is unset.
        let total = self.height.filter(|&height| height > 0).or(options.height);
        if let (Some(vertical), Some(total)) = (self.vertical, total) {
            let (top, bottom) = match vertical {
                VerticalAlign::Top => (0, total.saturating_sub(height)),
                VerticalAlign::Middle => {
                    let top = total.saturating_sub(height) / 2;
                    (top, total.saturating_sub(top + height))
                }
                VerticalAlign::Bottom => (total.saturating_sub(height), 0),
            };
            let mut shaped = Vec::with_capacity(top + rows.len() + bottom);
            shaped.extend(std::iter::repeat_with(blank).take(top));
            shaped.append(&mut rows);
            shaped.extend(std::iter::repeat_with(blank).take(bottom));
            rows = shaped;
        }

        let mut segments = Vec::new();
        let last = rows.len().saturating_sub(1);
        for (index, row) in rows.into_iter().enumerate() {
            segments.extend(row);
            if index != last {
                segments.push(Segment::line());
            }
        }
        match style.as_ref().filter(|style| !style.is_null()) {
            Some(style) => Segment::apply_style(&segments, style),
            None => segments,
        }
    }
}

impl Renderable for Align {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.spec().render(self.child.as_ref(), console, options)
    }

    /// Port of `Align.__rich_measure__`: the child's measurement.
    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        Measurement::get(console, options, self.child.as_ref())
    }

    /// Upstream's `Align.vertical`, which a `Table` cell aligns by.
    fn vertical(&self) -> Option<VerticalAlign> {
        self.vertical
    }
}

/// Vertically centres a renderable in the options' height (else the console
/// height). Mirrors the deprecated `rich.align.VerticalCenter`.
pub struct VerticalCenter {
    child: Box<dyn Renderable>,
    style: Option<StyleType>,
}

impl VerticalCenter {
    pub fn new(child: Box<dyn Renderable>) -> Self {
        VerticalCenter { child, style: None }
    }

    /// The style of the blank lines above and below (upstream `style=`): a
    /// [`Style`](crate::style::Style), or a name resolved at render time.
    pub fn style(mut self, style: impl Into<StyleType>) -> Self {
        self.style = Some(style.into());
        self
    }
}

impl Renderable for VerticalCenter {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut child_options = options.clone();
        child_options.height = None;
        let lines = console.render_lines(self.child.as_ref(), &child_options, false);
        let width = lines
            .iter()
            .map(|line| line.iter().map(Segment::cell_length).sum::<usize>())
            .max()
            .unwrap_or(0);
        // `options.height or options.size.height`: a zero height is unset.
        let height = options
            .height
            .filter(|&height| height > 0)
            .unwrap_or(options.size.height);
        let top = height.saturating_sub(lines.len()) / 2;
        let bottom = height.saturating_sub(top + lines.len());
        let style = self
            .style
            .as_ref()
            .map(|style| console.get_style(style).unwrap_or_default());
        let blank = || vec![Segment::new(" ".repeat(width), style.clone())];
        let mut rows: Vec<Vec<Segment>> = Vec::new();
        rows.extend(std::iter::repeat_with(blank).take(top));
        rows.extend(lines);
        rows.extend(std::iter::repeat_with(blank).take(bottom));
        let mut segments = Vec::new();
        let last = rows.len().saturating_sub(1);
        for (index, row) in rows.into_iter().enumerate() {
            segments.extend(row);
            if index != last {
                segments.push(Segment::line());
            }
        }
        segments
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        Measurement::get(console, options, self.child.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use crate::text::Text;

    fn console(width: usize) -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(width)
            .build()
    }

    #[test]
    fn center_pads_both_sides() {
        let out = console(20).render_export(&Align::center(Box::new(Text::new("hi"))));
        assert_eq!(out, "         hi         \n");
    }

    #[test]
    fn right_pads_left() {
        let out = console(20).render_export(&Align::right(Box::new(Text::new("hi"))));
        assert_eq!(out, "                  hi\n");
    }

    #[test]
    fn center_odd_remainder_floors_left() {
        let out = console(21).render_export(&Align::center(Box::new(Text::new("hi"))));
        assert_eq!(out, "         hi          \n");
    }

    #[test]
    fn aligns_the_wrapped_block_not_each_line() {
        // Captured from real rich 15.0.0 (#443): the block is 4 cells wide, so
        // the shorter wrapped line keeps its place inside it.
        let out = console(4).render_export(&Align::right(Box::new(Text::new("abcd ef"))));
        assert_eq!(out, "abcd\nef  \n");
        let out = console(8).render_export(&Align::right(Box::new(Text::new("abc de"))));
        assert_eq!(out, "  abc de\n");
    }
}
