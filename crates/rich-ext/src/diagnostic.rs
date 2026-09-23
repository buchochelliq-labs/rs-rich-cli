//! Compiler-style diagnostics. They render supplied source only; no
//! filesystem or clock access.
//!
//! A [`Diagnostic`] has a message and, optionally:
//! - a [`Level`] and code (`error[E0308]: …`), the code linked to its docs
//! - a [`Location`], linked through a [`Hyperlinker`]
//! - [`SourceSnippet`]s with primary (`^^^`) and secondary (`---`) spans, each
//!   with an optional label
//! - causes, notes, help and [`Suggestion`]s that show the edited line
//! - a [`StackTrace`]
//!
//! [`DiagnosticInfo`] lets an error type (a `thiserror` enum, for example)
//! supply its level, code, help and location, so [`Diagnostic::from_info`]
//! builds the whole diagnostic. With the `anyhow` feature,
//! [`Diagnostic::from_anyhow`] maps an `anyhow::Error`'s context chain and
//! captured backtrace.
//!
//! Without a level, labels or suggestions, a diagnostic renders as it did
//! before these were added: the message in `diagnostic.message`, then plain
//! rows.
use crate::event::{flatten, theme_style, EventView, Value};
use crate::hyperlink::Hyperlinker;
use crate::layout::{fit_segments, OverflowPolicy};
use crate::stacktrace::StackTrace;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagnosticError {
    InvalidSpan,
}
impl std::fmt::Display for DiagnosticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "source span is outside UTF-8 boundaries")
    }
}
impl std::error::Error for DiagnosticError {}

/// How serious a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Error,
    Warning,
    Info,
    Note,
    Help,
}

impl Level {
    /// The lowercase name shown in the header (`error`, `warning`, …).
    pub fn name(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Info => "info",
            Level::Note => "note",
            Level::Help => "help",
        }
    }

    /// The theme style for this level (`diagnostic.<name>`), with a default.
    pub fn style(self, console: &Console) -> Style {
        let fallback = match self {
            Level::Error => "bold red",
            Level::Warning => "bold yellow",
            Level::Info => "bold blue",
            Level::Note => "bold green",
            Level::Help => "bold cyan",
        };
        theme_style(console, &format!("diagnostic.{}", self.name()), fallback)
    }
}

/// A place in a file: `path:line:column`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Location {
    pub path: String,
    /// The 1-based line.
    pub line: Option<usize>,
    /// The 1-based column.
    pub column: Option<usize>,
}

impl Location {
    pub fn new(path: impl Into<String>, line: Option<usize>, column: Option<usize>) -> Self {
        Location {
            path: path.into(),
            line,
            column,
        }
    }
}

#[derive(Clone, Debug)]
struct SpanLabel {
    span: Range<usize>,
    message: Option<String>,
    primary: bool,
}

fn check_span(source: &str, span: &Range<usize>) -> Result<(), DiagnosticError> {
    if span.start > span.end
        || span.end > source.len()
        || !source.is_char_boundary(span.start)
        || !source.is_char_boundary(span.end)
    {
        return Err(DiagnosticError::InvalidSpan);
    }
    Ok(())
}

/// The display cells before `a` and from `a` to `b` in `line`, with tabs
/// expanded; a byte inside a grapheme cluster widens to the whole cluster.
fn marker_cells(line: &str, a: usize, b: usize) -> (usize, usize) {
    let (mut a, mut b) = (a.min(line.len()), b.min(line.len()));
    // A valid byte span may start inside a visible grapheme. Mark the whole
    // cluster rather than the following display cell.
    for (begin, end, _) in rich::cells::split_graphemes(line).0 {
        if begin < a && a < end {
            a = begin;
        }
        if begin < b && b < end {
            b = end;
        }
    }
    let mut before = Text::new(&line[..a]);
    before.expand_tabs(8);
    let mut through = Text::new(&line[..b]);
    through.expand_tabs(8);
    let left = before.cell_len();
    (left, through.cell_len().saturating_sub(left).max(1))
}

