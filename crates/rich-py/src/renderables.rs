//! Static renderables: `Rule`, `Padding`, `Align`, `VerticalCenter`,
//! `Columns`, `Group`, `Constrain`, `Styled`, `Tree`, `Layout`, `Bar`,
//! `Spinner`, and `rich.containers`' `Lines` and `Renderables` (Python
//! modules `rs_rich.rule`, `rs_rich.padding`, `rs_rich.align`,
//! `rs_rich.columns`, `rs_rich.console` (`Group`, `group`),
//! `rs_rich.constrain`, `rs_rich.styled`, `rs_rich.tree`, `rs_rich.layout`,
//! `rs_rich.bar`, `rs_rich.spinner` and `rs_rich.containers`).
//!
//! Owner: the static-renderables area. One submodule per upstream module.
//!
//! Several of core's ports cover only a slice of their upstream module (core's
//! `Align` has no vertical alignment, its `Tree` no styles, its `Columns` no
//! `width`, `column_first` or `title`, ...). The bindings promise Rich's whole
//! API, so the renderers here port the rest of each upstream module in Rust,
//! on core's public primitives (`Segment`, `Text`, `Table::grid`,
//! `Console::render_lines_styled`, `ratio_resolve`), and core's own types are
//! used where they already match upstream (`Constrain`, `Styled`).

use std::sync::Arc;

use pyo3::prelude::*;

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::style::{Style as CoreStyle, StyleType};
use rich::Text as CoreText;

use crate::renderable;

mod align;
mod bar;
mod columns;
mod constrain;
mod containers;
mod layout;
mod padding;
mod rule;

pub(crate) use rule::ends_inline as rule_ends_inline;
mod spinner;
mod tree;

/// A renderer's child: a boxed core renderable, or a shared one (a `Table`
/// cell must be `Send + Sync`).
pub(crate) trait Child {
    fn get(&self) -> &dyn Renderable;
}

impl Child for Box<dyn Renderable> {
    fn get(&self) -> &dyn Renderable {
        self.as_ref()
    }
}

impl Child for Arc<dyn Renderable + Send + Sync> {
    fn get(&self) -> &dyn Renderable {
        self.as_ref()
    }
}

/// `Console.render(renderable, options)` in core's convention: nothing at
/// all in no width.
pub(crate) fn render(
    console: &CoreConsole,
    child: &dyn Renderable,
    options: &CoreOptions,
) -> Vec<CoreSegment> {
    if options.max_width < 1 {
        return Vec::new();
    }
    child.rich_render(console, options)
}

/// `Console.render_lines(renderable, options, style=style, pad=pad)`.
///
/// Core's `render_lines_styled` pads a missing row with an empty line when
/// `pad` is off and leaves an over-long line alone; upstream always crops to
/// the width and always fills a missing row with spaces.
pub(crate) fn render_lines(
    console: &CoreConsole,
    child: &dyn Renderable,
    options: &CoreOptions,
    style: Option<&CoreStyle>,
    pad: bool,
) -> Vec<Vec<CoreSegment>> {
    let style = style.filter(|style| !style.is_null());
    let width = options.max_width;
    let mut segments = render(console, child, options);
    if let Some(style) = style {
        segments = CoreSegment::apply_style(&segments, style);
    }
    let mut lines = split_lines(segments);
    for line in &mut lines {
        let length: usize = line.iter().map(CoreSegment::cell_length).sum();
        if pad || length > width {
            *line = CoreSegment::adjust_line_length(line, width, style.cloned());
        }
    }
    if let Some(height) = options.height {
        lines.truncate(height);
        while lines.len() < height {
            lines.push(vec![CoreSegment::new(" ".repeat(width), style.cloned())]);
        }
    }
    lines
}

/// `Segment.split_lines` of a render in core's convention (the newline
/// after the last line implied), keeping a trailing blank line.
pub(crate) fn split_lines(segments: Vec<CoreSegment>) -> Vec<Vec<CoreSegment>> {
    CoreSegment::split_lines(&renderable::terminated(segments))
}

