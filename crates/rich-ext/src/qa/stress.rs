//! Layout stress: render at many sizes and look for layout failures.
//!
//! [`stress`] renders at every width × height of [`StressOptions`] and
//! reports an [`Issue`] for:
//!
//! * **overflow** — a line wider than the width (the raw render, before the
//!   crop a top-level print applies);
//! * **clipping** — content characters of the widest render missing from a
//!   narrower one. Content excludes whitespace, box/block glyphs and `…`.
//!   A render that shows an ellipsis truncated on purpose and is not
//!   reported, and neither is a width below the renderable's measured
//!   minimum (when it measures one: the default `measure` fills the width),
//!   where loss is expected, nor a render with a height imposed;
//! * **unstable wrapping** — more lines at a wider width than at the
//!   previous one, by more than [`StressOptions::line_tolerance`], among
//!   widths at or above the measured minimum whose renders kept all content;
//! * **panic** — caught with `catch_unwind` (the default panic hook still
//!   prints the message);
//! * **measure mismatch** — `measure()` returning `minimum > maximum`, or a
//!   line whose content (first to last non-space cell, so justification
//!   padding does not count) is wider than the measured maximum clamped to
//!   the width. With [`StressOptions::strict_minimum`],
//!   also a render that fits a width below the measured minimum.

use rich::cells::cell_len;
use rich::{Console, ConsoleOptions, Renderable, Segment, Table, Text};
use serde::{Deserialize, Serialize};

use super::{
    content_chars, is_ellipsis, max_width, plain_lines, plural, table_then_line, visible_width,
    Probe,
};

/// What [`stress`] renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StressOptions {
    /// Default `1, 2, 3, 4, 10, 20, 40, 80, 120, 200`.
    pub widths: Vec<usize>,
    /// Default `None` (unset), 5 and 24.
    pub heights: Vec<Option<usize>>,
    /// Render for a unicode terminal (default `true`).
    pub unicode: bool,
    /// Extra lines allowed when the width grows (default 1).
    pub line_tolerance: usize,
    /// Also report renders that fit below the measured minimum.
    pub strict_minimum: bool,
}

impl Default for StressOptions {
    fn default() -> Self {
        StressOptions {
            widths: vec![1, 2, 3, 4, 10, 20, 40, 80, 120, 200],
            heights: vec![None, Some(5), Some(24)],
            unicode: true,
            line_tolerance: 1,
            strict_minimum: false,
        }
    }
}

impl StressOptions {
    /// Only these widths, height unset.
    pub fn widths(widths: impl Into<Vec<usize>>) -> Self {
        StressOptions {
            widths: widths.into(),
            heights: vec![None],
            ..StressOptions::default()
        }
    }
}

/// A class of layout failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    Overflow,
    Clipping,
    UnstableWrapping,
    Panic,
    MeasureMismatch,
}

impl IssueKind {
    pub fn name(self) -> &'static str {
        match self {
            IssueKind::Overflow => "overflow",
            IssueKind::Clipping => "clipping",
            IssueKind::UnstableWrapping => "unstable wrapping",
            IssueKind::Panic => "panic",
            IssueKind::MeasureMismatch => "measure mismatch",
        }
    }
}

/// One failure at one size.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub width: usize,
    pub height: Option<usize>,
    pub kind: IssueKind,
    pub detail: String,
}

/// Everything [`stress`] found; renders as a table and a summary.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StressReport {
    /// Renders attempted.
    pub renders: usize,
    pub issues: Vec<Issue>,
}

impl StressReport {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
    /// Issues of one kind.
    pub fn of(&self, kind: IssueKind) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(move |i| i.kind == kind)
    }
}

impl Renderable for StressReport {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let table = (!self.issues.is_empty()).then(|| {
            let mut table = Table::new();
            for header in ["Width", "Height", "Issue", "Detail"] {
                table.add_column(header);
            }
            for issue in &self.issues {
                table.add_row_text(vec![
                    Text::new(issue.width.to_string()),
                    Text::new(issue.height.map_or("-".to_owned(), |h| h.to_string())),
                    Text::new(issue.kind.name()),
                    Text::new(issue.detail.clone()),
                ]);
            }
            table
        });
        let summary = format!(
            "{}, {}",
            plural(self.renders, "render"),
            plural(self.issues.len(), "issue")
        );
        table_then_line(table, summary, console, options)
    }
}

/// One size's result.
pub(crate) struct Sample {
    pub width: usize,
    pub lines: Vec<String>,
}

/// Count of each char in `chars`.
fn counts(chars: &[char]) -> std::collections::BTreeMap<char, usize> {
    let mut map = std::collections::BTreeMap::new();
    for &c in chars {
        *map.entry(c).or_insert(0) += 1;
    }
    map
}