#[derive(Clone, Debug)]
pub struct SourceSnippet {
    name: String,
    source: String,
    /// The first label is the snippet's own primary span.
    labels: Vec<SpanLabel>,
    context_lines: usize,
}
impl SourceSnippet {
    pub fn new(
        name: String,
        source: String,
        span: Range<usize>,
        context_lines: usize,
    ) -> Result<Self, DiagnosticError> {
        check_span(&source, &span)?;
        Ok(Self {
            name,
            source,
            labels: vec![SpanLabel {
                span,
                message: None,
                primary: true,
            }],
            context_lines,
        })
    }

    /// Label the primary span (`^^^ message`).
    pub fn primary_label(mut self, message: impl Into<String>) -> Self {
        self.labels[0].message = Some(message.into());
        self
    }

    /// Add another primary span with a label.
    pub fn primary(
        mut self,
        span: Range<usize>,
        message: impl Into<String>,
    ) -> Result<Self, DiagnosticError> {
        check_span(&self.source, &span)?;
        self.labels.push(SpanLabel {
            span,
            message: Some(message.into()).filter(|m| !m.is_empty()),
            primary: true,
        });
        Ok(self)
    }

    /// Add a secondary span (`--- message`), for related code.
    pub fn secondary(
        mut self,
        span: Range<usize>,
        message: impl Into<String>,
    ) -> Result<Self, DiagnosticError> {
        check_span(&self.source, &span)?;
        self.labels.push(SpanLabel {
            span,
            message: Some(message.into()).filter(|m| !m.is_empty()),
            primary: false,
        });
        Ok(self)
    }

    /// The snippet's name (usually a path).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where the primary span starts: line and column, both 1-based, the
    /// column counted in characters.
    pub fn location(&self) -> Location {
        let start = self.labels[0].span.start;
        let before = &self.source[..start];
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = before[line_start..].chars().count() + 1;
        Location::new(self.name.clone(), Some(line), Some(column))
    }

