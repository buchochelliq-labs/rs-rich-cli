//! Styled text with spans.
//!
//! Port of upstream `rich/text.py` (core subset). [`Text`] is a plain string
//! plus a list of [`Span`]s, each applying a [`Style`] to a byte range. Spans
//! may overlap and nest; [`Text::render`] flattens them into non-overlapping
//! [`Segment`]s by combining every span covering each run.

use crate::cells::{cell_len, set_cell_size};
use crate::console::{Justify, Overflow};
use crate::errors::Result;
use crate::markup;
use crate::segment::Segment;
use crate::style::{Style, StyleType};
use crate::theme::Theme;

/// The control codes upstream drops in `Text.__init__` (`strip_control_codes`):
/// BEL, backspace, vertical tab, form feed and carriage return. Tab and newline
/// are layout, not control, and are kept.
///
/// Crate-visible because **every** producer of a `Text` plus its spans must agree
/// on this set. `markup::render` computes span byte-offsets as it builds the
/// plain string; if it kept a code that `Text::new` later removed, the content
/// would shift left while the offsets stayed put, and a boundary landing inside
/// a multi-byte character panics on slicing.
pub(crate) fn is_control_code(c: char) -> bool {
    matches!(c, '\u{7}' | '\u{8}' | '\u{b}' | '\u{c}' | '\r')
}

/// Cell width of a tab stop. Upstream's `Console.tab_size` default; a per-console
/// override is not ported yet (see `docs/DIVERGENCES.md`).
pub const DEFAULT_TAB_SIZE: usize = 8;

/// A style applied to a byte range `[start, end)` of a [`Text`]'s plain string.
/// Mirrors `rich.text.Span`.
///
/// The style may be a *name* rather than a resolved [`Style`]; see [`StyleType`].
/// Names are resolved when the text is rendered, against the theme of whichever
/// console renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub style: StyleType,
}

/// Styled text. Mirrors `rich.text.Text`.
#[derive(Debug, Clone, Default)]
pub struct Text {
    plain: String,
    spans: Vec<Span>,
    /// A base style applied to the whole text. May be an unresolved name.
    style: StyleType,
    /// How lines are justified within the render width.
    justify: Justify,
    /// What to do with lines wider than the render width. `None` defers to the
    /// console options, then to [`Overflow::Fold`].
    overflow: Option<Overflow>,
    /// Whether to skip wrapping. `None` defers to the console options, then to
    /// `false`.
    no_wrap: Option<bool>,
}

impl Text {
    /// Strip the control codes upstream removes in `Text.__init__`
    /// (`strip_control_codes`): BEL, backspace, vertical tab, form feed and
    /// carriage return. Tab and newline are deliberately kept — they are layout,
    /// not control.
    fn strip_control_codes(text: &str) -> String {
        if text.chars().any(is_control_code) {
            text.chars().filter(|c| !is_control_code(*c)).collect()
        } else {
            text.to_string()
        }
    }

    /// Plain, unstyled text.
    pub fn new(plain: impl Into<String>) -> Self {
        Text {
            plain: Text::strip_control_codes(&plain.into()),
            spans: Vec::new(),
            style: StyleType::default(),
            justify: Justify::Default,
            overflow: None,
            no_wrap: None,
        }
    }

    /// Text with a base style, which may be a style *name* resolved at render
    /// time (`Text::styled("hi", "repr.number")`) or a resolved [`Style`].
    pub fn styled(plain: impl Into<String>, style: impl Into<StyleType>) -> Self {
        Text {
            // Strips too: upstream's `Text.__init__` does this regardless of
            // style, and a constructor that skipped it would reintroduce the
            // offset divergence the moment a caller added spans.
            plain: Text::strip_control_codes(&plain.into()),
            spans: Vec::new(),
            style: style.into(),
            justify: Justify::Default,
            overflow: None,
            no_wrap: None,
        }
    }

