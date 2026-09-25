//! Small core renderables the code area composes with: already-rendered
//! lines (upstream's `Segments`), a vertical stack (upstream's `Group`) and a
//! blank line (upstream's `""` in a render result).

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::style::StyleType;
use rich::Text as CoreText;

/// Lines to core's convention: a newline between lines, none after the last.
pub(crate) fn join_lines(lines: Vec<Vec<CoreSegment>>) -> Vec<CoreSegment> {
    let mut segments = Vec::new();
    let last = lines.len().saturating_sub(1);
    for (index, line) in lines.into_iter().enumerate() {
        if line.is_empty() {
            // A blank line still counts as a line.
            segments.push(CoreSegment::new("", None));
        }
        segments.extend(line);
        if index != last {
            segments.push(CoreSegment::line());
        }
    }
    segments
}

/// Core's render output back to lines: the inverse of [`join_lines`], so a
/// render ending in a newline ends with an empty line (core's own
/// `split_lines` drops it).
pub(crate) fn split_keep(segments: &[CoreSegment]) -> Vec<Vec<CoreSegment>> {
    if segments.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut current = Vec::new();
    for segment in segments {
        if segment.control || !segment.text.contains('\n') {
            if !segment.text.is_empty() {
                current.push(segment.clone());
            }
            continue;
        }
        let mut parts = segment.text.split('\n').peekable();
        while let Some(part) = parts.next() {
            if !part.is_empty() {
                current.push(CoreSegment::new(part, segment.style.clone()));
            }
            if parts.peek().is_some() {
                lines.push(std::mem::take(&mut current));
            }
        }
    }
    lines.push(current);
    lines
}

/// An empty line (upstream yields `""`).
pub(crate) struct Blank;

impl Renderable for Blank {
    fn rich_render(&self, _console: &CoreConsole, _options: &CoreOptions) -> Vec<CoreSegment> {
        vec![CoreSegment::new("", None)]
    }

    fn measure(&self, _console: &CoreConsole, _options: &CoreOptions) -> CoreMeasurement {
        CoreMeasurement::new(0, 0)
    }
}

/// Renderables one below the other (upstream's `Group`).
pub(crate) struct Stack {
    pub(crate) children: Vec<Box<dyn Renderable + Send + Sync>>,
}

impl Renderable for Stack {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let mut lines = Vec::new();
        for child in &self.children {
            let mut child_options = options.clone();
            child_options.height = None;
            let segments = child.rich_render(console, &child_options);
            lines.extend(split_keep(&segments));
        }
        join_lines(lines)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        let mut minimum = 0;
        let mut maximum = 0;
        for child in &self.children {
            let measured = CoreMeasurement::get(console, options, child.as_ref());
            minimum = minimum.max(measured.minimum);
            maximum = maximum.max(measured.maximum);
        }
        CoreMeasurement::new(minimum, maximum)
    }
}

/// A shared renderable, for core containers that take their child by value.
pub(crate) struct Shared(pub(crate) std::sync::Arc<dyn Renderable + Send + Sync>);

impl Renderable for Shared {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        self.0.rich_render(console, options)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        self.0.measure(console, options)
    }
}

/// A `Panel` whose border style is a name (`"scope.border"`), resolved
/// against the rendering console's theme, as upstream resolves it.
pub(crate) struct NamedPanel {
    pub(crate) child: std::sync::Arc<dyn Renderable + Send + Sync>,
    pub(crate) border_style: StyleType,
    pub(crate) title: Option<String>,
    pub(crate) expand: bool,
    pub(crate) width: Option<usize>,
    pub(crate) padding: (usize, usize, usize, usize),
}

impl NamedPanel {
    fn panel(&self, console: &CoreConsole) -> rich::Panel {
        let mut panel = rich::Panel::new(Box::new(Shared(self.child.clone())))
            .expand(self.expand)
            .padding(self.padding);
        if let Ok(style) = console.get_style(&self.border_style) {
            panel = panel.border_style(style);
        }
        if let Some(title) = &self.title {
            panel = panel.title(title.as_str());
        }
        if let Some(width) = self.width {
            panel = panel.width(width);
        }
        panel
    }
}

impl Renderable for NamedPanel {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        self.panel(console).rich_render(console, options)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        self.panel(console).measure(console, options)
    }
}

/// Console markup that renders as `text` does (for a core `Panel` title,
/// which takes markup): each run between span edges opens the styles that
/// cover it, in span order.
pub(crate) fn to_markup(text: &CoreText) -> String {
    let plain = text.plain();
    let mut edges: Vec<usize> = vec![0, plain.len()];
    for span in text.spans() {
        edges.push(span.start.min(plain.len()));
        edges.push(span.end.min(plain.len()));
    }
    edges.sort_unstable();
    edges.dedup();
    let name = |style: &StyleType| match style {
        StyleType::Name(name) => name.clone(),
        StyleType::Style(style) => style.definition(),
    };
    let base = text.base_style();
    let base = match base {
        StyleType::Name(name) if name.is_empty() => None,
        StyleType::Style(style) if style.is_null() => None,
        other => Some(name(other)),
    };
    let mut markup = String::new();
    for pair in edges.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start >= end {
            continue;
        }
        let styles: Vec<String> = base
            .iter()
            .cloned()
            .chain(
                text.spans()
                    .iter()
                    .filter(|span| span.start <= start && span.end >= end)
                    .map(|span| name(&span.style)),
            )
            .filter(|style| !style.is_empty())
            .collect();
        for style in &styles {
            markup.push_str(&format!("[{style}]"));
        }
        markup.push_str(&rich::markup::escape(&plain[start..end]));
        for _ in &styles {
            markup.push_str("[/]");
        }
    }
    markup
}

/// Rename the named styles of `text` found in `map` (upstream pushes a theme
/// over part of a render; core cannot push one mid-render, so the names are
/// replaced by the pushed theme's definitions instead).
pub(crate) fn restyle(text: &CoreText, map: &[(&str, &str)]) -> CoreText {
    let rename = |style: &StyleType| match style {
        StyleType::Name(name) => map
            .iter()
            .find(|(from, _)| from == name)
            .map(|(_, to)| StyleType::Name((*to).to_string()))
            .unwrap_or_else(|| style.clone()),
        other => other.clone(),
    };
    let mut restyled = text.blank_copy();
    restyled.set_base_style(rename(text.base_style()));
    restyled.append(text.plain(), None);
    for span in text.spans() {
        restyled.stylize(rename(&span.style), span.start, span.end);
    }
    restyled
}