    fn rows(
        &self,
        primary_style: Option<Style>,
        secondary_style: Option<Style>,
        linker: Option<&Hyperlinker>,
    ) -> Vec<Vec<Segment>> {
        struct Line<'a> {
            number: usize,
            offset: usize,
            end: usize,
            text: &'a str,
        }
        let mut lines = Vec::new();
        let mut offset = 0;
        for (i, line) in self.source.split('\n').enumerate() {
            let end = offset + line.len();
            // Offsets follow the raw source; a CRLF's `\r` must not reach the
            // terminal, where it would move the cursor back over the row.
            lines.push(Line {
                number: i + 1,
                offset,
                end,
                text: line.strip_suffix('\r').unwrap_or(line),
            });
            offset = end.saturating_add(1);
        }
        let touches = |label: &SpanLabel, line: &Line<'_>| {
            if label.span.is_empty() {
                label.span.start >= line.offset && label.span.start <= line.end
            } else {
                label.span.start <= line.end && label.span.end > line.offset
            }
        };
        let active: Vec<bool> = lines
            .iter()
            .map(|line| self.labels.iter().any(|label| touches(label, line)))
            .collect();
        // A label's message goes on the last line it touches.
        let last_line: Vec<Option<usize>> = self
            .labels
            .iter()
            .map(|label| lines.iter().rposition(|line| touches(label, line)))
            .collect();
        let first = active.iter().position(|a| *a).unwrap_or(0);
        let last = active.iter().rposition(|a| *a).unwrap_or(first);
        let digits = (last + 1).to_string().len();
        let gutter = format!("{} | ", " ".repeat(digits));

        let mut header = vec![Segment::new("--> ", None)];
        match linker.and_then(|linker| {
            let location = self.location();
            linker.file_url(&self.name, location.line, location.column)
        }) {
            Some(url) => header.push(Segment::new(
                self.name.clone(),
                Some(Style::new().with_link(url)),
            )),
            None => header.push(Segment::new(self.name.clone(), None)),
        }
        let mut out = vec![Segment::simplify(&header)];

        let window_start = first.saturating_sub(self.context_lines);
        let window_end = last
            .saturating_add(self.context_lines)
            .saturating_add(1)
            .min(lines.len());
        for (index, line) in lines.iter().enumerate().take(window_end).skip(window_start) {
            let mut text = Text::new(line.text);
            text.expand_tabs(8);
            out.push(vec![Segment::new(
                format!("{:>digits$} | {}", line.number, text.plain()),
                None,
            )]);
            if !active[index] {
                continue;
            }
            // Marker cells: secondary first, so a primary span wins overlaps.
            let mut cells: Vec<Option<bool>> = Vec::new();
            let mut messages: Vec<(usize, &str, bool)> = Vec::new();
            let mut order: Vec<usize> = (0..self.labels.len()).collect();
            order.sort_by_key(|&i| self.labels[i].primary);
            for i in order {
                let label = &self.labels[i];
                if !touches(label, line) {
                    continue;
                }
                let a = label.span.start.saturating_sub(line.offset);
                let b = label.span.end.saturating_sub(line.offset);
                let (left, count) = marker_cells(line.text, a, b);
                if cells.len() < left + count {
                    cells.resize(left + count, None);
                }
                for cell in &mut cells[left..left + count] {
                    *cell = Some(label.primary);
                }
                if let (Some(message), Some(true)) =
                    (&label.message, last_line[i].map(|l| l == index))
                {
                    messages.push((left, message, label.primary));
                }
            }
            messages.sort_by_key(|(left, _, _)| *left);
            let style_for = |primary: bool| {
                if primary {
                    primary_style.clone()
                } else {
                    secondary_style.clone()
                }
            };
            let mut row = vec![Segment::new(gutter.clone(), None)];
            let mut run = String::new();
            let mut run_kind: Option<bool> = None;
            let flush = |row: &mut Vec<Segment>, run: &mut String, kind: Option<bool>| {
                if !run.is_empty() {
                    let style = kind.and_then(style_for);
                    row.push(Segment::new(std::mem::take(run), style));
                }
            };
            for cell in &cells {
                if *cell != run_kind {
                    flush(&mut row, &mut run, run_kind);
                    run_kind = *cell;
                }
                run.push(match cell {
                    Some(true) => '^',
                    Some(false) => '-',
                    None => ' ',
                });
            }
            flush(&mut row, &mut run, run_kind);
            // The rightmost label shares the marker row; the others follow.
            if let Some((_, message, primary)) = messages.pop() {
                row.push(Segment::new(" ", None));
                row.push(Segment::new(message.to_string(), style_for(primary)));
            }
            out.push(Segment::simplify(&row));
            for (left, message, primary) in messages.into_iter().rev() {
                out.push(Segment::simplify(&[
                    Segment::new(format!("{gutter}{}", " ".repeat(left)), None),
                    Segment::new(message.to_string(), style_for(primary)),
                ]));
            }
        }
        out
    }
}

/// A suggested fix: a message, and optionally the line as it would read.
#[derive(Clone, Debug)]
pub struct Suggestion {
    message: String,
    edit: Option<SuggestedEdit>,
}

#[derive(Clone, Debug)]
struct SuggestedEdit {
    line_number: usize,
    line: String,
    left: usize,
    count: usize,
}

impl Suggestion {
    /// A suggestion with no edit.
    pub fn new(message: impl Into<String>) -> Self {
        Suggestion {
            message: message.into(),
            edit: None,
        }
    }

    /// Suggest replacing `span` of `source` with `replacement`. The edited
    /// line is shown with the replacement underlined; a span running past its
    /// line is cut at the line's end.
    pub fn replace(
        message: impl Into<String>,
        source: &str,
        span: Range<usize>,
        replacement: &str,
    ) -> Result<Self, DiagnosticError> {
        check_span(source, &span)?;
        let line_start = source[..span.start].rfind('\n').map_or(0, |i| i + 1);
        let line_end = source[span.start..]
            .find('\n')
            .map_or(source.len(), |i| span.start + i);
        let line = &source[line_start..line_end];
        let line = line.strip_suffix('\r').unwrap_or(line);
        let a = span.start - line_start;
        let b = (span.end.min(line_end) - line_start).min(line.len());
        let edited = format!("{}{replacement}{}", &line[..a], &line[b..]);
        let (left, count) = marker_cells(&edited, a, a + replacement.len());
        let mut shown = Text::new(edited);
        shown.expand_tabs(8);
        Ok(Suggestion {
            message: message.into(),
            edit: Some(SuggestedEdit {
                line_number: source[..span.start].matches('\n').count() + 1,
                line: shown.plain().to_string(),
                left,
                count,
            }),
        })
    }

