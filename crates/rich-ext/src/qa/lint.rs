//! Render lint: problems visible in rendered output, markup and text spans.
//!
//! [`lint`] renders a renderable for a target described by [`LintOptions`]
//! and checks:
//!
//! * **layout** — [`stress`](super::stress) at the lint widths: overflow and
//!   panics are errors, clipped text a warning, unstable wrapping and
//!   measure mismatches info;
//! * **hyperlinks** — every OSC 8 link in the segments: an empty URL, one
//!   with whitespace or controls, `javascript:`/`data:`/`vbscript:`, or an
//!   `http(s)` URL without a host is an error; a relative URL (no scheme) or
//!   a scheme outside [`KNOWN_SCHEMES`] a warning; a link whose visible text
//!   is blank a warning;
//! * **colour-only distinctions** — the same status token (a symbol such as
//!   `●`, `✔`, `✖`, `▲` or a status word such as `ok`, `error`, `fail`)
//!   shown in two different colours with identical attributes, anywhere in
//!   the render. The token cannot tell the reader which is which without
//!   colour. Only these tokens are checked, so ordinary coloured text never
//!   triggers the rule;
//! * **capability assumptions** — colours deeper than the target (info,
//!   raised to a warning when two colours collapse into one), non-ASCII
//!   glyphs on an ASCII target, hyperlinks on a target without them (a
//!   warning when the URL is not also in the text) and `blink` (always a
//!   warning: distracting and unreliable).
//!
//! Undefined theme names cannot be seen after rendering (they render as
//! nothing), so [`lint_markup`] and [`lint_text`] check markup strings and
//! [`Text`] spans against a [`Theme`] instead, suggesting the nearest name.

use std::collections::{BTreeMap, BTreeSet};

use rich::color::ColorType;
use rich::{
    Color, Console, ConsoleOptions, Renderable, Segment, Style, StyleType, Table, Text, Theme,
};
use serde::{Deserialize, Serialize};

use super::stress::{stress, IssueKind, StressOptions};
use super::{depth_key, plain_lines, plural, table_then_line, Probe};
use crate::capabilities::ColorDepth;
use crate::fidelity::style_without_color;

/// How bad a finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn name(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// Which check produced a finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    Overflow,
    ClippedText,
    Panic,
    UnstableWrapping,
    MeasureMismatch,
    MarkupError,
    UnknownStyle,
    BrokenHyperlink,
    EmptyLinkText,
    ColorOnlyDistinction,
    ColorDepth,
    NonAsciiGlyph,
    HyperlinksUnsupported,
    Blink,
}

impl Rule {
    /// The kebab-case rule id.
    pub fn id(self) -> &'static str {
        match self {
            Rule::Overflow => "overflow",
            Rule::ClippedText => "clipped-text",
            Rule::Panic => "panic",
            Rule::UnstableWrapping => "unstable-wrapping",
            Rule::MeasureMismatch => "measure-mismatch",
            Rule::MarkupError => "markup-error",
            Rule::UnknownStyle => "unknown-style",
            Rule::BrokenHyperlink => "broken-hyperlink",
            Rule::EmptyLinkText => "empty-link-text",
            Rule::ColorOnlyDistinction => "color-only-distinction",
            Rule::ColorDepth => "color-depth",
            Rule::NonAsciiGlyph => "non-ascii-glyph",
            Rule::HyperlinksUnsupported => "hyperlinks-unsupported",
            Rule::Blink => "blink",
        }
    }
}

/// One problem.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintFinding {
    pub rule: Rule,
    pub severity: Severity,
    pub message: String,
    /// The width rendered at, for render findings.
    pub width: Option<usize>,
    /// 1-based output line, when one applies.
    pub line: Option<usize>,
}

impl LintFinding {
    fn new(rule: Rule, severity: Severity, message: impl Into<String>) -> Self {
        LintFinding {
            rule,
            severity,
            message: message.into(),
            width: None,
            line: None,
        }
    }
    fn at(mut self, width: usize, line: Option<usize>) -> Self {
        self.width = Some(width);
        self.line = line;
        self
    }
}

/// The target a render is linted for.
#[derive(Clone, Debug)]
pub struct LintOptions {
    /// Widths to render at (default 20, 40, 80). Output checks use the widest.
    pub widths: Vec<usize>,
    /// Target colour depth (default 16 colours, the safe assumption).
    pub color: ColorDepth,
    /// Whether the target renders non-ASCII (default `true`).
    pub unicode: bool,
    /// Whether the target renders OSC 8 links (default `false`).
    pub hyperlinks: bool,
    /// Run the layout checks (default `true`).
    pub layout: bool,
    /// Theme to render with (default the extended theme).
    pub theme: Theme,
}