/// Lines back to a segment stream, in core's convention (no newline after
/// the last line).
pub(crate) fn join_lines(lines: Vec<Vec<CoreSegment>>) -> Vec<CoreSegment> {
    let mut segments = Vec::new();
    let last = lines.len().saturating_sub(1);
    for (index, line) in lines.into_iter().enumerate() {
        segments.extend(line);
        if index != last {
            segments.push(CoreSegment::line());
        }
    }
    segments
}

/// `Segment.get_shape(lines)`: the widest line and the number of lines.
pub(crate) fn shape(lines: &[Vec<CoreSegment>]) -> (usize, usize) {
    let width = lines
        .iter()
        .map(|line| line.iter().map(CoreSegment::cell_length).sum::<usize>())
        .max()
        .unwrap_or(0);
    (width, lines.len())
}

/// `console.get_style(style)`. A name the theme does not know and that does
/// not parse renders unstyled (core's renderers cannot raise mid-render).
pub(crate) fn get_style(console: &CoreConsole, style: &StyleType) -> CoreStyle {
    console.get_style(style).unwrap_or_default()
}

/// `options.ascii_only`: the output encoding is not a UTF one.
pub(crate) fn ascii_only(console: &CoreConsole) -> bool {
    match renderable::ambient() {
        Ok(ambient) => !ambient.base.encoding.starts_with("utf"),
        Err(_) => console.ascii_only(),
    }
}

/// `options.size.height`: the console's height.
pub(crate) fn screen_height(console: &CoreConsole) -> usize {
    match renderable::ambient() {
        Ok(ambient) => ambient.base.size.1,
        Err(_) => console.height(),
    }
}

/// The measurement of a renderable without `__rich_measure__`: anything
/// from nothing to the whole width.
pub(crate) fn unmeasured(options: &CoreOptions) -> CoreMeasurement {
    CoreMeasurement::new(0, options.max_width)
}

/// Console markup that reproduces `text`'s plain text and styles, for core
/// APIs that take a title as markup (`Table::title`, `Panel::title`).
pub(crate) fn text_markup(text: &CoreText) -> String {
    let tag = |style: &StyleType| match style {
        StyleType::Name(name) => name.clone(),
        StyleType::Style(style) => style.definition(),
    };
    let plain = text.plain();
    let mut spans: Vec<(usize, usize, String)> = Vec::new();
    let base = tag(text.base_style());
    if !base.is_empty() && base != "none" {
        spans.push((0, plain.len(), base));
    }
    for span in text.spans() {
        let name = tag(&span.style);
        if span.start < span.end && !name.is_empty() && name != "none" {
            spans.push((span.start, span.end.min(plain.len()), name));
        }
    }
    let mut cuts: Vec<usize> = spans
        .iter()
        .flat_map(|(start, end, _)| [*start, *end])
        .collect();
    cuts.push(0);
    cuts.push(plain.len());
    cuts.sort_unstable();
    cuts.dedup();
    let mut markup = String::new();
    let mut open: Vec<usize> = Vec::new();
    for window in cuts.windows(2) {
        let (at, next) = (window[0], window[1]);
        for (index, (_, end, name)) in spans.iter().enumerate().rev() {
            if *end == at && open.contains(&index) {
                markup.push_str(&format!("[/{name}]"));
                open.retain(|open| *open != index);
            }
        }
        for (index, (start, _, name)) in spans.iter().enumerate() {
            if *start == at {
                markup.push_str(&format!("[{name}]"));
                open.push(index);
            }
        }
        markup.push_str(&rich::markup::escape(&plain[at..next]));
    }
    markup
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    rule::register(m)?;
    padding::register(m)?;
    align::register(m)?;
    constrain::register(m)?;
    containers::register(m)?;
    columns::register(m)?;
    tree::register(m)?;
    layout::register(m)?;
    bar::register(m)?;
    spinner::register(m)?;
    Ok(())
}