/// Characters of `reference` missing (by count) from `lines`.
pub(crate) fn lost_chars(reference: &[String], lines: &[String], ascii: bool) -> String {
    let have = counts(&content_chars(lines, ascii));
    let mut lost = String::new();
    for (c, n) in counts(&content_chars(reference, ascii)) {
        let missing = n.saturating_sub(have.get(&c).copied().unwrap_or(0));
        for _ in 0..missing {
            lost.push(c);
        }
    }
    lost
}

/// Stress `renderable` (see the [module docs](self)).
pub fn stress(renderable: &dyn Renderable, options: &StressOptions) -> StressReport {
    let mut report = StressReport::default();
    let mut widths = options.widths.clone();
    widths.sort_unstable();
    widths.dedup();
    let ascii = !options.unicode;
    for &height in &options.heights {
        let mut samples: Vec<Sample> = Vec::new();
        let mut minimum = 0;
        for &width in &widths {
            let mut probe = Probe::new(width);
            probe.height = height;
            probe.unicode = options.unicode;
            probe.color = crate::capabilities::ColorDepth::None;
            report.renders += 1;
            let issue = |kind, detail: String| Issue {
                width,
                height,
                kind,
                detail,
            };
            let segments = match probe.try_segments(renderable) {
                Ok(segments) => segments,
                Err(message) => {
                    report.issues.push(issue(IssueKind::Panic, message));
                    continue;
                }
            };
            let lines = plain_lines(&segments);
            if let Some((i, line)) = lines.iter().enumerate().find(|(_, l)| cell_len(l) > width) {
                report.issues.push(issue(
                    IssueKind::Overflow,
                    format!(
                        "line {} is {} cells wide: {:?}",
                        i + 1,
                        cell_len(line),
                        line.trim_end()
                    ),
                ));
            }
            let measured = {
                let console = probe.target().console();
                let options = probe.options(&console);
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    renderable.measure(&console, &options)
                }))
                .ok()
            };
            if let Some(m) = measured {
                // The default `measure` fills the width (minimum = width),
                // which says nothing about a minimum: treat it as unknown.
                if width == *widths.last().unwrap_or(&width) && m.minimum < width {
                    minimum = m.minimum;
                }
                let visible = lines.iter().map(|l| visible_width(l)).max().unwrap_or(0);
                if m.minimum > m.maximum {
                    report.issues.push(issue(
                        IssueKind::MeasureMismatch,
                        format!("measure minimum {} > maximum {}", m.minimum, m.maximum),
                    ));
                } else if visible > m.maximum.min(width) && visible <= width {
                    report.issues.push(issue(
                        IssueKind::MeasureMismatch,
                        format!(
                            "rendered {visible} cells wide but measure maximum is {}",
                            m.maximum
                        ),
                    ));
                } else if options.strict_minimum && m.minimum > width && max_width(&lines) <= width
                {
                    report.issues.push(issue(
                        IssueKind::MeasureMismatch,
                        format!("fits in {width} cells but measure minimum is {}", m.minimum),
                    ));
                }
            }
            samples.push(Sample { width, lines });
        }
        // The widest render's minimum decides which widths should hold all
        // content (measured at the widest width, where it is unclamped).
        let Some(reference) = samples.last() else {
            continue;
        };
        for sample in &samples[..samples.len() - 1] {
            // An imposed height cuts content on purpose.
            if height.is_some() || sample.width < minimum {
                continue;
            }
            let truncated = sample.lines.iter().any(|l| l.chars().any(is_ellipsis));
            if truncated {
                continue;
            }
            let lost = lost_chars(&reference.lines, &sample.lines, ascii);
            if !lost.is_empty() {
                report.issues.push(Issue {
                    width: sample.width,
                    height,
                    kind: IssueKind::Clipping,
                    detail: format!(
                        "{} lost vs width {}: {:?}",
                        plural(lost.chars().count(), "character"),
                        reference.width,
                        truncate_detail(&lost)
                    ),
                });
            }
        }
        // Line counts only compare between renders that hold all content: a
        // squeezed render that lost content is short for another reason.
        let stable: Vec<&Sample> = samples
            .iter()
            .filter(|s| {
                s.width >= minimum
                    && !s.lines.iter().any(|l| l.chars().any(is_ellipsis))
                    && lost_chars(&reference.lines, &s.lines, ascii).is_empty()
            })
            .collect();
        for pair in stable.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if b.lines.len() > a.lines.len() + options.line_tolerance {
                report.issues.push(Issue {
                    width: b.width,
                    height,
                    kind: IssueKind::UnstableWrapping,
                    detail: format!(
                        "{} at width {}, {} at width {}",
                        plural(a.lines.len(), "line"),
                        a.width,
                        plural(b.lines.len(), "line"),
                        b.width
                    ),
                });
            }
        }
    }
    report
}

fn truncate_detail(s: &str) -> String {
    if s.chars().count() > 40 {
        let mut out: String = s.chars().take(40).collect();
        out.push('…');
        out
    } else {
        s.to_owned()
    }
}