    /// The suggestion's message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    message: String,
    level: Option<Level>,
    code: Option<String>,
    code_url: Option<String>,
    location: Option<Location>,
    causes: Vec<String>,
    notes: Vec<String>,
    help: Vec<String>,
    suggestions: Vec<Suggestion>,
    labels: Vec<String>,
    snippets: Vec<SourceSnippet>,
    metadata: Vec<(String, Value)>,
    trace: Option<StackTrace>,
    linker: Option<Hyperlinker>,
    view: EventView,
    overflow: OverflowPolicy,
}
impl Diagnostic {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            level: None,
            code: None,
            code_url: None,
            location: None,
            causes: Vec::new(),
            notes: Vec::new(),
            help: Vec::new(),
            suggestions: Vec::new(),
            labels: Vec::new(),
            snippets: Vec::new(),
            metadata: Vec::new(),
            trace: None,
            linker: None,
            view: EventView::Compact,
            overflow: OverflowPolicy::Fold,
        }
    }
    /// An error-level diagnostic: `Diagnostic::new(message).level(Level::Error)`.
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(message).level(Level::Error)
    }
    /// A warning-level diagnostic.
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(message).level(Level::Warning)
    }
    pub fn from_error(error: &(dyn std::error::Error + 'static), max_depth: usize) -> Self {
        let mut result = Self::new(error.to_string());
        let mut seen = vec![error as *const dyn std::error::Error];
        let mut next = error.source();
        while let Some(error) = next {
            if result.causes.len() >= max_depth {
                result.causes.push("[truncated]".into());
                break;
            }
            let ptr = error as *const dyn std::error::Error;
            // Identity is address *and* type (the whole wide pointer): an error
            // stored as its wrapper's first field shares the wrapper's address,
            // so comparing addresses alone reported ordinary chains as cycles.
            if seen.iter().any(|&p| std::ptr::eq(p, ptr)) {
                result.causes.push("[cycle]".into());
                break;
            }
            seen.push(ptr);
            result.causes.push(error.to_string());
            next = error.source();
        }
        result
    }
    /// A diagnostic from an error that describes itself through
    /// [`DiagnosticInfo`]: its level, code, help, notes and location, and its
    /// `source()` chain as causes.
    pub fn from_info<E: DiagnosticInfo>(error: &E, max_depth: usize) -> Self {
        let mut result = Self::from_error(error, max_depth).level(error.level());
        result.code = error.code();
        result.code_url = error.code_url();
        result.location = error.location();
        result.help.extend(error.help());
        result.notes.extend(error.notes());
        result
    }
    /// A diagnostic from an `anyhow::Error`: its context chain (outermost
    /// first) as message and causes, and its backtrace, when one was
    /// captured, as a stack trace.
    #[cfg(feature = "anyhow")]
    pub fn from_anyhow(error: &anyhow::Error, max_depth: usize) -> Self {
        let inner: &(dyn std::error::Error + 'static) = error.as_ref();
        let mut result = Self::from_error(inner, max_depth).level(Level::Error);
        let backtrace = error.backtrace();
        if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
            use crate::stacktrace::TraceParser;
            result.trace = crate::stacktrace::RustParser.parse(&backtrace.to_string());
        }
        result
    }
    pub fn level(mut self, level: Level) -> Self {
        self.level = Some(level);
        self
    }
    /// An error code shown as `error[CODE]`.
    pub fn code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
    /// Link the code to its documentation.
    pub fn code_url(mut self, url: impl Into<String>) -> Self {
        self.code_url = Some(url.into());
        self
    }
    /// Where the problem is: `--> path:line:column` under the header.
    pub fn location(mut self, location: Location) -> Self {
        self.location = Some(location);
        self
    }
    /// Link the location, snippet names and stack-trace frames with `linker`.
    /// Without one, locations render as plain text.
    pub fn hyperlinker(mut self, linker: Hyperlinker) -> Self {
        self.linker = Some(linker);
        self
    }
    pub fn cause(mut self, message: impl Into<String>) -> Self {
        self.causes.push(message.into());
        self
    }
    pub fn note(mut self, message: impl Into<String>) -> Self {
        self.notes.push(message.into());
        self
    }
    pub fn help(mut self, message: impl Into<String>) -> Self {
        self.help.push(message.into());
        self
    }
    pub fn suggestion(mut self, suggestion: Suggestion) -> Self {
        self.suggestions.push(suggestion);
        self
    }
    pub fn label(mut self, message: impl Into<String>) -> Self {
        self.labels.push(message.into());
        self
    }
    pub fn snippet(mut self, snippet: SourceSnippet) -> Self {
        self.snippets.push(snippet);
        self
    }
    pub fn metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.push((key.into(), value));
        self
    }
    /// Show a stack trace in the expanded view.
    pub fn trace(mut self, trace: StackTrace) -> Self {
        self.trace = Some(trace);
        self
    }
    pub fn view(mut self, view: EventView) -> Self {
        self.view = view;
        self
    }
    pub fn overflow(mut self, overflow: OverflowPolicy) -> Self {
        self.overflow = overflow;
        self
    }

    /// The headline message.
    pub fn message(&self) -> &str {
        &self.message
    }
    /// The level, if set.
    pub fn get_level(&self) -> Option<Level> {
        self.level
    }
    /// The code, if set.
    pub fn get_code(&self) -> Option<&str> {
        self.code.as_deref()
    }
    /// The location: the one set, else the first snippet's primary span.
    pub fn get_location(&self) -> Option<Location> {
        self.location
            .clone()
            .or_else(|| self.snippets.first().map(SourceSnippet::location))
    }
    /// The causes, outermost first.
    pub fn causes(&self) -> &[String] {
        &self.causes
    }
    /// The attached stack trace, if any.
    pub fn get_trace(&self) -> Option<&StackTrace> {
        self.trace.as_ref()
    }
    /// The documentation link for the code, if set.
    pub fn get_code_url(&self) -> Option<&str> {
        self.code_url.as_deref()
    }
    /// The notes, in order.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }
    /// The help messages, in order.
    pub fn help_messages(&self) -> &[String] {
        &self.help
    }
    /// The suggested fixes, in order.
    pub fn suggestions(&self) -> &[Suggestion] {
        &self.suggestions
    }
    /// The labels, in order.
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    fn header(&self, c: &Console) -> Vec<Segment> {
        let Some(level) = self.level else {
            let mut segments = Vec::new();
            if let Some(code) = &self.code {
                segments.push(Segment::new(format!("[{code}] "), None));
            }
            segments.push(Segment::new(
                &self.message,
                Some(theme_style(c, "diagnostic.message", "bold red")),
            ));
            return segments;
        };
        let style = level.style(c);
        let mut segments = vec![Segment::new(level.name(), Some(style.clone()))];
        if let Some(code) = &self.code {
            let code_style = match &self.code_url {
                Some(url) => style.clone().with_link(url.clone()),
                None => style.clone(),
            };
            segments.push(Segment::new(format!("[{code}]"), Some(code_style)));
        }
        segments.push(Segment::new(
            ": ",
            Some(theme_style(c, "diagnostic.headline", "bold")),
        ));
        segments.push(Segment::new(
            &self.message,
            Some(theme_style(c, "diagnostic.headline", "bold")),
        ));
        segments
    }
}