    /// Set how lines are justified within the render width (builder form).
    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = justify;
        self
    }

    /// Set how lines are justified within the render width.
    pub fn set_justify(&mut self, justify: Justify) {
        self.justify = justify;
    }

    /// This text's own justify method.
    pub fn get_justify(&self) -> Justify {
        self.justify
    }

    /// Set what happens to lines wider than the render width (builder form).
    pub fn overflow(mut self, overflow: Overflow) -> Self {
        self.overflow = Some(overflow);
        self
    }

    /// Set what happens to lines wider than the render width. Pass `None` to
    /// defer to the console options.
    pub fn set_overflow(&mut self, overflow: Option<Overflow>) {
        self.overflow = overflow;
    }

    /// This text's own overflow method, if it set one.
    pub fn get_overflow(&self) -> Option<Overflow> {
        self.overflow
    }

    /// Disable (or re-enable) wrapping for this text (builder form).
    pub fn no_wrap(mut self, no_wrap: bool) -> Self {
        self.no_wrap = Some(no_wrap);
        self
    }

    /// Disable (or re-enable) wrapping. Pass `None` to defer to the console
    /// options.
    pub fn set_no_wrap(&mut self, no_wrap: Option<bool>) {
        self.no_wrap = no_wrap;
    }

    /// This text's own no-wrap setting, if it set one.
    pub fn get_no_wrap(&self) -> Option<bool> {
        self.no_wrap
    }

    /// Shorten this text to at most `max_width` cells, optionally padding it out
    /// to exactly `max_width` when it is shorter. Port of `Text.truncate`.
    ///
    /// `overflow` defaults to this text's own method, then to [`Overflow::Fold`];
    /// [`Overflow::Ignore`] leaves the text alone entirely. Note that `Fold` and
    /// `Crop` behave identically here — folding is a property of *wrapping*, and
    /// a line that has already been wrapped can only be cut.
    pub fn truncate(&mut self, max_width: usize, overflow: Option<Overflow>, pad: bool) {
        let overflow = overflow.or(self.overflow).unwrap_or(Overflow::Fold);
        if overflow == Overflow::Ignore {
            return;
        }
        let length = cell_len(&self.plain);
        if length > max_width {
            let plain = if overflow == Overflow::Ellipsis {
                // `…` is one cell wide, so cut one short and add it back.
                format!(
                    "{}…",
                    set_cell_size(&self.plain, max_width.saturating_sub(1))
                )
            } else {
                set_cell_size(&self.plain, max_width)
            };
            self.set_plain(plain);
        } else if pad {
            let plain = set_cell_size(&self.plain, max_width);
            self.set_plain(plain);
        }
    }

    /// Replace the plain string, clamping every span into the new length so no
    /// span can dangle past the end. Upstream's `Text.plain` setter does the
    /// same via `_trim_spans`.
    fn set_plain(&mut self, plain: String) {
        let length = plain.len();
        self.plain = plain;
        self.spans.retain(|span| span.start < length);
        for span in &mut self.spans {
            span.end = span.end.min(length);
        }
    }

    /// An empty `Text` carrying this one's style, justify, overflow and no-wrap.
    /// Port of `Text.blank_copy`.
    pub fn blank_copy(&self) -> Text {
        Text {
            plain: String::new(),
            spans: Vec::new(),
            style: self.style.clone(),
            justify: self.justify,
            overflow: self.overflow,
            no_wrap: self.no_wrap,
        }
    }

    /// Cut this text at each byte offset in `offsets`, returning the pieces.
    /// Port of `Text.divide`.
    ///
    /// Every piece inherits the base style, justify, overflow and no-wrap, and
    /// each span is re-based onto the pieces it covers. Spans that would come out
    /// empty are dropped, matching upstream's `new_end > new_start`.
    ///
    /// Offsets are **byte** offsets (as everywhere else in this port's span
    /// arithmetic) and must fall on `char` boundaries.
    pub fn divide(&self, offsets: &[usize]) -> Vec<Text> {
        if offsets.is_empty() {
            return vec![self.clone()];
        }
        let mut bounds = Vec::with_capacity(offsets.len() + 2);
        bounds.push(0);
        bounds.extend(offsets.iter().copied());
        bounds.push(self.plain.len());

        let mut lines: Vec<Text> = bounds
            .windows(2)
            .map(|w| {
                let (start, end) = (w[0].min(self.plain.len()), w[1].min(self.plain.len()));
                let mut line = self.blank_copy();
                if start < end {
                    line.plain = self.plain[start..end].to_string();
                }
                line
            })
            .collect();

        for span in &self.spans {
            for (index, window) in bounds.windows(2).enumerate() {
                let (line_start, line_end) = (window[0], window[1]);
                let new_start = span.start.max(line_start) - line_start;
                let new_end = span.end.min(line_end).saturating_sub(line_start);
                if new_end > new_start {
                    lines[index].spans.push(Span {
                        start: new_start,
                        end: new_end,
                        style: span.style.clone(),
                    });
                }
            }
        }
        lines
    }

    /// Split on `separator`. Port of `Text.split`.
    ///
    /// `include_separator` keeps the separator at the end of each piece.
    /// `allow_blank` keeps the trailing empty piece that a text ending in the
    /// separator would otherwise produce.
    ///
    /// # Panics
    /// If `separator` is empty, which upstream asserts against.
    pub fn split(&self, separator: &str, include_separator: bool, allow_blank: bool) -> Vec<Text> {
        assert!(!separator.is_empty(), "separator must not be empty");
        if !self.plain.contains(separator) {
            return vec![self.clone()];
        }
        let matches: Vec<usize> = self
            .plain
            .match_indices(separator)
            .map(|(i, _)| i)
            .collect();
        let mut lines = if include_separator {
            let offsets: Vec<usize> = matches.iter().map(|s| s + separator.len()).collect();
            self.divide(&offsets)
        } else {
            // Cut on both sides of every separator, then drop the separators.
            let mut offsets = Vec::with_capacity(matches.len() * 2);
            for start in &matches {
                offsets.push(*start);
                offsets.push(start + separator.len());
            }
            self.divide(&offsets)
                .into_iter()
                .filter(|line| line.plain != separator)
                .collect()
        };
        if !allow_blank && self.plain.ends_with(separator) {
            lines.pop();
        }
        lines
    }

    /// Pad both sides with `count` copies of `character`. Port of `Text.pad`.
    pub fn pad(&mut self, count: usize, character: char) {
        self.pad_left(count, character);
        self.pad_right(count, character);
    }

    /// Pad the left with `count` copies of `character`, shifting every span to
    /// follow the text. Port of `Text.pad_left`.
    pub fn pad_left(&mut self, count: usize, character: char) {
        if count == 0 {
            return;
        }
        let padding: String = std::iter::repeat_n(character, count).collect();
        let offset = padding.len();
        self.plain.insert_str(0, &padding);
        for span in &mut self.spans {
            span.start += offset;
            span.end += offset;
        }
    }

    /// Pad the right with `count` copies of `character`. Port of
    /// `Text.pad_right`. Spans are untouched, so the padding is unstyled.
    pub fn pad_right(&mut self, count: usize, character: char) {
        if count == 0 {
            return;
        }
        self.plain.extend(std::iter::repeat_n(character, count));
    }

    /// Drop the last `amount` bytes, clipping any span that reached into them.
    /// Port of `Text.right_crop`.
    pub fn right_crop(&mut self, amount: usize) {
        if amount == 0 {
            return;
        }
        let max_offset = self.plain.len().saturating_sub(amount);
        let plain = self.plain[..max_offset].to_string();
        self.set_plain(plain);
    }

    /// Remove trailing whitespace. Port of `Text.rstrip`.
    pub fn rstrip(&mut self) {
        let plain = self.plain.trim_end().to_string();
        self.set_plain(plain);
    }

    /// Remove *only as much* trailing whitespace as it takes to get down to
    /// `size` cells, leaving the rest. Port of `Text.rstrip_end`.
    ///
    /// This is what lets a wrapped line keep the space that ended it while a
    /// line that overshot the width gives its padding back.
    pub fn rstrip_end(&mut self, size: usize) {
        let length = self.cell_len();
        if length <= size {
            return;
        }
        let excess = length - size;
        let whitespace = self.plain.len() - self.plain.trim_end().len();
        if whitespace > 0 {
            self.right_crop(whitespace.min(excess));
        }
    }

    /// Replace tabs with spaces up to the next `tab_size` stop. Port of
    /// `Text.expand_tabs`.
    ///
    /// Styles extend over the inserted spaces, so a styled tab pads in its own
    /// style rather than punching an unstyled hole (upstream reaches the same
    /// result via `extend_style`).
    /// Append `count` spaces, extending any span that reached the end so the
    /// padding takes its style. Port of `Text.extend_style`.
    fn extend_style(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        let length = self.plain.len();
        self.plain.extend(std::iter::repeat_n(' ', count));
        for span in &mut self.spans {
            if span.end >= length {
                span.end += count;
            }
        }
    }

    pub fn expand_tabs(&mut self, tab_size: usize) {
        if !self.plain.contains('\t') || tab_size == 0 {
            return;
        }
        // Rebuilt part-by-part rather than by remapping offsets, because the
        // *split* is observable: upstream turns each tab-terminated run into its
        // own piece, so a span crossing several tabs comes back as several spans
        // and renders as several segments. Remapping offsets keeps one span and
        // emits one segment — same colours, different bytes.
        let mut result = Text::new("");
        for line in self.split("\n", true, false) {
            if !line.plain.contains('\t') {
                result = result.append_text(&line);
                continue;
            }
            let mut cell_position = 0usize;
            for mut part in line.split("\t", true, false) {
                if part.plain.ends_with('\t') {
                    // The tab becomes one space, then the run is padded out to
                    // the next stop — so a tab always advances at least one cell.
                    part.plain.pop();
                    part.plain.push(' ');
                    cell_position += part.cell_len();
                    let remainder = cell_position % tab_size;
                    if remainder != 0 {
                        let spaces = tab_size - remainder;
                        part.extend_style(spaces);
                        cell_position += spaces;
                    }
                } else {
                    cell_position += part.cell_len();
                }
                result = result.append_text(&part);
            }
        }
        self.plain = result.plain;
        self.spans = result.spans;
    }

    /// Join `lines` with this text as the separator, carrying each piece's base
    /// style across as a covering span. Port of `Text.join`.
    pub fn join(&self, lines: &[Text]) -> Text {
        let mut joined = self.blank_copy();
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.iter().enumerate() {
            joined = joined.append_text(line);
            if !self.plain.is_empty() && index != last {
                joined = joined.append_text(self);
            }
        }
        joined
    }

    /// Style every occurrence of any of `words`. Port of `Text.highlight_words`,
    /// returning the number of matches.
    pub fn highlight_words(
        &mut self,
        words: &[&str],
        style: impl Into<StyleType>,
        case_sensitive: bool,
    ) -> Result<usize> {
        let alternation = words
            .iter()
            .map(|word| fancy_regex::escape(word).into_owned())
            .collect::<Vec<_>>()
            .join("|");
        if alternation.is_empty() {
            return Ok(0);
        }
        let pattern = if case_sensitive {
            alternation
        } else {
            format!("(?i){alternation}")
        };
        self.highlight_regex(&pattern, Some(style.into()), "")
    }

    /// Style every match of `pattern`, returning the number of matches. Full port
    /// of `Text.highlight_regex`.
    ///
    /// `style`, when given, styles the whole match. Each **named group** is then
    /// styled with `{style_prefix}{name}` as a style *name*, left for the theme
    /// to resolve at render time — which is how a highlighter colours its groups
    /// without ever seeing a console.
    ///
    /// Groups that did not participate in the match, and zero-width ones, are
    /// skipped.
    pub fn highlight_regex(
        &mut self,
        pattern: &str,
        style: Option<StyleType>,
        style_prefix: &str,
    ) -> Result<usize> {
        let regex = fancy_regex::Regex::new(pattern)
            .map_err(|e| crate::errors::RichError::Regex(format!("invalid pattern: {e}")))?;
        Ok(self.highlight_with_regex(&regex, style, style_prefix))
    }

    /// As [`highlight_regex`](Self::highlight_regex) with an already-compiled
    /// pattern, for callers that apply the same patterns repeatedly.
    ///
    /// A match that errors mid-scan (a `fancy-regex` backtrack-limit hit) stops
    /// the scan and keeps the spans found so far, rather than discarding them.
    pub(crate) fn highlight_with_regex(
        &mut self,
        regex: &fancy_regex::Regex,
        style: Option<StyleType>,
        style_prefix: &str,
    ) -> usize {
        // Capture-definition order, matching upstream's `match.groupdict()`.
        let names: Vec<(usize, String)> = regex
            .capture_names()
            .enumerate()
            .filter_map(|(index, name)| name.map(|name| (index, name.to_string())))
            .collect();

        // Scanning borrows the plain string while the spans are pushed, so move
        // it out and put it back — no copy, and no fighting the borrow checker.
        let plain = std::mem::take(&mut self.plain);
        let mut count = 0;
        for captures in regex.captures_iter(&plain) {
            let Ok(captures) = captures else { break };
            if let (Some(style), Some(whole)) = (style.as_ref(), captures.get(0)) {
                if whole.end() > whole.start() {
                    self.spans.push(Span {
                        start: whole.start(),
                        end: whole.end(),
                        style: style.clone(),
                    });
                }
            }
            count += 1;
            for (index, name) in &names {
                if let Some(group) = captures.get(*index) {
                    if group.end() > group.start() {
                        self.spans.push(Span {
                            start: group.start(),
                            end: group.end(),
                            style: StyleType::Name(format!("{style_prefix}{name}")),
                        });
                    }
                }
            }
        }
        self.plain = plain;
        count
    }

    /// Build styled text from console markup. Port of `Text.from_markup`.
    ///
    /// Tag names are stored on the spans and resolved when the text is rendered,
    /// so no theme is needed here.
    pub fn from_markup(markup_text: &str) -> Result<Text> {
        markup::render(markup_text)
    }

    /// The unstyled string content.
    pub fn plain(&self) -> &str {
        &self.plain
    }

    /// The spans currently applied.
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Length in terminal cells.
    pub fn cell_len(&self) -> usize {
        cell_len(&self.plain)
    }

    /// True when there is no content.
    pub fn is_empty(&self) -> bool {
        self.plain.is_empty()
    }

    /// Append more text, optionally under `style` (a resolved [`Style`] or a
    /// style name).
    pub fn append(&mut self, text: &str, style: Option<StyleType>) {
        let start = self.plain.len();
        // Strip here as well as in `new`: upstream's `Text.append` runs the same
        // `strip_control_codes`, and skipping it let BEL, backspace, vertical
        // tab and form feed reach the terminal through every path that builds
        // text incrementally — Markdown, Syntax and plain files. A backspace run
        // is a spoofing tool: `FAILED\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}PASSED`
        // displays as `PASSED`.
        self.plain.push_str(&Text::strip_control_codes(text));
        let end = self.plain.len();
        if let Some(style) = style {
            self.spans.push(Span { start, end, style });
        }
    }

    /// Append another `Text`, carrying over its base style (as a covering span)
    /// and all of its spans, shifted to their new offsets. Port of
    /// `Text.append_text`. Consumes `self` and returns it for chaining.
    pub fn append_text(mut self, other: &Text) -> Text {
        let offset = self.plain.len();
        self.plain.push_str(&other.plain);
        let end = self.plain.len();
        if !other.style.is_null_style() {
            self.spans.push(Span {
                start: offset,
                end,
                style: other.style.clone(),
            });
        }
        for span in &other.spans {
            self.spans.push(Span {
                start: span.start + offset,
                end: span.end + offset,
                style: span.style.clone(),
            });
        }
        self
    }

    /// Apply `style` to the byte range `[start, end)`. Port of `Text.stylize`,
    /// including its argument order.
    ///
    /// `style` may be a resolved [`Style`] or a name (`"repr.number"`) left for
    /// the renderer to look up. Byte offsets, not char offsets; ASCII-only
    /// callers such as highlighters are unaffected by the distinction.
    ///
    /// A range that is empty or inverted is ignored, which is what gives us
    /// upstream's `end > start` skip for non-participating regex groups.
    pub fn stylize(&mut self, style: impl Into<StyleType>, start: usize, end: usize) {
        let end = end.min(self.plain.len());
        if start >= end {
            return;
        }
        self.spans.push(Span {
            start,
            end,
            style: style.into(),
        });
    }

    /// Push a raw span (used by the markup parser).
    pub(crate) fn push_span(&mut self, span: Span) {
        self.spans.push(span);
    }

    /// Set the whole-text base style, resolved or named.
    pub fn set_base_style(&mut self, style: impl Into<StyleType>) {
        self.style = style.into();
    }

    /// Flatten into non-overlapping segments (newlines become [`Segment::line`]),
    /// combining `base_style`, this text's base style, and every covering span.
    /// Does **not** wrap. Port of the core of `Text.render`.
    ///
    /// Named span styles are resolved against `theme`.
    pub fn render(&self, theme: &Theme, base_style: &Style) -> Vec<Segment> {
        self.render_joined(theme, base_style, None)
    }

    /// The `(minimum, maximum)` cell width of this text: `maximum` is the widest
    /// hard line, `minimum` the widest word. Port of `Text.__rich_measure__`.
    pub fn measurement(&self) -> (usize, usize) {
        // Measured against the tab-EXPANDED text. Upstream measures the raw
        // string, where a tab counts as zero cells, and gets away with it
        // because its measurement never narrows the render width. Ours does, so
        // measuring raw would hand the renderer a width smaller than the text it
        // is about to expand and `"a\tb\tc"` would wrap on nothing.
        let expanded;
        let plain = if self.plain.contains('\t') {
            let mut text = self.clone();
            text.expand_tabs(DEFAULT_TAB_SIZE);
            expanded = text.plain;
            &expanded
        } else {
            &self.plain
        };
        let max_line = plain.split('\n').map(cell_len).max().unwrap_or(0);
        let min_word = plain
            .split_whitespace()
            .map(cell_len)
            .max()
            .unwrap_or(max_line);
        (min_word, max_line)
    }

    /// Render into visual lines, wrapping each hard line to `width` cells when
    /// `Some`, and justifying per this text's own justify.
    pub fn render_lines(
        &self,
        theme: &Theme,
        base_style: &Style,
        width: Option<usize>,
    ) -> Vec<Vec<Segment>> {
        self.render_lines_justified(theme, base_style, width, self.justify)
    }

    /// Like [`render_lines`](Self::render_lines) but with an explicit `justify`
    /// (used by the console to apply `options.justify`).
    pub fn render_lines_justified(
        &self,
        theme: &Theme,
        base_style: &Style,
        width: Option<usize>,
        justify: Justify,
    ) -> Vec<Vec<Segment>> {
        self.render_lines_wrapped(
            theme,
            base_style,
            width,
            justify,
            self.overflow.unwrap_or(Overflow::Fold),
            self.no_wrap.unwrap_or(false),
        )
    }

    /// The full wrap-justify-truncate pipeline, with every knob resolved by the
    /// caller. Port of `Text.wrap`.
    ///
    /// Lines are split on `\n`, wrapped to `width` (folding over-long words only
    /// when `overflow` is [`Overflow::Fold`]), justified, and finally truncated
    /// to `width`. [`Overflow::Ignore`] skips wrapping and truncation both, so
    /// lines may come back wider than `width`.
    pub fn render_lines_wrapped(
        &self,
        theme: &Theme,
        base_style: &Style,
        width: Option<usize>,
        justify: Justify,
        overflow: Overflow,
        no_wrap: bool,
    ) -> Vec<Vec<Segment>> {
        // Tabs are expanded before anything measures or wraps the text, as
        // upstream's `Text.wrap` does per line. Without this a tab occupies one
        // cell everywhere in the layout and then eight on the terminal, so every
        // width calculation downstream is wrong.
        if self.plain.contains('\t') {
            let mut expanded = self.clone();
            expanded.expand_tabs(DEFAULT_TAB_SIZE);
            return expanded
                .render_lines_wrapped(theme, base_style, width, justify, overflow, no_wrap);
        }

        // Resolve every span's style once, up front, into a vector parallel to
        // `self.spans` — upstream's `style_map`. Resolving inside the per-line
        // loop would re-parse the same names for every visual line.
        let resolved: Vec<Style> = self
            .spans
            .iter()
            .map(|span| theme.get_style_or_null(&span.style))
            .collect();
        let effective_base = base_style.combine(&theme.get_style_or_null(&self.style));
        // Upstream folds `overflow == "ignore"` into no_wrap before splitting.
        let no_wrap = no_wrap || overflow == Overflow::Ignore;
        let mut lines: Vec<Vec<Segment>> = Vec::new();
        for (start, end) in self.wrapped_ranges(width, overflow, no_wrap) {
            lines.push(self.line_segments(&resolved, start, end, &effective_base));
        }
        let Some(width) = width else {
            return lines;
        };
        if justify != Justify::Default {
            let last = lines.len().saturating_sub(1);
            for (index, line) in lines.iter_mut().enumerate() {
                // Full justification leaves the final line ragged, so it
                // needs to know where it is in the paragraph.
                *line = justify_line(line, width, justify, &effective_base, index == last);
            }
        }
        if overflow != Overflow::Ignore {
            for line in &mut lines {
                *line = truncate_line(line, width, overflow);
            }
        }
        lines
    }

    /// As [`render_lines_wrapped`](Self::render_lines_wrapped), flattened into a
    /// single segment stream with [`Segment::line`] between visual lines.
    pub fn render_joined_wrapped(
        &self,
        theme: &Theme,
        base_style: &Style,
        width: usize,
        justify: Justify,
        overflow: Overflow,
        no_wrap: bool,
    ) -> Vec<Segment> {
        let lines =
            self.render_lines_wrapped(theme, base_style, Some(width), justify, overflow, no_wrap);
        let mut segments = Vec::new();
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.into_iter().enumerate() {
            segments.extend(line);
            if index != last {
                segments.push(Segment::line());
            }
        }
        segments
    }

    /// Render into a flat segment stream with [`Segment::line`] between visual
    /// lines (wrapping when `width` is `Some`), using this text's own justify.
    fn render_joined(
        &self,
        theme: &Theme,
        base_style: &Style,
        width: Option<usize>,
    ) -> Vec<Segment> {
        let lines = self.render_lines(theme, base_style, width);
        let mut segments = Vec::new();
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.into_iter().enumerate() {
            segments.extend(line);
            if index != last {
                segments.push(Segment::line());
            }
        }
        segments
    }

    /// The `(start_byte, end_byte)` range of each visual line: hard lines split
    /// on `\n`, then wrapped to `width` cells when `Some`.
    fn wrapped_ranges(
        &self,
        width: Option<usize>,
        overflow: Overflow,
        no_wrap: bool,
    ) -> Vec<(usize, usize)> {
        let mut hard: Vec<(usize, usize)> = Vec::new();
        let mut start = 0;
        for (i, byte) in self.plain.bytes().enumerate() {
            if byte == b'\n' {
                hard.push((start, i));
                start = i + 1;
            }
        }
        hard.push((start, self.plain.len()));

        let Some(width) = width else {
            return hard;
        };
        if no_wrap {
            return hard;
        }

        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for (a, b) in hard {
            let sub = &self.plain[a..b];
            // Only `fold` breaks a word that is wider than the whole line; the
            // cropping methods leave it long and let truncation cut it.
            let breaks = crate::wrap::divide_line(sub, width, overflow == Overflow::Fold);
            let mut cuts = vec![a];
            for char_offset in breaks {
                cuts.push(a + char_to_byte(sub, char_offset));
            }
            cuts.push(b);
            for window in cuts.windows(2) {
                ranges.push((window[0], window[1]));
            }
        }
        ranges
    }

    /// Combine `effective_base` with every span covering `[start, end)`,
    /// producing non-overlapping segments for that byte range.
    ///
    /// `resolved` is the per-render style map, index-parallel to `self.spans`.
    /// Spans are folded in vector order, and spans that resolved to nothing are
    /// **not** skipped — they still contribute a boundary. Upstream behaves the
    /// same way, and the highlighter fixtures depend on it: an ISO-8601 date
    /// emits separate segments per sub-field even where the field styles are
    /// identical.
    fn line_segments(
        &self,
        resolved: &[Style],
        start: usize,
        end: usize,
        effective_base: &Style,
    ) -> Vec<Segment> {
        if start >= end {
            return Vec::new();
        }
        let mut points: Vec<usize> = vec![start, end];
        for span in &self.spans {
            let span_start = span.start.clamp(start, end);
            let span_end = span.end.clamp(start, end);
            points.push(span_start);
            points.push(span_end);
        }
        points.sort_unstable();
        points.dedup();

        let mut segments = Vec::new();
        for window in points.windows(2) {
            let (a, b) = (window[0], window[1]);
            if a >= b {
                continue;
            }
            let slice = &self.plain[a..b];
            if slice.is_empty() {
                continue;
            }
            let mut style = effective_base.clone();
            for (span, span_style) in self.spans.iter().zip(resolved) {
                if span.start <= a && span.end >= b {
                    style = style.combine(span_style);
                }
            }
            segments.push(Segment::new(slice, Some(style)));
        }
        segments
    }
}

