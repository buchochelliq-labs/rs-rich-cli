//! Small core renderables the code area composes with: already-rendered
//! lines (upstream's `Segments`), a vertical stack (upstream's `Group`) and a
//! blank line (upstream's `""` in a render result).

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;

/// Rendered lines, output as they are whatever the width (upstream's
/// `Segments`, which the caller rendered at the right width already).
pub(crate) struct Fixed {
    pub(crate) lines: Vec<Vec<CoreSegment>>,
}

impl Renderable for Fixed {
    fn rich_render(&self, _console: &CoreConsole, _options: &CoreOptions) -> Vec<CoreSegment> {
        join_lines(self.lines.clone())
    }

    fn measure(&self, _console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        let width = self
            .lines
            .iter()
            .map(|line| line.iter().map(CoreSegment::cell_length).sum::<usize>())
            .max()
            .unwrap_or(0)
            .min(options.max_width);
        CoreMeasurement::new(width, width)
    }
}

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
    pub(crate) children: Vec<Box<dyn Renderable>>,
}

impl Renderable for Stack {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let mut lines = Vec::new();
        for child in &self.children {
            let mut child_options = options.clone();
            child_options.height = None;
            let segments = child.rich_render(console, &child_options);
            if segments.is_empty() {
                continue;
            }
            lines.extend(CoreSegment::split_lines(&segments));
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