/// An error that describes itself as a diagnostic. Implement it on an error
/// type — a `thiserror` enum, for example — and [`Diagnostic::from_info`]
/// maps it without hand-building the renderable. Every method has a default.
pub trait DiagnosticInfo: std::error::Error + 'static {
    /// The level (default error).
    fn level(&self) -> Level {
        Level::Error
    }
    /// An error code.
    fn code(&self) -> Option<String> {
        None
    }
    /// A documentation link for the code.
    fn code_url(&self) -> Option<String> {
        None
    }
    /// Help text.
    fn help(&self) -> Option<String> {
        None
    }
    /// Notes.
    fn notes(&self) -> Vec<String> {
        Vec::new()
    }
    /// Where the error is.
    fn location(&self) -> Option<Location> {
        None
    }
    /// This error as a diagnostic, following up to 16 causes.
    fn to_diagnostic(&self) -> Diagnostic
    where
        Self: Sized,
    {
        Diagnostic::from_info(self, 16)
    }
}

impl Renderable for Diagnostic {
    fn rich_render(&self, c: &Console, o: &ConsoleOptions) -> Vec<Segment> {
        if o.max_width == 0 || o.height == Some(0) {
            return Vec::new();
        }
        let mut rows = fit_segments(&self.header(c), o.max_width, self.overflow);
        if let Some(location) = &self.location {
            let gutter = theme_style(c, "diagnostic.gutter", "bold blue");
            let linker = self.linker.clone().unwrap_or_else(Hyperlinker::disabled);
            let text = linker.location(&location.path, location.line, location.column, "");
            let mut segments = vec![Segment::new("  --> ", Some(gutter))];
            segments.extend(text.render(c.theme(), &Style::new()));
            rows.extend(fit_segments(&segments, o.max_width, OverflowPolicy::Crop));
        }
        for cause in &self.causes {
            rows.extend(fit_segments(
                &[Segment::new(format!("caused by: {cause}"), None)],
                o.max_width,
                self.overflow,
            ));
        }
        if self.view == EventView::Expanded {
            for label in &self.labels {
                rows.extend(fit_segments(
                    &[Segment::new(label, None)],
                    o.max_width,
                    self.overflow,
                ));
            }
            let (primary, secondary) = match self.level {
                Some(level) => (
                    Some(level.style(c)),
                    Some(theme_style(c, "diagnostic.secondary", "bold blue")),
                ),
                None => (None, None),
            };
            for (index, snippet) in self.snippets.iter().enumerate() {
                // The `  --> path:line:column` row already names the first
                // snippet's file, so its own `--> path` header is dropped
                // rather than repeating the location (as rustc shows it once).
                let repeated = index == 0
                    && self
                        .location
                        .as_ref()
                        .is_some_and(|location| location.path == snippet.name);
                // Crop paired source/underline rows together; wrapping them independently mislabels columns.
                let lines = snippet.rows(primary.clone(), secondary.clone(), self.linker.as_ref());
                for line in lines.into_iter().skip(usize::from(repeated)) {
                    rows.extend(fit_segments(&line, o.max_width, OverflowPolicy::Crop));
                }
            }
            for (key, value) in &self.metadata {
                rows.extend(fit_segments(
                    &[Segment::new(
                        format!("{key}={}", value.format(true, 0)),
                        None,
                    )],
                    o.max_width,
                    self.overflow,
                ));
            }
            for (prefix, values) in [("note", &self.notes), ("help", &self.help)] {
                for value in values {
                    rows.extend(fit_segments(
                        &[Segment::new(format!("{prefix}: {value}"), None)],
                        o.max_width,
                        self.overflow,
                    ));
                }
            }
            let added = theme_style(c, "diagnostic.suggestion", "green");
            for suggestion in &self.suggestions {
                rows.extend(fit_segments(
                    &[Segment::new(format!("help: {}", suggestion.message), None)],
                    o.max_width,
                    self.overflow,
                ));
                if let Some(edit) = &suggestion.edit {
                    let digits = edit.line_number.to_string().len();
                    let line = format!("{:>digits$} | {}", edit.line_number, edit.line);
                    rows.extend(fit_segments(
                        &[Segment::new(line, None)],
                        o.max_width,
                        OverflowPolicy::Crop,
                    ));
                    let marks = [
                        Segment::new(
                            format!("{} | {}", " ".repeat(digits), " ".repeat(edit.left)),
                            None,
                        ),
                        Segment::new("+".repeat(edit.count), Some(added.clone())),
                    ];
                    rows.extend(fit_segments(&marks, o.max_width, OverflowPolicy::Crop));
                }
            }
            if let Some(trace) = &self.trace {
                let view = trace
                    .render_options()
                    .hyperlinker(self.linker.clone().unwrap_or_else(Hyperlinker::disabled));
                rows.extend(c.render_lines(&view, &o.update_width(o.max_width), false));
            }
        }
        if let Some(height) = o.height {
            rows.truncate(height);
        }
        flatten(rows)
    }
}
