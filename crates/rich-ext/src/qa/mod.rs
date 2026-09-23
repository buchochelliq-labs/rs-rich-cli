//! Render quality tooling: screenshots, layout stress, lint, explain,
//! profiling, fuzzing, a capability matrix and benchmarks.
//!
//! Everything here renders through an explicit [`RenderTarget`] built from
//! caller-supplied capabilities, never from the environment, so results are
//! the same on every machine. Findings are computed by *analysing* rendered
//! output (and `measure()`), not by instrumenting core.
//!
//! - [`screenshot`]: a width × colour × unicode matrix of renders and an
//!   approval workflow (`.txt` approved files, `.new` pending ones).
//! - [`stress`]: many width × height renders checked for overflow, clipping,
//!   unstable wrapping, panics and measure disagreement.
//! - [`lint`]: render and markup lints with a JSON report.
//! - [`explain`]: why output looks the way it does (wrapping, truncation,
//!   colour downgrades, fidelity, unicode fallback).
//! - [`profile`]: measure/render timings, output size, frame cost and,
//!   with [`profile::CountingAllocator`] installed, allocations.
//! - [`fuzz`]: seeded random renderables, invariants and minimisation.
//! - [`matrix`]: fixtures × capability profiles, structural checks and
//!   approvals.
//! - [`bench`](mod@bench): a small benchmark harness, a stable JSON run format and
//!   baseline comparison.
//!
//! Requires the `testing` feature.

pub mod bench;
pub mod explain;
pub mod fuzz;
pub mod lint;
pub mod matrix;
pub mod profile;
pub mod screenshot;
pub mod stress;

use std::panic::{catch_unwind, AssertUnwindSafe};

use rich::cells::cell_len;
use rich::protocol::{Support, TargetCapabilities};
use rich::{Console, ConsoleOptions, Renderable, Segment, Theme};

use crate::capabilities::ColorDepth;
use crate::target::{RenderTarget, TargetKind};

pub use screenshot::{assert_screenshots, Approvals, Matrix, Outcome, Screenshot, Shot};

/// Height given to a target when a probe sets none: large enough that no
/// ordinary renderable is cut, while `options.height` itself stays unset.
pub(crate) const UNBOUNDED_HEIGHT: usize = 10_000;

/// One deterministic render configuration.
#[derive(Clone, Debug)]
pub(crate) struct Probe {
    pub width: usize,
    /// `options.height`; `None` leaves it unset (the renderable picks).
    pub height: Option<usize>,
    pub color: ColorDepth,
    pub unicode: bool,
    pub hyperlinks: bool,
    pub theme: Theme,
}

impl Probe {
    pub fn new(width: usize) -> Self {
        Probe {
            width,
            height: None,
            color: ColorDepth::TrueColor,
            unicode: true,
            hyperlinks: true,
            theme: crate::theme::extended_theme(),
        }
    }

    /// The probe for a target's capabilities (height left unset).
    pub fn from_capabilities(caps: &TargetCapabilities, theme: Theme) -> Self {
        Probe {
            width: caps.width,
            height: None,
            color: ColorDepth::from_color_system(caps.color_system),
            unicode: caps.unicode,
            hyperlinks: caps.hyperlinks,
            theme,
        }
    }

