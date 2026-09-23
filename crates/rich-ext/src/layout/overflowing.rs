//! An explicit overflow policy for any renderable (#149).
//!
//! Core `Syntax` keeps each source line whole and lets it run past the width
//! (the console's final crop is the only bound), while `Json` word-wraps like
//! `Text`. [`Overflowing`] renders a renderable at its measured natural width
//! and then applies one [`OverflowPolicy`] to every line, so Syntax, JSON and
//! Text share the same cell-aware wrap/fold/crop/ellipsis semantics. Output
//! that already fits is returned untouched, byte for byte.
use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Overflow, Renderable, Segment, Style};

use super::overflow::{fit_segments, OverflowPolicy};

/// Widths at or beyond this are treated as "no intrinsic width": the default
/// `Renderable::measure` echoes whatever width it is offered.
const PROBE_WIDTH: usize = 1 << 16;

/// Wrap a renderable so its lines obey an explicit [`OverflowPolicy`].
pub struct Overflowing {
    inner: Box<dyn Renderable>,
    policy: OverflowPolicy,
}

impl Overflowing {
    pub fn new(inner: Box<dyn Renderable>, policy: OverflowPolicy) -> Self {
        Overflowing { inner, policy }
    }

    pub fn policy(&self) -> OverflowPolicy {
        self.policy
    }
}

/// Split trailing space padding off a line, returning the content and the
/// style the padding was drawn in (so a Syntax background can be restored).
fn trim_padding(mut line: Vec<Segment>) -> (Vec<Segment>, Option<Option<Style>>) {
    let mut pad = None;
    while let Some(last) = line.last_mut() {
        if last.control {
            break;
        }
        let kept = last.text.trim_end_matches(' ').len();
        if kept == last.text.len() {
            break;
        }
        pad.get_or_insert_with(|| last.style.clone());
        if kept == 0 {
            line.pop();
        } else {
            last.text.truncate(kept);
            break;
        }
    }
    (line, pad)
}

impl Renderable for Overflowing {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        if width == 0 {
            return Vec::new();
        }
        let natural = self
            .inner
            .measure(console, &options.update_width(PROBE_WIDTH))
            .maximum;
        let natural = if natural >= PROBE_WIDTH {
            width
        } else {
            natural.max(width)
        };
        let mut opts = options.update_width(natural);
        opts.overflow = Some(Overflow::Ignore);
        opts.no_wrap = Some(true);
        let segments = self.inner.rich_render(console, &opts);
        let lines = Segment::split_lines(&segments);
        let fits =
            |line: &Vec<Segment>| line.iter().map(Segment::cell_length).sum::<usize>() <= width;
        if self.policy == OverflowPolicy::Visible || lines.iter().all(fits) {
            return segments;
        }
        // A padded block (Syntax's background) keeps every row solid, including
        // rows folded out of its widest line, which carried no padding itself.
        let trimmed: Vec<_> = lines.into_iter().map(trim_padding).collect();
        let pad = trimmed.iter().find_map(|(_, pad)| pad.clone());
        let mut out = Vec::new();
        for (index, (content, _)) in trimmed.iter().enumerate() {
            // An empty line folds to zero rows; keep it as one.
            let mut rows = fit_segments(content, width, self.policy);
            if rows.is_empty() {
                rows.push(Vec::new());
            }
            for (row_index, mut row) in rows.into_iter().enumerate() {
                if index + row_index > 0 {
                    out.push(Segment::line());
                }
                if let Some(style) = &pad {
                    let used: usize = row.iter().map(Segment::cell_length).sum();
                    if used < width {
                        row.push(Segment::new(" ".repeat(width - used), style.clone()));
                    }
                }
                out.extend(row);
            }
        }
        out
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.inner.measure(console, options)
    }

    fn fit_to_measurement(&self) -> bool {
        self.inner.fit_to_measurement()
    }
}
