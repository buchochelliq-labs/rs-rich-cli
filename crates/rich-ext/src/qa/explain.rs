//! Why output looks the way it does, found by analysis rather than by
//! instrumenting core.
//!
//! [`explain`] renders for a target and compares against reference renders:
//!
//! * **wrapped** — the natural width (`measure().maximum`) exceeds the
//!   target width. Lines of the natural-width render are matched to target
//!   lines by their content characters (whitespace and box glyphs ignored),
//!   so each wrapped source line reports the target lines it became. When
//!   content was also lost the match is skipped and only counts are given.
//! * **truncated** — more `…` in the target render than in the natural one
//!   (ellipsis overflow); **cropped** — content missing without an ellipsis.
//! * **colour downgraded** — each colour deeper than the target, with its
//!   mapping, e.g. `#ff8700 → 208 → yellow` on a 16-colour target (the
//!   256-colour step is shown for reference; core maps the original colour);
//!   **colour removed** on a no-colour target.
//! * **fidelity** — the [`Fidelity`] level the target selects, and why;
//!   with a capability [`Report`], each capability's value and source.
//! * **unicode fallback** — on an ASCII target, the glyph substitutions
//!   (`╭ → +`) and any non-ASCII left in place.
//! * **hyperlinks dropped** — links in the render on a target without OSC 8.

use std::collections::{BTreeMap, BTreeSet};

use rich::color::ColorType;
use rich::protocol::{RenderEnvironment, Support, TargetCapabilities};
use rich::{Color, ColorSystem, Console, ConsoleOptions, Renderable, Segment, Table, Text};
use serde::{Deserialize, Serialize};

use super::stress::lost_chars;
use super::{content_chars, is_ellipsis, plain_lines, plural, table_then_line, Probe};
use crate::capabilities::{ColorDepth, Report};
use crate::fidelity::{Fidelity, FidelitySource, Policy};
use crate::target::RenderTarget;

/// What happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Wrapped,
    Truncated,
    Cropped,
    ColorDowngraded,
    ColorRemoved,
    Fidelity,
    Capability,
    UnicodeFallback,
    HyperlinksDropped,
}

impl EventKind {
    pub fn name(self) -> &'static str {
        match self {
            EventKind::Wrapped => "wrapped",
            EventKind::Truncated => "truncated",
            EventKind::Cropped => "cropped",
            EventKind::ColorDowngraded => "colour downgraded",
            EventKind::ColorRemoved => "colour removed",
            EventKind::Fidelity => "fidelity",
            EventKind::Capability => "capability",
            EventKind::UnicodeFallback => "unicode fallback",
            EventKind::HyperlinksDropped => "hyperlinks dropped",
        }
    }
}

/// One explained decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub kind: EventKind,
    /// 1-based target output lines concerned (may be empty).
    pub lines: Vec<usize>,
    /// What happened.
    pub summary: String,
    /// Why.
    pub reason: String,
}

impl Event {
    fn new(kind: EventKind, summary: impl Into<String>, reason: impl Into<String>) -> Self {
        Event {
            kind,
            lines: Vec::new(),
            summary: summary.into(),
            reason: reason.into(),
        }
    }
    fn lines(mut self, lines: Vec<usize>) -> Self {
        self.lines = lines;
        self
    }
}

/// Everything [`explain`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Explanation {
    pub width: usize,
    /// `measure().maximum` at an unconstrained width.
    pub natural_width: usize,
    pub events: Vec<Event>,
}

impl Explanation {
    /// Events of one kind.
    pub fn of(&self, kind: EventKind) -> impl Iterator<Item = &Event> {
        self.events.iter().filter(move |e| e.kind == kind)
    }
    pub fn has(&self, kind: EventKind) -> bool {
        self.of(kind).next().is_some()
    }
}

/// Widest width [`explain`] renders its natural-width reference at.
const NATURAL_CAP: usize = 1000;
/// Wrapped source lines listed individually before summarising.
const MAX_WRAP_EVENTS: usize = 20;

/// Standard colour names by number.
pub const STANDARD_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright_black",
    "bright_red",
    "bright_green",
    "bright_yellow",
    "bright_blue",
    "bright_magenta",
    "bright_cyan",
    "bright_white",
];