impl Default for LintOptions {
    fn default() -> Self {
        LintOptions {
            widths: vec![20, 40, 80],
            color: ColorDepth::Ansi16,
            unicode: true,
            hyperlinks: false,
            layout: true,
            theme: crate::theme::extended_theme(),
        }
    }
}

impl LintOptions {
    /// Lint for a full-featured terminal: truecolor, unicode, links.
    pub fn capable() -> Self {
        LintOptions {
            color: ColorDepth::TrueColor,
            hyperlinks: true,
            ..LintOptions::default()
        }
    }
    pub fn widths(mut self, widths: impl Into<Vec<usize>>) -> Self {
        self.widths = widths.into();
        self
    }
    pub fn color(mut self, color: ColorDepth) -> Self {
        self.color = color;
        self
    }
    pub fn unicode(mut self, unicode: bool) -> Self {
        self.unicode = unicode;
        self
    }
    pub fn hyperlinks(mut self, hyperlinks: bool) -> Self {
        self.hyperlinks = hyperlinks;
        self
    }
    pub fn layout(mut self, layout: bool) -> Self {
        self.layout = layout;
        self
    }
}

/// URL schemes a link may use without a warning.
pub const KNOWN_SCHEMES: &[&str] = &[
    "http",
    "https",
    "file",
    "mailto",
    "ftp",
    "sftp",
    "ssh",
    "git",
    "tel",
    "irc",
    "news",
    "x-man-page",
    "vscode",
    "vscode-insiders",
    "vscodium",
    "cursor",
    "idea",
    "subl",
    "txmt",
    "zed",
];