/// Byte offset of the `char_idx`-th char in `text` (clamped to `text.len()`).
fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

/// Cut a rendered line down to `width` cells, applying `overflow`. Segment-level
/// counterpart of [`Text::truncate`], used once per line at the end of the wrap
/// pipeline.
///
/// [`Overflow::Fold`] and [`Overflow::Crop`] both plain-cut: by this point the
/// line has already been wrapped, so anything still over-long is an unbreakable
/// run that folding cannot help with.
///
/// [`Overflow::Ellipsis`] cuts one cell short and appends `…`. The marker takes
/// the style of the first segment the cut did *not* keep whole — upstream writes
/// the ellipsis into the plain string and lets span-trimming decide, which works
/// out to the same rule, including when the cut lands exactly on a boundary.
fn truncate_line(line: &[Segment], width: usize, overflow: Overflow) -> Vec<Segment> {
    if overflow == Overflow::Ignore {
        return line.to_vec();
    }
    let total: usize = line.iter().map(Segment::cell_length).sum();
    if total <= width {
        return line.to_vec();
    }
    let ellipsis = overflow == Overflow::Ellipsis;
    // `…` occupies one cell, so the kept text must stop one cell early.
    let keep = if ellipsis {
        width.saturating_sub(1)
    } else {
        width
    };

    let mut result: Vec<Segment> = Vec::new();
    let mut used = 0usize;
    // Style the ellipsis inherits: that of the first segment reaching past the
    // cut. `total > width >= keep` guarantees such a segment exists.
    let mut cut_style: Option<Style> = None;
    for segment in line {
        let length = segment.cell_length();
        if used + length <= keep {
            result.push(segment.clone());
            used += length;
            continue;
        }
        cut_style = segment.style.clone();
        if used < keep {
            // This segment straddles the cut, so keep the part that fits. A wide
            // character landing across the boundary is dropped whole and
            // `set_cell_size` pads the gap with a space, as upstream does.
            result.push(Segment::new(
                set_cell_size(&segment.text, keep - used),
                segment.style.clone(),
            ));
        }
        break;
    }
    if ellipsis {
        // Upstream appends the marker to the plain string and re-renders, so it
        // lands inside the preceding run rather than beside it. Merging keeps
        // the byte stream identical — a separate segment would re-emit the style.
        match result.last_mut() {
            Some(last) if !last.control && last.style == cut_style => last.text.push('…'),
            _ => result.push(Segment::new("…", cut_style)),
        }
    }
    result
}