/// How `color` reaches a `depth` target, e.g. `#ff8700 → 208 → yellow`;
/// `None` when it is shown as is.
pub fn downgrade_chain(color: &Color, depth: ColorDepth) -> Option<String> {
    let label = match color.kind {
        ColorType::Default => return None,
        ColorType::Truecolor => color.triplet.map_or(color.name.clone(), |t| t.hex()),
        _ => color.name.clone(),
    };
    let rank = |kind: ColorType| match kind {
        ColorType::Standard | ColorType::Windows | ColorType::Default => 0,
        ColorType::EightBit => 1,
        ColorType::Truecolor => 2,
    };
    let target = match depth {
        ColorDepth::None => return None,
        ColorDepth::Ansi16 => 0,
        ColorDepth::Ansi256 => 1,
        ColorDepth::TrueColor => 2,
    };
    if rank(color.kind) <= target {
        return None;
    }
    let mut chain = vec![label];
    if color.kind == ColorType::Truecolor {
        if let Some(n) = color.downgrade(ColorSystem::EightBit).number {
            chain.push(n.to_string());
        }
    }
    if target == 0 {
        if let Some(n) = color.downgrade(ColorSystem::Standard).number {
            chain.push(STANDARD_NAMES[usize::from(n % 16)].to_owned());
        }
    }
    Some(chain.join(" → "))
}

/// Explain `renderable` on `target`.
pub fn explain(renderable: &dyn Renderable, target: &RenderTarget) -> Explanation {
    explain_with_report(renderable, target, None)
}

/// [`explain`] for a console at `width`: its colour system, `ascii_only`
/// and terminal flag (hyperlinks are assumed on a terminal).
pub fn explain_console(
    renderable: &dyn Renderable,
    console: &Console,
    width: usize,
) -> Explanation {
    let caps = TargetCapabilities {
        width,
        height: console.height(),
        color_system: console.color_system(),
        interactive: console.is_terminal(),
        unicode: !console.ascii_only(),
        hyperlinks: console.is_terminal(),
        sixel: Support::Unsupported,
    };
    run(
        renderable,
        Probe::from_capabilities(&caps, console.theme().clone()),
        &caps,
        None,
    )
}

/// [`explain`], adding each capability's value and source from `report`.
pub fn explain_with_report(
    renderable: &dyn Renderable,
    target: &RenderTarget,
    report: Option<&Report>,
) -> Explanation {
    let caps = target.capabilities();
    let theme = target.console().theme().clone();
    run(
        renderable,
        Probe::from_capabilities(&caps, theme),
        &caps,
        report,
    )
}

