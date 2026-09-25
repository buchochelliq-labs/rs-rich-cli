//! Renderable containers.
//!
//! Port of upstream `rich/containers.py` (the `Renderables` class). `Lines` is
//! internal to upstream's `Text` wrapping, which this port does at the segment
//! level, so it is not ported.

use std::sync::Arc;

use crate::console::{Console, ConsoleOptions};
use crate::measure::Measurement;
use crate::protocol::Renderable;
use crate::segment::Segment;

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