/// Split a rendered line into whitespace-separated words, each word keeping its
/// own styled segments. Separator spaces are dropped — [`full_justify`] decides
/// the new gaps. Port of the `line.split(" ")` in upstream's `full` branch.
fn split_words(line: &[Segment]) -> Vec<Vec<Segment>> {
    let mut words: Vec<Vec<Segment>> = Vec::new();
    let mut current: Vec<Segment> = Vec::new();
    for segment in line {
        // A segment can straddle a space, so split within it and keep the style.
        for (index, piece) in segment.text.split(' ').enumerate() {
            if index > 0 {
                words.push(std::mem::take(&mut current));
            }
            if !piece.is_empty() {
                current.push(Segment::new(piece, segment.style.clone()));
            }
        }
    }
    words.push(current);
    // Wrapping leaves a trailing space on every line but the last, so the naive
    // split ends with an empty word. Upstream's `Text.split` drops it, and the
    // count matters: it decides how many gaps share the slack.
    if words.last().is_some_and(|w| w.is_empty()) {
        words.pop();
    }
    words
}

/// Distribute `width` across `line`'s words by widening the gaps between them.
/// Direct port of the `justify == "full"` branch of upstream's `Lines.justify`:
/// every gap starts at one space, and the extra columns are handed out from the
/// rightmost gap backwards, cycling.
fn full_justify(line: &[Segment], width: usize, style: &Style) -> Vec<Segment> {
    let words = split_words(line);
    let words_size: usize = words
        .iter()
        .map(|word| word.iter().map(Segment::cell_length).sum::<usize>())
        .sum();
    let mut num_spaces = words.len().saturating_sub(1);
    let mut spaces = vec![1usize; num_spaces];
    if !spaces.is_empty() {
        let mut index = 0;
        while words_size + num_spaces < width {
            let slot = spaces.len() - index - 1;
            spaces[slot] += 1;
            num_spaces += 1;
            index = (index + 1) % spaces.len();
        }
    }

    let mut out: Vec<Segment> = Vec::new();
    for (index, word) in words.iter().enumerate() {
        out.extend(word.iter().cloned());
        if let Some(&gap) = spaces.get(index) {
            // Upstream styles the gap with the surrounding style when the two
            // neighbours agree, else with the line's base style.
            let before = word.last().and_then(|s| s.style.clone());
            let after = words
                .get(index + 1)
                .and_then(|w| w.first())
                .and_then(|s| s.style.clone());
            let gap_style = if before == after {
                before.unwrap_or_else(|| style.clone())
            } else {
                style.clone()
            };
            out.push(Segment::new(" ".repeat(gap), Some(gap_style)));
        }
    }
    out
}