fn run(
    renderable: &dyn Renderable,
    probe: Probe,
    caps: &TargetCapabilities,
    report: Option<&Report>,
) -> Explanation {
    let width = probe.width;
    let mut explanation = Explanation {
        width,
        ..Explanation::default()
    };
    let events = &mut explanation.events;

    // Fidelity and capability sources come first: they frame the rest.
    let level = match report {
        Some(report) => Fidelity::select(report, &Policy::default()),
        None => Fidelity::select(caps, &Policy::default()),
    };
    let facts = match report {
        Some(report) => report.facts(),
        None => caps.facts(),
    };
    let why = if !facts.unicode {
        "no unicode: ASCII glyphs, no styles".to_owned()
    } else if !facts.color {
        if facts.interactive {
            "no colour on an interactive terminal: attributes only".to_owned()
        } else {
            "no colour and not a terminal: plain text".to_owned()
        }
    } else if facts.animation && facts.interactive {
        "colour on an interactive terminal that allows animation".to_owned()
    } else {
        "colour, but not an animating terminal: static output".to_owned()
    };
    events.push(Event::new(
        EventKind::Fidelity,
        format!("fidelity {}", level.name()),
        why,
    ));
    if let Some(report) = report {
        for (name, value, origin, reason) in report.rows() {
            let reason = if reason.is_empty() {
                origin.to_string()
            } else {
                format!("{origin}: {reason}")
            };
            events.push(Event::new(
                EventKind::Capability,
                format!("{name} = {value}"),
                reason,
            ));
        }
    }

    // The target render (links kept so dropped links can be counted).
    let mut target_probe = probe.clone();
    target_probe.hyperlinks = true;
    let Ok(segments) = target_probe.try_segments(renderable) else {
        return explanation;
    };
    let lines = plain_lines(&segments);

    // Natural width and the reference render at it.
    let natural = {
        let console = probe.target().console();
        let mut options = console.options().update_width(NATURAL_CAP);
        options.height = None;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            renderable.measure(&console, &options)
        }))
        .map(|m| m.maximum)
        .unwrap_or(width)
    };
    explanation.natural_width = natural;
    let events = &mut explanation.events;
    let mut wide_probe = probe.clone();
    wide_probe.width = natural.clamp(width, NATURAL_CAP);
    let wide = wide_probe
        .try_segments(renderable)
        .map(|s| plain_lines(&s))
        .unwrap_or_default();
    let ascii = !probe.unicode;

    let lost = lost_chars(&wide, &lines, ascii);
    let ellipses = |ls: &[String]| {
        ls.iter()
            .map(|l| l.chars().filter(|&c| is_ellipsis(c)).count())
            .sum::<usize>()
    };
    if natural > width {
        let mut wrapped = Vec::new();
        if lost.is_empty() && ellipses(&lines) == ellipses(&wide) {
            wrapped = map_wraps(&wide, &lines, ascii);
        }
        if wrapped.is_empty() {
            events.push(Event::new(
                EventKind::Wrapped,
                format!(
                    "{} at natural width {natural} became {} at width {width}",
                    plural(wide.len(), "line"),
                    plural(lines.len(), "line")
                ),
                format!("natural width {natural} > available width {width}"),
            ));
        }
        let total = wrapped.len();
        for (source, target_lines) in wrapped.into_iter().take(MAX_WRAP_EVENTS) {
            let (first, last) = (target_lines[0], target_lines[target_lines.len() - 1]);
            events.push(
                Event::new(
                    EventKind::Wrapped,
                    format!("source line {source} wrapped onto lines {first}–{last}"),
                    format!(
                        "{} cells of content in {width} cells",
                        super::visible_width(&wide[source - 1])
                    ),
                )
                .lines(target_lines),
            );
        }
        if total > MAX_WRAP_EVENTS {
            events.push(Event::new(
                EventKind::Wrapped,
                format!("…and {} more wrapped lines", total - MAX_WRAP_EVENTS),
                format!("natural width {natural} > available width {width}"),
            ));
        }
    }
    if ellipses(&lines) > ellipses(&wide) {
        let at: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.chars().any(is_ellipsis))
            .map(|(i, _)| i + 1)
            .collect();
        events.push(
            Event::new(
                EventKind::Truncated,
                format!(
                    "{} ellipsised ({} cut)",
                    plural(at.len(), "line"),
                    plural(lost.chars().count(), "character")
                ),
                "content wider than its space, with ellipsis overflow",
            )
            .lines(at),
        );
    } else if !lost.is_empty() {
        let shown: String = lost.chars().take(30).collect();
        events.push(Event::new(
            EventKind::Cropped,
            format!(
                "{} cut without an ellipsis: {shown:?}",
                plural(lost.chars().count(), "character")
            ),
            "crop overflow, or the width is below the content's minimum",
        ));
    }

    // Colours.
    let mut colors: BTreeMap<String, Option<String>> = BTreeMap::new();
    for segment in &segments {
        let Some(style) = &segment.style else {
            continue;
        };
        for color in [style.color(), style.bgcolor()].into_iter().flatten() {
            if color.is_default() {
                continue;
            }
            let label = color
                .get_truecolor()
                .map_or(color.name.clone(), |t| t.hex());
            colors
                .entry(label)
                .or_insert_with(|| downgrade_chain(color, probe.color));
        }
    }
    if probe.color == ColorDepth::None {
        if !colors.is_empty() {
            events.push(Event::new(
                EventKind::ColorRemoved,
                format!("{} not shown", plural(colors.len(), "colour")),
                "the target has no colour (NO_COLOR, not a terminal, or configured)",
            ));
        }
    } else {
        for chain in colors.values().flatten() {
            events.push(Event::new(
                EventKind::ColorDowngraded,
                chain.clone(),
                format!("target colour depth is {}", probe.color.name()),
            ));
        }
    }

    // Unicode fallback.
    if ascii {
        let mut unicode_probe = probe.clone();
        unicode_probe.unicode = true;
        if let Ok(u) = unicode_probe.try_segments(renderable) {
            let u = plain_lines(&u);
            let mut pairs = BTreeSet::new();
            for (a, b) in u.iter().zip(&lines) {
                if a.chars().count() == b.chars().count() {
                    for (x, y) in a.chars().zip(b.chars()) {
                        if x != y {
                            pairs.insert((x, y));
                        }
                    }
                }
            }
            if !pairs.is_empty() {
                let shown: Vec<String> = pairs
                    .iter()
                    .take(16)
                    .map(|(x, y)| format!("{x} → {y}"))
                    .collect();
                events.push(Event::new(
                    EventKind::UnicodeFallback,
                    format!("ASCII substitutions: {}", shown.join(", ")),
                    "the target is ASCII-only, so boxes and guides use ASCII",
                ));
            }
        }
        let remaining: BTreeSet<char> = lines
            .iter()
            .flat_map(|l| l.chars())
            .filter(|c| !c.is_ascii())
            .collect();
        if !remaining.is_empty() {
            let at: Vec<usize> = lines
                .iter()
                .enumerate()
                .filter(|(_, l)| !l.is_ascii())
                .map(|(i, _)| i + 1)
                .collect();
            events.push(
                Event::new(
                    EventKind::UnicodeFallback,
                    format!(
                        "{} left as is: {}",
                        plural(remaining.len(), "non-ASCII glyph"),
                        remaining.iter().take(16).collect::<String>()
                    ),
                    "content glyphs are not substituted; wrap in fidelity::Degrade for ASCII",
                )
                .lines(at),
            );
        }
    }

    // Hyperlinks.
    if !probe.hyperlinks {
        let links: BTreeSet<&str> = segments
            .iter()
            .filter_map(|s| s.style.as_ref().and_then(|st| st.link()))
            .collect();
        if !links.is_empty() {
            events.push(Event::new(
                EventKind::HyperlinksDropped,
                format!("{} rendered as plain text", plural(links.len(), "link")),
                "the target does not support OSC 8 hyperlinks",
            ));
        }
    }
    explanation
}