/// Problems with a link URL, if any.
pub fn check_url(url: &str) -> Option<(Severity, String)> {
    if url.trim().is_empty() {
        return Some((Severity::Error, "link with an empty URL".into()));
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Some((
            Severity::Error,
            format!("link URL {url:?} contains whitespace or control characters"),
        ));
    }
    let scheme = url.split_once(':').map(|(s, _)| s).filter(|s| {
        let mut chars = s.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    let Some(scheme) = scheme else {
        return Some((
            Severity::Warning,
            format!("relative link URL {url:?}: a terminal has no base to resolve it against"),
        ));
    };
    let lower = scheme.to_ascii_lowercase();
    if matches!(lower.as_str(), "javascript" | "data" | "vbscript") {
        return Some((Severity::Error, format!("unsafe link scheme {scheme}:")));
    }
    if matches!(lower.as_str(), "http" | "https") {
        let host = url[scheme.len() + 1..]
            .strip_prefix("//")
            .map(|rest| rest.split(['/', '?', '#']).next().unwrap_or(""));
        if host.is_none_or(str::is_empty) {
            return Some((Severity::Error, format!("link URL {url:?} has no host")));
        }
    }
    if !KNOWN_SCHEMES.contains(&lower.as_str()) {
        return Some((
            Severity::Warning,
            format!("link scheme {scheme}: is not widely supported by terminals"),
        ));
    }
    None
}

/// Symbols that commonly carry status.
const STATUS_SYMBOLS: &[&str] = &[
    "●", "○", "◉", "■", "□", "◆", "◇", "•", "✔", "✓", "✖", "✗", "✘", "×", "⚠", "▲", "▼", "⬤", "★",
    "☆", "*", "x", "v", "!", "?",
];

/// Words that commonly carry status (case-insensitive).
const STATUS_WORDS: &[&str] = &[
    "ok",
    "okay",
    "pass",
    "passed",
    "fail",
    "failed",
    "error",
    "warning",
    "warn",
    "info",
    "pending",
    "skipped",
    "skip",
    "success",
    "failure",
    "up",
    "down",
    "yes",
    "no",
    "on",
    "off",
    "true",
    "false",
    "healthy",
    "unhealthy",
    "online",
    "offline",
];

fn is_status_token(token: &str) -> bool {
    STATUS_SYMBOLS.contains(&token)
        || STATUS_WORDS.contains(&token.to_ascii_lowercase().as_str())
        || crate::a11y::Status::ALL
            .iter()
            .any(|s| s.word().eq_ignore_ascii_case(token))
}

/// Colour key of a style: foreground and background names.
fn color_key(style: &Style) -> (Option<String>, Option<String>) {
    let name = |c: Option<&Color>| {
        c.filter(|c| !c.is_default())
            .map(|c| c.get_truecolor().map_or(c.name.clone(), |t| t.hex()))
    };
    (name(style.color()), name(style.bgcolor()))
}

fn color_label(key: &(Option<String>, Option<String>)) -> String {
    match key {
        (Some(fg), Some(bg)) => format!("{fg} on {bg}"),
        (Some(fg), None) => fg.clone(),
        (None, Some(bg)) => format!("on {bg}"),
        (None, None) => "default".into(),
    }
}

fn depth_of(color: &Color) -> ColorDepth {
    match color.kind {
        ColorType::Default => ColorDepth::None,
        ColorType::Standard | ColorType::Windows => ColorDepth::Ansi16,
        ColorType::EightBit => ColorDepth::Ansi256,
        ColorType::Truecolor => ColorDepth::TrueColor,
    }
}

/// `color` as the target shows it: a palette number.
fn downgraded(color: &Color, depth: ColorDepth) -> Option<u8> {
    let system = depth.color_system()?;
    color.downgrade(system).number
}

/// Lint a renderable (see the [module docs](self)).
pub fn lint(renderable: &dyn Renderable, options: &LintOptions) -> Vec<LintFinding> {
    let mut findings = Vec::new();
    if options.layout && !options.widths.is_empty() {
        let stress_options = StressOptions {
            widths: options.widths.clone(),
            heights: vec![None],
            unicode: options.unicode,
            ..StressOptions::default()
        };
        for issue in stress(renderable, &stress_options).issues {
            let (rule, severity) = match issue.kind {
                IssueKind::Overflow => (Rule::Overflow, Severity::Error),
                IssueKind::Panic => (Rule::Panic, Severity::Error),
                IssueKind::Clipping => (Rule::ClippedText, Severity::Warning),
                IssueKind::UnstableWrapping => (Rule::UnstableWrapping, Severity::Info),
                IssueKind::MeasureMismatch => (Rule::MeasureMismatch, Severity::Info),
            };
            findings.push(LintFinding::new(rule, severity, issue.detail).at(issue.width, None));
        }
    }
    let Some(&width) = options.widths.iter().max() else {
        return findings;
    };
    let mut probe = Probe::new(width);
    probe.color = options.color;
    probe.unicode = options.unicode;
    probe.hyperlinks = true;
    probe.theme = options.theme.clone();
    let Ok(segments) = probe.try_segments(renderable) else {
        return findings;
    };
    findings.extend(lint_segments(&segments, width, options));
    findings
}

/// The output checks of [`lint`] on already-rendered segments.
pub fn lint_segments(
    segments: &[Segment],
    width: usize,
    options: &LintOptions,
) -> Vec<LintFinding> {
    let mut findings = Vec::new();
    let lines = Segment::split_lines(segments);

    // Hyperlinks: runs of segments sharing one link on a line.
    let mut links_seen: Vec<(usize, String, String)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let mut current: Option<(String, String)> = None;
        for segment in line {
            let link = segment.style.as_ref().and_then(|s| s.link());
            match (&mut current, link) {
                (Some((url, text)), Some(l)) if url == l => text.push_str(&segment.text),
                (_, link) => {
                    if let Some((url, text)) = current.take() {
                        links_seen.push((index + 1, url, text));
                    }
                    current = link.map(|l| (l.to_owned(), segment.text.clone()));
                }
            }
        }
        if let Some((url, text)) = current {
            links_seen.push((index + 1, url, text));
        }
    }
    let mut checked = BTreeSet::new();
    for (line, url, text) in &links_seen {
        if checked.insert(url.clone()) {
            if let Some((severity, message)) = check_url(url) {
                findings.push(
                    LintFinding::new(Rule::BrokenHyperlink, severity, message)
                        .at(width, Some(*line)),
                );
            }
        }
        if text.trim().is_empty() {
            findings.push(
                LintFinding::new(
                    Rule::EmptyLinkText,
                    Severity::Warning,
                    format!("link to {url:?} has no visible text"),
                )
                .at(width, Some(*line)),
            );
        }
    }
    if !options.hyperlinks && !links_seen.is_empty() {
        let plain: String = segments.iter().map(|s| s.text.as_str()).collect();
        let hidden: BTreeSet<&str> = links_seen
            .iter()
            .map(|(_, url, _)| url.as_str())
            .filter(|url| !plain.contains(url))
            .collect();
        let (severity, message) = if hidden.is_empty() {
            (
                Severity::Info,
                format!(
                    "{} dropped on a target without OSC 8; each URL is still in the text",
                    plural(links_seen.len(), "link")
                ),
            )
        } else {
            (
                Severity::Warning,
                format!(
                    "target has no OSC 8 support: {} lost (URL not in the text: {})",
                    plural(hidden.len(), "link"),
                    hidden.into_iter().collect::<Vec<_>>().join(", ")
                ),
            )
        };
        findings
            .push(LintFinding::new(Rule::HyperlinksUnsupported, severity, message).at(width, None));
    }

    // Blink.
    if let Some((index, segment)) = lines.iter().enumerate().find_map(|(i, line)| {
        line.iter()
            .find(|s| {
                s.style
                    .as_ref()
                    .is_some_and(|st| st.attr(4) == Some(true) || st.attr(5) == Some(true))
            })
            .map(|s| (i, s))
    }) {
        findings.push(
            LintFinding::new(
                Rule::Blink,
                Severity::Warning,
                format!(
                    "blinking text {:?}: distracting, and ignored by many terminals",
                    segment.text.trim()
                ),
            )
            .at(width, Some(index + 1)),
        );
    }

    // Non-ASCII glyphs on an ASCII target.
    if !options.unicode {
        let plain = plain_lines(segments);
        let mut glyphs = BTreeSet::new();
        let mut first = None;
        for (i, line) in plain.iter().enumerate() {
            for c in line.chars().filter(|c| !c.is_ascii()) {
                glyphs.insert(c);
                first.get_or_insert(i + 1);
            }
        }
        if !glyphs.is_empty() {
            let shown: String = glyphs.iter().take(12).collect();
            findings.push(
                LintFinding::new(
                    Rule::NonAsciiGlyph,
                    Severity::Warning,
                    format!(
                        "{} on an ASCII-only target: {shown}",
                        plural(glyphs.len(), "non-ASCII glyph")
                    ),
                )
                .at(width, first),
            );
        }
    }

    // Colour depth.
    if options.color != ColorDepth::None {
        let mut deep: BTreeMap<String, Option<u8>> = BTreeMap::new();
        for segment in segments {
            let Some(style) = &segment.style else {
                continue;
            };
            for color in [style.color(), style.bgcolor()].into_iter().flatten() {
                if depth_of(color) > options.color {
                    let label = color
                        .get_truecolor()
                        .map_or(color.name.clone(), |t| t.hex());
                    deep.insert(label, downgraded(color, options.color));
                }
            }
        }
        if !deep.is_empty() {
            let mut by_target: BTreeMap<u8, Vec<&str>> = BTreeMap::new();
            for (label, number) in &deep {
                if let Some(n) = number {
                    by_target.entry(*n).or_default().push(label);
                }
            }
            let collisions: Vec<String> = by_target
                .iter()
                .filter(|(_, labels)| labels.len() > 1)
                .map(|(n, labels)| format!("{} → {n}", labels.join(", ")))
                .collect();
            let list: Vec<String> = deep
                .iter()
                .map(|(label, n)| match n {
                    Some(n) => format!("{label} → {n}"),
                    None => label.clone(),
                })
                .collect();
            let (severity, message) = if collisions.is_empty() {
                (
                    Severity::Info,
                    format!(
                        "{} deeper than the {} target, approximated: {}",
                        plural(deep.len(), "colour"),
                        depth_key(options.color),
                        list.join(", ")
                    ),
                )
            } else {
                (
                    Severity::Warning,
                    format!(
                        "distinct colours become one on the {} target: {}",
                        depth_key(options.color),
                        collisions.join("; ")
                    ),
                )
            };
            findings.push(LintFinding::new(Rule::ColorDepth, severity, message).at(width, None));
        }
    }

    // Colour-only distinctions.
    let mut tokens: BTreeMap<(String, String), BTreeMap<String, usize>> = BTreeMap::new();
    for (index, line) in lines.iter().enumerate() {
        for segment in line {
            let Some(style) = &segment.style else {
                continue;
            };
            let key = color_key(style);
            let attrs = style_without_color(style).definition();
            for token in segment.text.split_whitespace() {
                let token = token.trim_matches(|c: char| matches!(c, ':' | ',' | '.' | '[' | ']'));
                if token.is_empty() || !is_status_token(token) {
                    continue;
                }
                tokens
                    .entry((token.to_owned(), attrs.clone()))
                    .or_default()
                    .entry(color_label(&key))
                    .or_insert(index + 1);
            }
        }
    }
    for ((token, _), colors) in tokens {
        if colors.len() > 1 {
            let line = colors.values().min().copied();
            let names: Vec<&str> = colors.keys().map(String::as_str).collect();
            findings.push(
                LintFinding::new(
                    Rule::ColorOnlyDistinction,
                    Severity::Warning,
                    format!(
                        "{token:?} appears in {} that differ only by colour ({}); add a \
                         distinct symbol or word",
                        plural(colors.len(), "style"),
                        names.join(", ")
                    ),
                )
                .at(width, line),
            );
        }
    }
    findings
}

/// Lint a markup string: syntax errors, then [`lint_text`] on the result.
pub fn lint_markup(markup: &str, theme: &Theme) -> Vec<LintFinding> {
    match rich::markup::render(markup) {
        Ok(text) => lint_text(&text, theme),
        Err(e) => vec![LintFinding::new(
            Rule::MarkupError,
            Severity::Error,
            e.to_string(),
        )],
    }
}

/// Lint the spans of a [`Text`]: names that are neither theme keys nor
/// style definitions, broken link URLs and blink.
pub fn lint_text(text: &Text, theme: &Theme) -> Vec<LintFinding> {
    let mut findings = Vec::new();
    for span in text.spans() {
        // Span offsets are bytes.
        let excerpt = text
            .plain()
            .get(span.start..span.end)
            .unwrap_or("")
            .to_owned();
        let style = match theme.get_style(&span.style) {
            Ok(style) => style,
            Err(_) => {
                let StyleType::Name(name) = &span.style else {
                    continue;
                };
                let hint = nearest(name, theme)
                    .map(|n| format!("; did you mean {n:?}?"))
                    .unwrap_or_default();
                findings.push(LintFinding::new(
                    Rule::UnknownStyle,
                    Severity::Error,
                    format!("unknown style {name:?} on {excerpt:?}{hint}"),
                ));
                continue;
            }
        };
        if let Some(url) = style.link() {
            if let Some((severity, message)) = check_url(url) {
                findings.push(LintFinding::new(Rule::BrokenHyperlink, severity, message));
            }
            if excerpt.trim().is_empty() {
                findings.push(LintFinding::new(
                    Rule::EmptyLinkText,
                    Severity::Warning,
                    format!("link to {url:?} has no visible text"),
                ));
            }
        }
        if style.attr(4) == Some(true) || style.attr(5) == Some(true) {
            findings.push(LintFinding::new(
                Rule::Blink,
                Severity::Warning,
                format!("blinking text {excerpt:?}"),
            ));
        }
    }
    findings
}

/// The theme name closest to `name` within two edits, if any.
fn nearest<'a>(name: &str, theme: &'a Theme) -> Option<&'a str> {
    theme
        .names()
        .map(|n| (edit_distance(name, n), n))
        .filter(|(d, _)| *d <= 2)
        .min()
        .map(|(_, n)| n)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// Findings as a table, a summary and JSON.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintReport {
    pub findings: Vec<LintFinding>,
}