/// Pad `line` to `width` cells according to `justify`, using `style` for the
/// pad (so e.g. a styled table cell fills with its own style).
///
/// `is_last` marks the final line of the paragraph, which full justification
/// leaves ragged rather than stretching.
fn justify_line(
    line: &[Segment],
    width: usize,
    justify: Justify,
    style: &Style,
    is_last: bool,
) -> Vec<Segment> {
    // Full justification rewrites the interior gaps instead of padding an edge.
    if justify == Justify::Full {
        // Upstream `break`s before the final line, so it is left exactly as
        // wrapped — not even padded out to the width, unlike every other mode.
        return if is_last {
            line.to_vec()
        } else {
            full_justify(line, width, style)
        };
    }
    let line_width: usize = line.iter().map(Segment::cell_length).sum();
    let excess = width.saturating_sub(line_width);
    let (left, right) = match justify {
        Justify::Right => (excess, 0),
        Justify::Center => (excess / 2, excess - excess / 2),
        // Left, Default, and full justification's ragged last line pad right.
        Justify::Left | Justify::Full | Justify::Default => (0, excess),
    };
    let mut out = Vec::with_capacity(line.len() + 2);
    if left > 0 {
        out.push(Segment::new(" ".repeat(left), Some(style.clone())));
    }
    out.extend(line.iter().cloned());
    if right > 0 {
        out.push(Segment::new(" ".repeat(right), Some(style.clone())));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Full justification widens the gaps between words so every line but the
    /// last fills the width exactly.
    ///
    /// Captured verbatim from real rich 15.0.0 —
    /// `Lines.justify(console, 20, justify="full")` on
    /// `"aaa bbb ccc ddddddddddddddddddd ee ff"` yields:
    ///
    /// ```text
    /// 'aaa     bbb      ccc'   <- stretched to exactly 20
    /// 'ddddddddddddddddddd'    <- one word: nothing to widen, and the
    ///                             trailing space wrapping left is dropped
    /// 'ee ff'                  <- final line untouched: NOT padded to width
    /// ```
    ///
    /// Two details worth pinning: the slack is handed out from the rightmost
    /// gap backwards (so the gaps are 5 then 6, not 6 then 5), and the last
    /// line is the one case where a justified line is left short of the width.
    #[test]
    fn full_justify_matches_upstream() {
        let text = Text::new("aaa bbb ccc ddddddddddddddddddd ee ff").justify(Justify::Full);
        let plain: Vec<String> = text
            .render_lines(&Theme::default_theme(), &Style::new(), Some(20))
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect())
            .collect();
        assert_eq!(
            plain,
            vec!["aaa     bbb      ccc", "ddddddddddddddddddd", "ee ff"]
        );
        assert_eq!(plain[0].chars().count(), 20);
    }

    #[test]
    fn append_creates_spans() {
        let mut text = Text::new("");
        text.append("hello", Some(Style::parse("bold").unwrap().into()));
        text.append(" world", None);
        assert_eq!(text.plain(), "hello world");
        assert_eq!(text.spans().len(), 1);
    }

    #[test]
    fn render_flattens_overlapping_spans() {
        let mut text = Text::new("abcdef");
        text.stylize(Style::parse("bold").unwrap(), 0, 4);
        text.stylize(Style::parse("red").unwrap(), 2, 6);
        let segments = text.render(&Theme::default_theme(), &Style::new());
        // Boundaries at 0,2,4,6 -> "ab"(bold) "cd"(bold+red) "ef"(red)
        let rendered: Vec<_> = segments.iter().map(|s| s.text.clone()).collect();
        assert_eq!(rendered, vec!["ab", "cd", "ef"]);
    }

    /// `Text::truncate` on its own, against real rich 15.0.0. `fold` and `crop`
    /// deliberately agree: folding is a wrapping behaviour, and truncation has
    /// no line to fold onto.
    #[test]
    fn truncate_matches_upstream() {
        for (overflow, expected) in [
            (Overflow::Fold, "hello"),
            (Overflow::Crop, "hello"),
            (Overflow::Ellipsis, "hell…"),
            (Overflow::Ignore, "hello world"),
        ] {
            let mut text = Text::new("hello world");
            text.truncate(5, Some(overflow), false);
            assert_eq!(text.plain(), expected, "overflow {overflow:?}");
        }
    }

    /// `pad` fills out to the width, but only when the text is short — a text
    /// that is already too long is cut, never padded.
    #[test]
    fn truncate_pads_only_when_short() {
        let mut short = Text::new("hi");
        short.truncate(6, Some(Overflow::Crop), true);
        assert_eq!(short.plain(), "hi    ");

        let mut exact = Text::new("hi");
        exact.truncate(2, Some(Overflow::Crop), true);
        assert_eq!(exact.plain(), "hi");
    }

    /// Truncating must not leave a span pointing past the end of the string.
    #[test]
    fn truncate_trims_dangling_spans() {
        let mut text = Text::new("hello world");
        text.stylize(Style::parse("bold").unwrap(), 6, 11);
        text.stylize(Style::parse("red").unwrap(), 0, 5);
        text.truncate(3, Some(Overflow::Crop), false);
        assert_eq!(text.plain(), "hel");
        // The "world" span starts past the new end and is dropped entirely; the
        // "hello" span survives, clamped.
        assert_eq!(text.spans().len(), 1);
        assert!(text.spans().iter().all(|s| s.end <= text.plain().len()));
    }

    use crate::protocol::Renderable;

    /// The overflow method may come from the text or from the console options,
    /// and the text's own setting wins — mirroring upstream's
    /// `self.overflow or options.overflow or DEFAULT_OVERFLOW`.
    #[test]
    fn text_overflow_beats_console_options() {
        let console = crate::Console::builder().width(8).build();
        let mut options = console.options();
        options.overflow = Some(Overflow::Ellipsis);
        options.no_wrap = Some(true);

        // Nothing set on the text: the options decide.
        let from_options = Text::new("the quick brown fox");
        assert_eq!(
            plain_of(&from_options.rich_render(&console, &options)),
            "the qui…"
        );

        // Set on the text: the text decides, and the options are ignored.
        let from_text = Text::new("the quick brown fox").overflow(Overflow::Crop);
        assert_eq!(
            plain_of(&from_text.rich_render(&console, &options)),
            "the quic"
        );
    }

    /// With no overflow anywhere, upstream's default applies: fold.
    #[test]
    fn overflow_defaults_to_fold() {
        let console = crate::Console::builder().width(8).build();
        let text = Text::new("supercalifragilistic");
        let rendered = plain_of(&text.rich_render(&console, &console.options()));
        assert_eq!(rendered, "supercal\nifragili\nstic");
    }

    /// Concatenate the visible text of a segment stream, for assertions that
    /// care about layout rather than styling.
    fn plain_of(segments: &[Segment]) -> String {
        segments
            .iter()
            .filter(|s| !s.control)
            .map(|s| s.text.as_str())
            .collect()
    }

    /// `Text::new` stripped control codes but `append` did not, so every path
    /// that builds text incrementally — Markdown, Syntax, plain files — leaked
    /// them to the terminal. A backspace run is a spoofing tool: the reader sees
    /// the overwritten text, not what the file says.
    #[test]
    fn append_strips_control_codes_like_new() {
        let mut text = Text::new("");
        text.append("FAILED\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}PASSED", None);
        assert_eq!(text.plain(), "FAILEDPASSED");

        for code in ['\u{7}', '\u{8}', '\u{b}', '\u{c}', '\u{d}'] {
            let mut text = Text::new("");
            text.append(&format!("a{code}b"), None);
            assert_eq!(text.plain(), "ab", "control code {code:?} survived append");
        }
    }

    /// Upstream's `strip_control_codes` keeps NUL and ESC; only BEL, backspace,
    /// vertical tab, form feed and carriage return go.
    #[test]
    fn append_keeps_the_codes_upstream_keeps() {
        let mut text = Text::new("");
        text.append("a\u{0}b\u{1b}c", None);
        assert_eq!(text.plain(), "a\u{0}b\u{1b}c");
    }
}