    pub fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            width: self.width,
            height: self.height.unwrap_or(UNBOUNDED_HEIGHT),
            color_system: self.color.color_system(),
            interactive: false,
            unicode: self.unicode,
            hyperlinks: self.hyperlinks,
            sixel: Support::Unsupported,
        }
    }

    pub fn target(&self) -> RenderTarget {
        RenderTarget::new(TargetKind::Capture, self.capabilities(), self.theme.clone())
    }

    pub fn options(&self, console: &Console) -> ConsoleOptions {
        let mut options = console.options().update_width(self.width);
        options.height = self.height;
        options
    }

    /// Render without a panic guard: control segments dropped, links
    /// stripped when unsupported.
    pub fn segments(&self, renderable: &dyn Renderable) -> Vec<Segment> {
        if self.width == 0 {
            return Vec::new();
        }
        let console = self.target().console();
        let mut segments = renderable.rich_render(&console, &self.options(&console));
        segments.retain(|s| !s.control);
        if !self.hyperlinks {
            for segment in &mut segments {
                segment.style = segment.style.as_ref().map(|s| s.update_link(None));
            }
        }
        segments
    }

    /// [`segments`](Self::segments), with a panic reported as its message.
    pub fn try_segments(&self, renderable: &dyn Renderable) -> Result<Vec<Segment>, String> {
        catch_unwind(AssertUnwindSafe(|| self.segments(renderable))).map_err(panic_message)
    }
}

/// The message of a caught panic.
pub(crate) fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_owned()
    }
}

/// Visible text of `segments`, one string per line (a trailing newline adds
/// no line).
pub(crate) fn plain_lines(segments: &[Segment]) -> Vec<String> {
    let text: String = segments
        .iter()
        .filter(|s| !s.control)
        .map(|s| s.text.as_str())
        .collect();
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    lines
}

/// Widest line in cells.
pub(crate) fn max_width(lines: &[String]) -> usize {
    lines.iter().map(|l| cell_len(l)).max().unwrap_or(0)
}

/// Cells from the first to the last non-space character: the extent of a
/// line's content, so justification padding on either side is not counted.
pub(crate) fn visible_width(line: &str) -> usize {
    cell_len(line.trim_matches(' '))
}

/// Box drawing and block glyphs: layout decoration whose count depends on
/// the width, so content comparisons ignore it. With `ascii`, the ASCII box
/// characters `+ - | =` count as decoration too.
pub(crate) fn is_decoration(c: char, ascii: bool) -> bool {
    matches!(c, '\u{2500}'..='\u{259f}') || (ascii && matches!(c, '+' | '-' | '|' | '='))
}

/// Ellipsis glyphs a truncating renderer inserts.
pub(crate) fn is_ellipsis(c: char) -> bool {
    c == '…'
}

/// Content characters of `lines`: no spaces, decoration or ellipses.
pub(crate) fn content_chars(lines: &[String], ascii: bool) -> Vec<char> {
    lines
        .iter()
        .flat_map(|l| l.chars())
        .filter(|&c| !c.is_whitespace() && !is_decoration(c, ascii) && !is_ellipsis(c))
        .collect()
}

/// `name` of a colour depth as used in keys and reports.
pub(crate) fn depth_key(depth: ColorDepth) -> &'static str {
    match depth {
        ColorDepth::None => "none",
        ColorDepth::Ansi16 => "ansi16",
        ColorDepth::Ansi256 => "ansi256",
        ColorDepth::TrueColor => "truecolor",
    }
}

/// `n` with `word` pluralised by `s`.
pub(crate) fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// Delegates to a renderable with `options.height` cleared, so a target's
/// height does not make fill-to-height renderables (a `Panel`) grow.
pub(crate) struct NoHeight<'a>(pub &'a dyn Renderable);

impl Renderable for NoHeight<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut options = options.clone();
        options.height = None;
        self.0.rich_render(console, &options)
    }
    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        self.0.measure(console, options)
    }
}

/// Render `table`, then a summary line.
pub(crate) fn table_then_line(
    table: Option<rich::Table>,
    summary: String,
    console: &Console,
    options: &ConsoleOptions,
) -> Vec<Segment> {
    let mut out = Vec::new();
    if let Some(table) = table {
        out = table.rich_render(console, options);
        if out.last().is_some_and(|s| !s.text.ends_with('\n')) {
            out.push(Segment::line());
        }
    }
    out.extend(rich::Text::new(summary).rich_render(console, options));
    if out.last().is_some_and(|s| !s.text.ends_with('\n')) {
        out.push(Segment::line());
    }
    out
}