impl LintReport {
    pub fn new(findings: Vec<LintFinding>) -> Self {
        LintReport { findings }
    }
    /// Findings at `severity`.
    pub fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == severity)
            .count()
    }
    /// Whether any finding is an error.
    pub fn has_errors(&self) -> bool {
        self.count(Severity::Error) > 0
    }
    /// Pretty JSON: `{"findings": [{"rule", "severity", "message", "width", "line"}]}`.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }
}

impl Renderable for LintReport {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let table = (!self.findings.is_empty()).then(|| {
            let mut table = Table::new();
            for header in ["Severity", "Rule", "Where", "Message"] {
                table.add_column(header);
            }
            let mut findings: Vec<&LintFinding> = self.findings.iter().collect();
            findings.sort_by_key(|f| f.severity);
            for f in findings {
                let severity_style = match f.severity {
                    Severity::Error => "bold red",
                    Severity::Warning => "yellow",
                    Severity::Info => "dim",
                };
                let place = match (f.width, f.line) {
                    (Some(w), Some(l)) => format!("w{w} l{l}"),
                    (Some(w), None) => format!("w{w}"),
                    (None, Some(l)) => format!("l{l}"),
                    (None, None) => String::new(),
                };
                table.add_row_text(vec![
                    Text::styled(f.severity.name(), severity_style),
                    Text::new(f.rule.id()),
                    Text::new(place),
                    Text::new(f.message.clone()),
                ]);
            }
            table
        });
        let summary = format!(
            "{}: {} errors, {} warnings, {} info",
            plural(self.findings.len(), "finding"),
            self.count(Severity::Error),
            self.count(Severity::Warning),
            self.count(Severity::Info)
        );
        table_then_line(table, summary, console, options)
    }
}
