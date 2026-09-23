//! Diagnostics render supplied source only; no filesystem or clock access.
use crate::event::{flatten, theme_style, EventView, Value};
use crate::layout::{fit_segments, OverflowPolicy};
use rich::{Console, ConsoleOptions, Renderable, Segment, Text};
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
#[derive(Clone, Debug)]
pub struct SourceSnippet {
    name: String,
    source: String,
    span: Range<usize>,
    context_lines: usize,
}
impl SourceSnippet {
    pub fn new(
        name: String,
        source: String,
        span: Range<usize>,
        context_lines: usize,
    ) -> Result<Self, DiagnosticError> {
        if span.start > span.end
            || span.end > source.len()
            || !source.is_char_boundary(span.start)
            || !source.is_char_boundary(span.end)
        {
            return Err(DiagnosticError::InvalidSpan);
        }
        Ok(Self {
            name,
            source,
            span,
            context_lines,
        })
    }
    fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let mut offset = 0;
        for (i, line) in self.source.split('\n').enumerate() {
            let end = offset + line.len();
            let active = if self.span.is_empty() {
                self.span.start >= offset && self.span.start <= end
            } else {
                self.span.start <= end && self.span.end > offset
            };
            // Offsets follow the raw source; a CRLF's `\r` must not reach the
            // terminal, where it would move the cursor back over the row.
            lines.push((
                i + 1,
                offset,
                line.strip_suffix('\r').unwrap_or(line),
                active,
            ));
            offset = end.saturating_add(1);
        }
        let first = lines.iter().position(|l| l.3).unwrap_or(0);
        let last = lines.iter().rposition(|l| l.3).unwrap_or(first);
        let digits = (last + 1).to_string().len();
        let mut out = vec![format!("--> {}", self.name)];
        for &(number, start, line, active) in lines
            .iter()
            .skip(first.saturating_sub(self.context_lines))
            .take(
                last.saturating_add(self.context_lines)
                    .saturating_add(1)
                    .min(lines.len())
                    - first.saturating_sub(self.context_lines),
            )
        {
            let mut text = Text::new(line);
            text.expand_tabs(8);
            out.push(format!("{number:>digits$} | {}", text.plain()));
            if active {
                let mut a = self.span.start.saturating_sub(start).min(line.len());
                let mut b = self.span.end.saturating_sub(start).min(line.len());
                // A valid byte span may start inside a visible grapheme. Mark
                // the whole cluster rather than the following display cell.
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
                let count = through.cell_len().saturating_sub(left).max(1);
                out.push(format!(
                    "{} | {}{}",
                    " ".repeat(digits),
                    " ".repeat(left),
                    "^".repeat(count)
                ));
            }
        }
        out
    }
}
#[derive(Clone, Debug)]
pub struct Diagnostic {
    message: String,
    causes: Vec<String>,
    notes: Vec<String>,
    help: Vec<String>,
    labels: Vec<String>,
    snippets: Vec<SourceSnippet>,
    metadata: Vec<(String, Value)>,
    view: EventView,
    overflow: OverflowPolicy,
}
impl Diagnostic {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            causes: Vec::new(),
            notes: Vec::new(),
            help: Vec::new(),
            labels: Vec::new(),
            snippets: Vec::new(),
            metadata: Vec::new(),
            view: EventView::Compact,
            overflow: OverflowPolicy::Fold,
        }
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
            if seen.iter().any(|&p| std::ptr::addr_eq(p, ptr)) {
                result.causes.push("[cycle]".into());
                break;
            }
            seen.push(ptr);
            result.causes.push(error.to_string());
            next = error.source();
        }
        result
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
    pub fn view(mut self, view: EventView) -> Self {
        self.view = view;
        self
    }
    pub fn overflow(mut self, overflow: OverflowPolicy) -> Self {
        self.overflow = overflow;
        self
    }
}
impl Renderable for Diagnostic {
    fn rich_render(&self, c: &Console, o: &ConsoleOptions) -> Vec<Segment> {
        if o.max_width == 0 || o.height == Some(0) {
            return Vec::new();
        }
        let mut rows = fit_segments(
            &[Segment::new(
                &self.message,
                Some(theme_style(c, "diagnostic.message", "bold red")),
            )],
            o.max_width,
            self.overflow,
        );
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
            for snippet in &self.snippets {
                // Crop paired source/underline rows together; wrapping them independently mislabels columns.
                for line in snippet.lines() {
                    rows.extend(fit_segments(
                        &[Segment::new(line, None)],
                        o.max_width,
                        OverflowPolicy::Crop,
                    ));
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
        }
        if let Some(height) = o.height {
            rows.truncate(height);
        }
        flatten(rows)
    }
}