/// Match natural-width lines to target lines by content; returns each
/// source line (1-based) that became more than one target line, with those
/// target lines. Empty when the content does not line up.
fn map_wraps(wide: &[String], lines: &[String], ascii: bool) -> Vec<(usize, Vec<usize>)> {
    let key = |l: &String| -> Vec<char> { content_chars(std::slice::from_ref(l), ascii) };
    let wide_keys: Vec<Vec<char>> = wide.iter().map(key).collect();
    let line_keys: Vec<Vec<char>> = lines.iter().map(key).collect();
    if wide_keys.concat() != line_keys.concat() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut j = 0;
    for (i, wk) in wide_keys.iter().enumerate() {
        let mut taken = Vec::new();
        if wk.is_empty() {
            if j < line_keys.len() && line_keys[j].is_empty() {
                j += 1;
            }
            continue;
        }
        let mut have = 0;
        // Skip blank target lines that precede this content.
        while j < line_keys.len() && line_keys[j].is_empty() {
            j += 1;
        }
        while j < line_keys.len() && have < wk.len() {
            have += line_keys[j].len();
            taken.push(j + 1);
            j += 1;
        }
        if have != wk.len() {
            return Vec::new();
        }
        if taken.len() > 1 {
            out.push((i + 1, taken));
        }
    }
    out
}

/// An [`Explanation`] as a table of events.
pub struct ExplanationView<'a> {
    explanation: &'a Explanation,
}

impl<'a> ExplanationView<'a> {
    pub fn new(explanation: &'a Explanation) -> Self {
        ExplanationView { explanation }
    }
}

impl Renderable for ExplanationView<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let e = self.explanation;
        let table = (!e.events.is_empty()).then(|| {
            let mut table = Table::new();
            for header in ["Event", "Lines", "What", "Why"] {
                table.add_column(header);
            }
            for event in &e.events {
                let lines = match event.lines.as_slice() {
                    [] => String::new(),
                    [one] => one.to_string(),
                    [first, .., last] => format!("{first}–{last}"),
                };
                table.add_row_text(vec![
                    Text::new(event.kind.name()),
                    Text::new(lines),
                    Text::new(event.summary.clone()),
                    Text::new(event.reason.clone()),
                ]);
            }
            table
        });
        let summary = format!(
            "width {}, natural width {}, {}",
            e.width,
            e.natural_width,
            plural(e.events.len(), "event")
        );
        table_then_line(table, summary, console, options)
    }
}
