//! Renderable containers.
//!
//! Port of upstream `rich/containers.py`: [`Renderables`] and [`Lines`].
//! Upstream's `Text.wrap` returns `Lines`; this port wraps at the segment
//! level, so `Lines` is here for callers that justify lines of text
//! themselves.

use std::sync::Arc;

use crate::console::{Console, ConsoleOptions, Justify, Overflow};
use crate::measure::Measurement;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::StyleType;
use crate::text::Text;

/// A list of renderables rendered one after another. Mirrors
/// `rich.containers.Renderables`, which `LogRender` puts in its message column.
#[derive(Clone, Default)]
pub struct Renderables {
    renderables: Vec<Arc<dyn Renderable + Send + Sync>>,
}

impl Renderables {
    /// A container of `renderables`, rendered in order.
    pub fn new(renderables: Vec<Arc<dyn Renderable + Send + Sync>>) -> Self {
        Renderables { renderables }
    }

    /// Add a renderable to the end. Port of `Renderables.append`.
    pub fn append(&mut self, renderable: Arc<dyn Renderable + Send + Sync>) {
        self.renderables.push(renderable);
    }

    /// The contained renderables.
    pub fn renderables(&self) -> &[Arc<dyn Renderable + Send + Sync>] {
        &self.renderables
    }
}

impl Renderable for Renderables {
    /// `yield from self._renderables`, each through `Console.render`.
    ///
    /// Upstream's renderables each end their output with a newline; this
    /// port's separate lines instead, so each child's output is joined to the
    /// previous one with a newline. A child that renders nothing adds no line.
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let options = options.reset_height();
        let mut segments: Vec<Segment> = Vec::new();
        let mut first = true;
        for renderable in &self.renderables {
            let rendered = console.render(renderable.as_ref(), Some(&options));
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

    /// Port of `Renderables.__rich_measure__`: the widest minimum and maximum
    /// of the children, or `(1, 1)` when there are none.
    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let dimensions: Vec<Measurement> = self
            .renderables
            .iter()
            .map(|renderable| Measurement::get(console, options, renderable.as_ref()))
            .collect();
        if dimensions.is_empty() {
            return Measurement::new(1, 1);
        }
        Measurement::new(
            dimensions.iter().map(|d| d.minimum).max().unwrap_or(0),
            dimensions.iter().map(|d| d.maximum).max().unwrap_or(0),
        )
    }
}

/// A list of [`Text`] lines that renders one per line. Port of
/// `rich.containers.Lines`.
#[derive(Clone, Debug, Default)]
pub struct Lines {
    lines: Vec<Text>,
}

impl Lines {
    /// Lines holding `lines`.
    pub fn new(lines: Vec<Text>) -> Self {
        Lines { lines }
    }

    /// The lines.
    pub fn lines(&self) -> &[Text] {
        &self.lines
    }

    /// The lines, mutably (upstream's `__setitem__`).
    pub fn lines_mut(&mut self) -> &mut Vec<Text> {
        &mut self.lines
    }

    /// How many lines there are.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether there are no lines.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Add a line. Port of `Lines.append`.
    pub fn append(&mut self, line: Text) {
        self.lines.push(line);
    }

    /// Add several lines. Port of `Lines.extend`.
    pub fn extend(&mut self, lines: impl IntoIterator<Item = Text>) {
        self.lines.extend(lines);
    }

    /// Remove and return the line at `index`, the last when `None`. Port of
    /// `Lines.pop`.
    pub fn pop(&mut self, index: Option<usize>) -> Option<Text> {
        match index {
            None => self.lines.pop(),
            Some(index) if index < self.lines.len() => Some(self.lines.remove(index)),
            Some(_) => None,
        }
    }

    /// Justify and overflow every line to `width` cells. Port of
    /// `Lines.justify`: `left` pads each line, `center` and `right` strip and
    /// pad on the left (and right), and `full` spreads the spaces between the
    /// words of every line but the last. Any other method leaves the lines
    /// alone.
    pub fn justify(
        &mut self,
        console: &Console,
        width: usize,
        justify: Justify,
        overflow: Overflow,
    ) {
        let cells = |line: &Text| crate::cells::cell_len(line.plain());
        match justify {
            Justify::Left => {
                for line in &mut self.lines {
                    line.truncate(width, Some(overflow), true);
                }
            }
            Justify::Center => {
                for line in &mut self.lines {
                    line.rstrip();
                    line.truncate(width, Some(overflow), false);
                    line.pad_left(width.saturating_sub(cells(line)) / 2, ' ');
                    line.pad_right(width.saturating_sub(cells(line)), ' ');
                }
            }
            Justify::Right => {
                for line in &mut self.lines {
                    line.rstrip();
                    line.truncate(width, Some(overflow), false);
                    line.pad_left(width.saturating_sub(cells(line)), ' ');
                }
            }
            Justify::Full => {
                let last = self.lines.len().saturating_sub(1);
                for line_index in 0..last {
                    let line = &self.lines[line_index];
                    let words = line.split(" ", false, false);
                    let words_size: usize = words.iter().map(cells).sum();
                    let mut num_spaces = words.len().saturating_sub(1);
                    let mut spaces = vec![1usize; num_spaces];
                    let mut index = 0;
                    if !spaces.is_empty() {
                        while words_size + num_spaces < width {
                            let slot = spaces.len() - index - 1;
                            spaces[slot] += 1;
                            num_spaces += 1;
                            index = (index + 1) % spaces.len();
                        }
                    }
                    let mut tokens: Vec<Text> = Vec::with_capacity(words.len() * 2);
                    for (index, word) in words.iter().enumerate() {
                        tokens.push(word.clone());
                        if index < spaces.len() {
                            let style = word.get_style_at_offset(console, -1);
                            let next_style = words[index + 1].get_style_at_offset(console, 0);
                            let space_style: StyleType = if style == next_style {
                                style.into()
                            } else {
                                line.base_style().clone()
                            };
                            tokens.push(Text::styled(" ".repeat(spaces[index]), space_style));
                        }
                    }
                    self.lines[line_index] = Text::new("").join(&tokens);
                }
            }
            Justify::Default => {}
        }
    }
}

impl std::ops::Index<usize> for Lines {
    type Output = Text;

    fn index(&self, index: usize) -> &Text {
        &self.lines[index]
    }
}

impl std::ops::IndexMut<usize> for Lines {
    fn index_mut(&mut self, index: usize) -> &mut Text {
        &mut self.lines[index]
    }
}

impl IntoIterator for Lines {
    type Item = Text;
    type IntoIter = std::vec::IntoIter<Text>;

    fn into_iter(self) -> Self::IntoIter {
        self.lines.into_iter()
    }
}

impl<'a> IntoIterator for &'a Lines {
    type Item = &'a Text;
    type IntoIter = std::slice::Iter<'a, Text>;

    fn into_iter(self) -> Self::IntoIter {
        self.lines.iter()
    }
}

impl Renderable for Lines {
    /// `yield from self._lines`: each line rendered in turn.
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let renderables: Vec<Arc<dyn Renderable + Send + Sync>> = self
            .lines
            .iter()
            .map(|line| Arc::new(line.clone()) as Arc<dyn Renderable + Send + Sync>)
            .collect();
        Renderables::new(renderables).rich_render(console, options)
    }
}
