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

/// Cell width of a tab stop: upstream's `Console.tab_size` default, and the
/// fallback when neither a text nor its console sets one (`tab_size or 8`).
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
    /// How lines are justified within the render width. `None` defers to
    /// the console options (upstream's `justify=None`); `Some(Default)` is an
    /// explicit `justify="default"`, which a container's justify does not
    /// override.
    justify: Option<Justify>,
    /// What to do with lines wider than the render width. `None` defers to the
    /// console options, then to [`Overflow::Fold`].
    overflow: Option<Overflow>,
    /// Whether to skip wrapping. `None` defers to the console options, then to
    /// `false`.
    no_wrap: Option<bool>,
    /// Tab stop width. `None` defers to the console's `tab_size` when
    /// rendered, then to [`DEFAULT_TAB_SIZE`]. Upstream's `Text.tab_size`.
    tab_size: Option<usize>,
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
            justify: None,
            overflow: None,
            no_wrap: None,
            tab_size: None,
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
            justify: None,
            overflow: None,
            no_wrap: None,
            tab_size: None,
        }
    }

    /// Set how lines are justified within the render width (builder form).
    pub fn justify(mut self, justify: Justify) -> Self {
        self.set_justify(justify);
        self
    }

    /// Set how lines are justified within the render width.
    ///
    /// [`Justify::Default`] means *unset* here (upstream's `justify=None`),
    /// deferring to the console options; use
    /// [`set_justify_option`](Self::set_justify_option) for an explicit
    /// `justify="default"`.
    pub fn set_justify(&mut self, justify: Justify) {
        self.justify = (justify != Justify::Default).then_some(justify);
    }

    /// This text's own justify method ([`Justify::Default`] when unset).
    pub fn get_justify(&self) -> Justify {
        self.justify.unwrap_or_default()
    }

    /// Set this text's justify, distinguishing an explicit
    /// `Some(Justify::Default)` (upstream's `justify="default"`, which a
    /// table column's or the print call's justify does not override) from
    /// `None` (upstream's `justify=None`, which defers to them).
    pub fn set_justify_option(&mut self, justify: Option<Justify>) {
        self.justify = justify;
    }

    /// This text's own justify, `None` when unset. See
    /// [`set_justify_option`](Self::set_justify_option).
    pub fn get_justify_option(&self) -> Option<Justify> {
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

    /// Set the tab stop width (builder form). Port of `Text(tab_size=…)`.
    pub fn tab_size(mut self, tab_size: usize) -> Self {
        self.tab_size = Some(tab_size);
        self
    }

    /// Set the tab stop width. Pass `None` to defer to the console's
    /// `tab_size`. Upstream's `Text.tab_size` attribute.
    pub fn set_tab_size(&mut self, tab_size: Option<usize>) {
        self.tab_size = tab_size;
    }

    /// This text's own tab stop width, if it set one.
    pub fn get_tab_size(&self) -> Option<usize> {
        self.tab_size
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
        // Upstream spans use character offsets. A truncation may replace a
        // wide glyph with padding or ASCII with a multibyte ellipsis, so byte
        // offsets alone cannot be clamped into the replacement string.
        if !self.spans.is_empty()
            && !self.plain.starts_with(&plain)
            && !plain.starts_with(&self.plain)
        {
            let old_offsets: Vec<usize> = self
                .plain
                .char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(self.plain.len()))
                .collect();
            let new_offsets: Vec<usize> = plain
                .char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(plain.len()))
                .collect();
            let map_offset = |offset: usize| {
                let character = old_offsets.partition_point(|old| *old < offset);
                new_offsets[character.min(new_offsets.len() - 1)]
            };
            for span in &mut self.spans {
                span.start = map_offset(span.start);
                span.end = map_offset(span.end);
            }
        }
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
            tab_size: self.tab_size,
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

        // Upstream binary-searches for the first and last line a span touches
        // rather than visiting every line for every span, which made
        // splitting a highlighted file quadratic in its line count. Offsets
        // from `split` are sorted; for anything else keep the full scan, whose
        // result the search would not reproduce.
        let sorted = bounds.windows(2).all(|w| w[0] <= w[1]);
        for span in &self.spans {
            let lines_touched = if sorted {
                // Lines ending at or before the span starts cannot hold it,
                // nor can lines starting at or after it ends.
                let first = bounds[1..].partition_point(|&end| end <= span.start);
                let last = bounds[..bounds.len() - 1].partition_point(|&start| start < span.end);
                first..last.max(first)
            } else {
                0..bounds.len() - 1
            };
            for index in lines_touched {
                let (line_start, line_end) = (bounds[index], bounds[index + 1]);
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
        // Python's `str.rstrip()` whitespace, which includes U+001C..U+001F.
        let plain = self.plain.trim_end_matches(is_python_space).to_string();
        self.set_plain(plain);
    }

    /// Remove *only as much* trailing whitespace as it takes to get down to
    /// `size` characters, leaving the rest. Port of `Text.rstrip_end`.
    ///
    /// This is what lets a wrapped line keep the space that ended it while a
    /// line that overshot the width gives its padding back. As upstream, the
    /// length is `len(self)` — characters, not cells.
    pub fn rstrip_end(&mut self, size: usize) {
        let length = self.plain.chars().count();
        if length <= size {
            return;
        }
        let excess = length - size;
        let trimmed = self.plain.trim_end_matches(is_python_space);
        let whitespace = self.plain[trimmed.len()..].chars().count();
        if whitespace > 0 {
            let keep = length - whitespace.min(excess);
            let bytes = self.plain.len() - char_to_byte(&self.plain, keep);
            self.right_crop(bytes);
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

    /// A `Text` from a string containing ANSI escape codes, one decoded line
    /// per line joined by newlines, with base `style`. Port of
    /// `Text.from_ansi` (its `tab_size` defaults to 8; set `justify`,
    /// `overflow` and `no_wrap` with the builders).
    pub fn from_ansi(text: &str, style: impl Into<StyleType>) -> Text {
        let mut joiner = Text::styled("\n", style);
        joiner.tab_size = Some(DEFAULT_TAB_SIZE);
        let lines = crate::ansi::AnsiDecoder::new().decode_split_newlines(text);
        joiner.join(&lines)
    }

    /// The indentation unit of the text: the greatest common divisor of its
    /// even line indents, or 1. Port of `Text.detect_indentation`.
    pub fn detect_indentation(&self) -> usize {
        fn gcd(a: usize, b: usize) -> usize {
            if b == 0 {
                a
            } else {
                gcd(b, a % b)
            }
        }
        let mut indents: Vec<usize> = self
            .plain
            .split('\n')
            .map(|line| line.len() - line.trim_start_matches(' ').len())
            .filter(|indent| indent % 2 == 0)
            .collect();
        indents.sort_unstable();
        indents.dedup();
        match indents.into_iter().reduce(gcd) {
            None | Some(0) => 1,
            Some(indentation) => indentation,
        }
    }

    /// A copy with indent guides drawn in each line's leading spaces: every
    /// `indent_size` spaces (auto-detected for `None`) become `character`
    /// then padding, styled `style`. Port of `Text.with_indent_guides`
    /// (upstream's defaults are `"│"` and `"dim green"`).
    pub fn with_indent_guides(
        &self,
        indent_size: Option<usize>,
        character: &str,
        style: impl Into<StyleType>,
    ) -> Text {
        let style = style.into();
        let indent_size = indent_size.unwrap_or_else(|| self.detect_indentation());
        let mut text = self.clone();
        text.expand_tabs(match self.tab_size {
            None => DEFAULT_TAB_SIZE,
            Some(tab_size) => tab_size,
        });
        let indent_line = format!("{character}{}", " ".repeat(indent_size.saturating_sub(1)));
        let mut new_lines: Vec<Text> = Vec::new();
        let mut blank_lines = 0usize;
        for mut line in text.split("\n", false, true) {
            let indent = line.plain.len() - line.plain.trim_start_matches(' ').len();
            // `re.match(r"^( *)(.*)$")`: `.` stops at a line break, so a
            // line with nothing after its indent (or a `\r`) counts as blank.
            let rest = &line.plain[indent..];
            if rest.is_empty() || rest.starts_with('\r') {
                blank_lines += 1;
                continue;
            }
            let (full_indents, remaining_space) = (
                indent.checked_div(indent_size).unwrap_or(0),
                indent.checked_rem(indent_size).unwrap_or(indent),
            );
            let new_indent = format!(
                "{}{}",
                indent_line.repeat(full_indents),
                " ".repeat(remaining_space)
            );
            // `line.plain = new_indent + line.plain[len(new_indent):]`: the
            // same number of characters, so upstream's (character) spans stay
            // put; here the byte offsets past the indent shift.
            let old_bytes = char_to_byte(&line.plain, new_indent.chars().count());
            let plain = format!("{new_indent}{}", &line.plain[old_bytes..]);
            let remap = |position: usize| {
                if position >= old_bytes {
                    position - old_bytes + new_indent.len()
                } else {
                    char_to_byte(&new_indent, position)
                }
            };
            for span in &mut line.spans {
                span.start = remap(span.start);
                span.end = remap(span.end);
            }
            line.plain = plain;
            line.stylize(style.clone(), 0, new_indent.len());
            for _ in 0..std::mem::take(&mut blank_lines) {
                new_lines.push(Text::styled(new_indent.clone(), style.clone()));
            }
            new_lines.push(line);
        }
        for _ in 0..blank_lines {
            new_lines.push(Text::styled("", style.clone()));
        }
        let mut separator = text.blank_copy();
        separator.plain.push('\n');
        separator.join(&new_lines)
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
        // `if style:` — a null style or an empty name adds no span, so it
        // leaves no segment boundary behind either.
        if let Some(style) = style.filter(|style| !is_falsy_style(style)) {
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
        let style = style.into();
        let end = end.min(self.plain.len());
        // `if style:` — a falsy style adds no span.
        if start >= end || is_falsy_style(&style) {
            return;
        }
        self.spans.push(Span { start, end, style });
    }

    /// As [`stylize`](Self::stylize), but the span goes *first*, beneath every
    /// existing span. Port of `Text.stylize_before`.
    pub fn stylize_before(&mut self, style: impl Into<StyleType>, start: usize, end: usize) {
        let style = style.into();
        let end = end.min(self.plain.len());
        if start >= end || is_falsy_style(&style) {
            return;
        }
        self.spans.insert(0, Span { start, end, style });
    }

    /// Apply metadata to the byte range `start..end` (the rest of the text
    /// when `end` is `None`). Port of `Text.apply_meta`: a span of
    /// `Style.from_meta(meta)`, which an empty map (a null style) skips.
    pub fn apply_meta(&mut self, meta: crate::style::Meta, start: usize, end: Option<usize>) {
        let end = end.unwrap_or(self.plain.len());
        self.stylize(Style::from_meta(meta), start, end);
    }

    /// Apply event-handler metadata to the whole text. Port of `Text.on`:
    /// handlers are stored as `"@name"` keys over `meta`.
    pub fn on(
        &mut self,
        meta: Option<crate::style::Meta>,
        handlers: &[(&str, crate::style::MetaValue)],
    ) -> &mut Self {
        let len = self.plain.len();
        self.stylize(Style::on(meta, handlers), 0, len);
        self
    }

    /// Append a raw span, as upstream's `text.spans.append(span)` does: no
    /// clamping and no falsy-style check.
    ///
    /// Offsets are **byte** offsets into [`plain`](Self::plain) and must fall
    /// on `char` boundaries (upstream's are character offsets; convert first).
    /// A span may extend past the end of the text, as upstream allows.
    pub fn push_span(&mut self, span: Span) {
        self.spans.push(span);
    }

    /// The spans, mutably. Upstream's `Text.spans` is a public, mutable list;
    /// the same byte-offset rules as [`push_span`](Self::push_span) apply.
    pub fn spans_mut(&mut self) -> &mut Vec<Span> {
        &mut self.spans
    }

    /// Replace every span. Port of the `Text.spans` setter.
    pub fn set_spans(&mut self, spans: Vec<Span>) {
        self.spans = spans;
    }

    /// Drop this text's own `justify`, `overflow` and `no_wrap`, so they defer to
    /// the console options as upstream's `Text.join` result does.
    pub(crate) fn clear_layout_options(&mut self) {
        self.justify = None;
        self.overflow = None;
        self.no_wrap = None;
        self.tab_size = None;
    }

    /// Move the base style into a span over the whole text, ahead of the
    /// existing spans — what `Text.join` does to each joined text. A falsy
    /// style (null, or an empty name) adds no span, as `if text.style:` skips
    /// it upstream.
    pub(crate) fn base_style_to_span(&mut self) {
        let style = std::mem::take(&mut self.style);
        let falsy =
            style.is_null_style() || matches!(&style, StyleType::Name(name) if name.is_empty());
        if !falsy {
            self.spans.insert(
                0,
                Span {
                    start: 0,
                    end: self.plain.len(),
                    style,
                },
            );
        }
    }

    /// The whole-text base style, resolved or named. Port of the `Text.style`
    /// attribute.
    pub fn base_style(&self) -> &StyleType {
        &self.style
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
        // Measured against the raw string, as upstream does: `cell_len` counts
        // a tab as zero cells, and tabs expand only at render time. A printed
        // `Text` renders at the full width (see `Renderable::printed_text`), so
        // this measurement never becomes its own render width (#447).
        measure_plain(&self.plain)
    }

    /// Render into visual lines, wrapping each hard line to `width` cells when
    /// `Some`, and justifying per this text's own justify.
    pub fn render_lines(
        &self,
        theme: &Theme,
        base_style: &Style,
        width: Option<usize>,
    ) -> Vec<Vec<Segment>> {
        self.render_lines_justified(theme, base_style, width, self.get_justify())
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
    ///
    /// Tabs expand to this text's own [`tab_size`](Self::get_tab_size), else
    /// [`DEFAULT_TAB_SIZE`]; [`render_lines_wrapped_tabs`](Self::render_lines_wrapped_tabs)
    /// takes the width explicitly (upstream's `wrap(tab_size=…)`).
    pub fn render_lines_wrapped(
        &self,
        theme: &Theme,
        base_style: &Style,
        width: Option<usize>,
        justify: Justify,
        overflow: Overflow,
        no_wrap: bool,
    ) -> Vec<Vec<Segment>> {
        self.render_lines_wrapped_tabs(
            theme,
            base_style,
            width,
            justify,
            overflow,
            no_wrap,
            match self.tab_size {
                None | Some(0) => DEFAULT_TAB_SIZE,
                Some(tab_size) => tab_size,
            },
        )
    }

    /// The tab stop width `Text.__rich_console__` wraps with on `console`:
    /// this text's own, else the console's, and `8` for zero
    /// (`tab_size or 8`).
    pub fn console_tab_size(&self, console: &crate::console::Console) -> usize {
        match self.tab_size.unwrap_or_else(|| console.tab_size()) {
            0 => DEFAULT_TAB_SIZE,
            tab_size => tab_size,
        }
    }

    /// [`render_lines_wrapped`](Self::render_lines_wrapped) with an explicit
    /// tab stop width. Port of `Text.wrap(…, tab_size=…)`.
    #[allow(clippy::too_many_arguments)]
    pub fn render_lines_wrapped_tabs(
        &self,
        theme: &Theme,
        base_style: &Style,
        width: Option<usize>,
        justify: Justify,
        overflow: Overflow,
        no_wrap: bool,
        tab_size: usize,
    ) -> Vec<Vec<Segment>> {
        // Tabs are expanded before anything measures or wraps the text, as
        // upstream's `Text.wrap` does per line. Without this a tab occupies one
        // cell everywhere in the layout and then eight on the terminal, so every
        // width calculation downstream is wrong.
        if self.plain.contains('\t') {
            let mut expanded = self.clone();
            expanded.expand_tabs(tab_size);
            return expanded.render_lines_wrapped_tabs(
                theme, base_style, width, justify, overflow, no_wrap, tab_size,
            );
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
        let groups = self.wrapped_ranges(width, overflow, no_wrap);
        // Which spans touch which visual line, found once by binary search as
        // upstream's `divide` does. Scanning every span for every line (and
        // every cut within it) made printing a long highlighted repr quadratic.
        let line_spans = self.spans_by_line(&groups);
        let mut line_spans = line_spans.iter();
        let Some(width) = width else {
            return groups
                .into_iter()
                .flatten()
                .map(|(start, end)| {
                    let ids = line_spans.next().map(Vec::as_slice).unwrap_or(&[]);
                    self.line_segments(&resolved, start, end, &effective_base, ids)
                })
                .collect();
        };

        let mut lines: Vec<Vec<Segment>> = Vec::new();
        // One hard line at a time, as upstream's `for line in self.split(...)`
        // does — the paragraph boundary is what full justification treats as
        // ragged, so the groups cannot be flattened first.
        for group in groups {
            let group_spans: Vec<&[usize]> = group
                .iter()
                .map(|_| line_spans.next().map(Vec::as_slice).unwrap_or(&[]))
                .collect();
            let mut new_lines: Vec<Vec<Segment>> = group
                .iter()
                .zip(&group_spans)
                .map(|(&(start, end), ids)| {
                    self.line_segments(&resolved, start, end, &effective_base, ids)
                })
                .collect();

            // `overflow == "ignore"` is a hard stop upstream: the line is
            // appended verbatim and the loop `continue`s, so it is neither
            // justified nor truncated. Padding it out to the width here was
            // adding trailing spaces to text upstream returns untouched.
            if overflow == Overflow::Ignore {
                lines.append(&mut new_lines);
                continue;
            }

            // Give each wrapped line back the padding it overshot by, exactly
            // where upstream's `Text.wrap` does it — after dividing, before
            // justifying. `divide_line` counts a word *including* its trailing
            // space, so a line whose last word ends flush with the width comes
            // back one cell too long; without this the ellipsis overflow then
            // chops a real character to make room for a `…` that upstream never
            // emits ("abcdefghij more" at width 10 became "abcdefghi…", not
            // "abcdefghij").
            //
            // Only in the wrapping branch: upstream's `rstrip_end` loop sits
            // inside `Text.wrap`'s `else`, which `no_wrap` skips entirely.
            if !no_wrap {
                for line in &mut new_lines {
                    rstrip_end_line(line, width);
                }
            }
            if justify != Justify::Default {
                let last = new_lines.len().saturating_sub(1);
                for (index, line) in new_lines.iter_mut().enumerate() {
                    // Full justification leaves the final line of the paragraph
                    // ragged, so it needs to know where it is in the group.
                    let (start, end) = group[index];
                    *line = justify_line(
                        line,
                        width,
                        justify,
                        overflow,
                        &effective_base,
                        index == last,
                        &|char_index| self.covered_at(start, end, char_index, group_spans[index]),
                    );
                }
            }
            for line in &mut new_lines {
                *line = truncate_line(line, width, overflow);
            }
            lines.append(&mut new_lines);
        }
        lines
    }

    /// Whether any span covers the `char_index`-th character of the line
    /// `[start, end)` — i.e. whether upstream's `Text.render` has a span
    /// boundary at that character's edge of the line. Justify padding joins
    /// the neighbouring run only where it does not.
    ///
    /// `ids` are the spans touching that line (see [`spans_by_line`](Self::spans_by_line)).
    fn covered_at(&self, start: usize, end: usize, char_index: usize, ids: &[usize]) -> bool {
        let Some((offset, _)) = self.plain[start..end].char_indices().nth(char_index) else {
            return false;
        };
        let position = start + offset;
        ids.iter().any(|&id| {
            let span = &self.spans[id];
            span.start <= position && position < span.end
        })
    }

    /// For each visual line of `groups` (flattened, in order), the indices of
    /// the spans that overlap it, in span order. Lines are ordered and
    /// disjoint, so each span binary-searches its first and last line, as
    /// upstream's `Text.divide` does; the cost is the number of (span, line)
    /// pairs that actually overlap rather than spans × lines.
    fn spans_by_line(&self, groups: &[Vec<(usize, usize)>]) -> Vec<Vec<usize>> {
        let lines: Vec<(usize, usize)> = groups.iter().flatten().copied().collect();
        let mut by_line: Vec<Vec<usize>> = vec![Vec::new(); lines.len()];
        // Upstream only divides a text that has a newline or a wrap cut, and
        // `divide` drops spans that come out empty. An undivided text keeps
        // its empty spans, whose start/end events still split the run
        // (`[b]a[i][/i]b[/b]` renders `a` and `b` as two segments).
        let undivided = lines.len() == 1;
        for (id, span) in self.spans.iter().enumerate() {
            if span.start >= span.end {
                if undivided && span.start == span.end {
                    by_line[0].push(id);
                }
                continue;
            }
            let first = lines.partition_point(|&(_, end)| end <= span.start);
            let last = lines.partition_point(|&(start, _)| start < span.end);
            for index in first..last.max(first) {
                let (start, end) = lines[index];
                if span.start.max(start) < span.end.min(end) {
                    by_line[index].push(id);
                }
            }
        }
        by_line
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
        self.render_joined_wrapped_tabs(
            theme,
            base_style,
            width,
            justify,
            overflow,
            no_wrap,
            match self.tab_size {
                None | Some(0) => DEFAULT_TAB_SIZE,
                Some(tab_size) => tab_size,
            },
        )
    }

    /// [`render_joined_wrapped`](Self::render_joined_wrapped) with an explicit
    /// tab stop width (see [`console_tab_size`](Self::console_tab_size)).
    #[allow(clippy::too_many_arguments)]
    pub fn render_joined_wrapped_tabs(
        &self,
        theme: &Theme,
        base_style: &Style,
        width: usize,
        justify: Justify,
        overflow: Overflow,
        no_wrap: bool,
        tab_size: usize,
    ) -> Vec<Segment> {
        let lines = self.render_lines_wrapped_tabs(
            theme,
            base_style,
            Some(width),
            justify,
            overflow,
            no_wrap,
            tab_size,
        );
        let mut segments = Vec::new();
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.into_iter().enumerate() {
            // A final empty line (a trailing newline, or an empty text left
            // unpadded, as with `overflow="ignore"`) is still a line upstream:
            // `Text.render` yields `Segment("")` before the `end`.
            if index == last && line.is_empty() {
                segments.push(Segment::new("", None));
            }
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

    /// The `(start_byte, end_byte)` range of each visual line, **grouped by the
    /// hard line it came from**: hard lines split on `\n`, then each wrapped to
    /// `width` cells when `Some`.
    ///
    /// The grouping is not cosmetic. Upstream wraps and justifies one hard line
    /// at a time (`for line in self.split(...)`), so full justification leaves
    /// the last visual line of *each paragraph* ragged. Flattening first makes
    /// every paragraph but the final one get stretched, which turned
    /// `"line here"` into `"line  here"`.
    fn wrapped_ranges(
        &self,
        width: Option<usize>,
        overflow: Overflow,
        no_wrap: bool,
    ) -> Vec<Vec<(usize, usize)>> {
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
            return hard.into_iter().map(|range| vec![range]).collect();
        };
        if no_wrap {
            return hard.into_iter().map(|range| vec![range]).collect();
        }

        let mut groups: Vec<Vec<(usize, usize)>> = Vec::with_capacity(hard.len());
        for (a, b) in hard {
            let sub = &self.plain[a..b];
            // Only `fold` breaks a word that is wider than the whole line; the
            // cropping methods leave it long and let truncation cut it.
            let breaks = crate::wrap::divide_line(sub, width, overflow == Overflow::Fold);
            let mut cuts = vec![a];
            let mut previous_char = 0;
            let mut previous_byte = 0;
            for char_offset in breaks {
                // Breaks are ordered char offsets. Scan only the next slice;
                // rescanning the prefix for every line is quadratic.
                previous_byte += char_to_byte(&sub[previous_byte..], char_offset - previous_char);
                previous_char = char_offset;
                cuts.push(a + previous_byte);
            }
            cuts.push(b);
            groups.push(cuts.windows(2).map(|w| (w[0], w[1])).collect());
        }
        groups
    }

    /// Combine `effective_base` with every span covering `[start, end)`,
    /// producing non-overlapping segments for that byte range. Port of the
    /// sweep in upstream's `Text.render`: span start/end events are sorted
    /// once and walked with a stack of active spans.
    ///
    /// `resolved` is the per-render style map, index-parallel to `self.spans`;
    /// `ids` are the spans overlapping this line, in span order. Active spans
    /// are combined in span order (upstream's `sorted(stack)`), and spans that
    /// resolved to nothing are **not** skipped — they still contribute a
    /// boundary. The highlighter fixtures depend on it: an ISO-8601 date emits
    /// separate segments per sub-field even where the field styles are
    /// identical.
    fn line_segments(
        &self,
        resolved: &[Style],
        start: usize,
        end: usize,
        effective_base: &Style,
        ids: &[usize],
    ) -> Vec<Segment> {
        if start >= end {
            return Vec::new();
        }
        // (offset, leaving, span id); entering sorts before leaving.
        let mut events: Vec<(usize, bool, usize)> = Vec::with_capacity(ids.len() * 2 + 1);
        for &id in ids {
            let span = &self.spans[id];
            let (span_start, span_end) = (span.start.max(start), span.end.min(end));
            // An empty span (kept only on an undivided line) is a boundary.
            let empty = span.start == span.end && start < span.start && span.end < end;
            if span_start < span_end || empty {
                events.push((span_start, false, id));
                events.push((span_end, true, id));
            }
        }
        events.sort_unstable();

        let mut segments = Vec::new();
        let mut stack: Vec<usize> = Vec::new();
        let mut offset = start;
        let mut events = events.into_iter().peekable();
        loop {
            // Apply every event at `offset`.
            while let Some(&(position, leaving, id)) = events.peek() {
                if position > offset {
                    break;
                }
                events.next();
                match stack.binary_search(&id) {
                    Ok(index) if leaving => {
                        stack.remove(index);
                    }
                    Err(index) if !leaving => stack.insert(index, id),
                    _ => {}
                }
            }
            let next = events
                .peek()
                .map_or(end, |&(position, _, _)| position.min(end));
            if next <= offset {
                break;
            }
            let mut style = effective_base.clone();
            for &id in &stack {
                style = style.combine(&resolved[id]);
            }
            segments.push(Segment::new(&self.plain[offset..next], Some(style)));
            offset = next;
        }
        segments
    }
}

/// Python truthiness of a style argument: a null `Style` and an empty style
/// name are falsy, and upstream's `append`/`stylize` skip them.
fn is_falsy_style(style: &StyleType) -> bool {
    style.is_null_style() || matches!(style, StyleType::Name(name) if name.is_empty())
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
    // Measured and cut over the WHOLE line, exactly as upstream's `Text.truncate`
    // works on `self.plain`. Summing the segments instead is wrong wherever a
    // grapheme spans a segment boundary — a zero-width joiner at the end of one
    // segment swallows the first character of the next, so the per-segment sum
    // reads one cell too wide and cuts text upstream keeps.
    let plain: String = line.iter().map(|segment| segment.text.as_str()).collect();
    if cell_len(&plain) <= width {
        return line.to_vec();
    }
    let ellipsis = overflow == Overflow::Ellipsis;
    // `…` occupies one cell, so the kept text must stop one cell early.
    let keep = if ellipsis {
        width.saturating_sub(1)
    } else {
        width
    };
    // Never longer than `plain`, and only ever differs from a byte prefix of it
    // in its final byte (a wide grapheme straddling the cut becomes a space), so
    // slicing it at the original segment boundaries stays on char boundaries.
    let kept = set_cell_size(&plain, keep);

    let mut result: Vec<Segment> = Vec::new();
    // Style the ellipsis inherits: that of the first segment the cut did not
    // keep whole, falling back to the last segment's when the cut lands exactly
    // on the end of the line's bytes.
    let mut cut_style: Option<Style> = line.last().and_then(|segment| segment.style.clone());
    let mut offset = 0usize;
    for segment in line {
        if offset >= kept.len() {
            cut_style = segment.style.clone();
            break;
        }
        let end = (offset + segment.text.len()).min(kept.len());
        if end > offset {
            result.push(Segment::new(&kept[offset..end], segment.style.clone()));
        }
        if offset + segment.text.len() > kept.len() {
            cut_style = segment.style.clone();
            break;
        }
        offset = end;
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
///
/// `covered(i)` says whether a span covers the line's `i`-th character.
/// Upstream pads the plain string, so its padding joins the adjacent run
/// unless a span ends (or starts) at the text's edge; a separate segment
/// would re-emit the style as a second SGR run.
fn justify_line(
    line: &[Segment],
    width: usize,
    justify: Justify,
    overflow: Overflow,
    style: &Style,
    is_last: bool,
    covered: &dyn Fn(usize) -> bool,
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
    let mut content = line.to_vec();
    // Upstream's `Lines.justify` calls `line.rstrip()` in its `center` and
    // `right` branches — and only there — so the space wrapping left at the end
    // of a line is *not* content to be positioned. Keeping it shifts the visible
    // text half a space left when centring (`" abcd efgh ijklmnop "` became
    // `"abcd efgh ijklmnop  "`) and a whole column left when right-aligning.
    // `left`/`full` deliberately keep it: upstream pads them without stripping.
    if matches!(justify, Justify::Center | Justify::Right) {
        rstrip_line(&mut content);
        // …and then TRUNCATES, before it pads. The order is load-bearing: cell
        // width is not additive across a cut, so a line whose over-long tail is
        // chopped can measure *less* than the width afterwards and still want
        // padding. A leading zero-width joiner is the clearest case — it eats the
        // character after it, so cutting the line hands one of its cells back —
        // and padding first computes the gap from the pre-cut measurement, which
        // is zero, and leaves the line short.
        content = truncate_line(&content, width, overflow);
    }

    // Whether padding may join the content's first / last run: no span
    // covers the character at that edge, so upstream's run there is the base
    // style alone and the padding extends it.
    let content_chars: usize = content.iter().map(|s| s.text.chars().count()).sum();
    // With no content at all, the two pads of a centred line are one run.
    let join_left = content_chars > 0 && !covered(0);
    let join_right = content_chars == 0 || !covered(content_chars - 1);

    let mut out = Vec::with_capacity(content.len() + 2);
    match justify {
        Justify::Right => {
            // `line.pad_left(width - cell_len(line.plain))`.
            let excess = width.saturating_sub(line_cell_len(&content));
            if excess > 0 {
                out.push(Segment::new(" ".repeat(excess), Some(style.clone())));
            }
            append_joined(&mut out, content, join_left);
        }
        Justify::Center => {
            // `pad_left((width - cell_len) // 2)` and then `pad_right(width -
            // cell_len)` — the second `cell_len` is re-measured *after* the left
            // pad, so the two halves are not simply `excess / 2` and the rest.
            let left = width.saturating_sub(line_cell_len(&content)) / 2;
            if left > 0 {
                out.push(Segment::new(" ".repeat(left), Some(style.clone())));
            }
            append_joined(&mut out, content, join_left);
            let right = width.saturating_sub(line_cell_len(&out));
            if right > 0 {
                push_joined(
                    &mut out,
                    Segment::new(" ".repeat(right), Some(style.clone())),
                    join_right,
                );
            }
        }
        // Left, Default, and full justification's ragged last line pad right.
        // Upstream reaches this through `truncate(width, pad=True)`, whose pad
        // is driven by the *pre*-truncate length — so padding and truncating are
        // mutually exclusive here and the order does not matter.
        Justify::Left | Justify::Full | Justify::Default => {
            let excess = width.saturating_sub(line_cell_len(&content));
            out.append(&mut content);
            if excess > 0 {
                push_joined(
                    &mut out,
                    Segment::new(" ".repeat(excess), Some(style.clone())),
                    join_right,
                );
            }
        }
    }
    out
}

/// Append `segment`, extending the last segment instead when `join` allows it
/// and the two share a style.
fn push_joined(out: &mut Vec<Segment>, segment: Segment, join: bool) {
    match out.last_mut() {
        Some(last) if join && !last.control && last.style == segment.style => {
            last.text.push_str(&segment.text);
        }
        _ => out.push(segment),
    }
}

/// Append `content`, joining its first segment onto the last of `out` when
/// `join` allows it and the two share a style.
fn append_joined(out: &mut Vec<Segment>, content: Vec<Segment>, join: bool) {
    let mut content = content.into_iter();
    if let Some(first) = content.next() {
        push_joined(out, first, join);
    }
    out.extend(content);
}

/// The cell width of a rendered line.
fn line_cell_len(line: &[Segment]) -> usize {
    line.iter().map(Segment::cell_length).sum()
}

/// The number of trailing whitespace *characters* on a rendered line.
///
/// Segment-level, because by the time the wrap pipeline justifies a line the
/// spans have already been flattened into [`Segment`]s and there is no `Text`
/// left to call `rstrip` on.
fn trailing_whitespace(line: &[Segment]) -> usize {
    let mut count = 0usize;
    for segment in line.iter().rev() {
        let trimmed = segment.text.trim_end_matches(is_python_space);
        count += segment.text[trimmed.len()..].chars().count();
        if !trimmed.is_empty() {
            break;
        }
    }
    count
}

/// Drop the last `count` characters, discarding segments that empty out.
/// Segment-level counterpart of `Text.right_crop`.
fn right_crop_line(line: &mut Vec<Segment>, count: usize) {
    let mut remaining = count;
    while remaining > 0 {
        let Some(last) = line.last_mut() else { break };
        let length = last.text.chars().count();
        if length <= remaining {
            remaining -= length;
            line.pop();
        } else {
            let keep = char_to_byte(&last.text, length - remaining);
            last.text.truncate(keep);
            remaining = 0;
        }
    }
}

/// Remove all trailing whitespace. Segment-level counterpart of `Text.rstrip`.
fn rstrip_line(line: &mut Vec<Segment>) {
    right_crop_line(line, trailing_whitespace(line));
}

/// Remove *only as much* trailing whitespace as it takes to get the line down to
/// `size`, leaving the rest. Segment-level counterpart of `Text.rstrip_end`.
///
/// The length compared against `size` is a **character** count, not a cell
/// count: upstream's `Text.rstrip_end` uses `len(self)`, which is
/// `len(self.plain)`. The two only diverge on wide characters, and copying the
/// quirk is cheaper than explaining a one-column difference later.
fn rstrip_end_line(line: &mut Vec<Segment>, size: usize) {
    let length: usize = line.iter().map(|s| s.text.chars().count()).sum();
    let Some(excess) = length.checked_sub(size).filter(|excess| *excess > 0) else {
        return;
    };
    let whitespace = trailing_whitespace(line);
    if whitespace > 0 {
        right_crop_line(line, whitespace.min(excess));
    }
}

/// `Text.__rich_measure__` on a plain string: `(widest word, widest line)`.
pub(crate) fn measure_plain(plain: &str) -> (usize, usize) {
    // `text.splitlines()` and `text.split()`: Python's line boundaries
    // (U+2028, U+0085, …) and whitespace (including U+001C..U+001F), not
    // just `\n` and Rust's `White_Space`.
    let max_line = python_splitlines(plain)
        .into_iter()
        .map(cell_len)
        .max()
        .unwrap_or(0);
    let min_word = plain
        .split(is_python_space)
        .filter(|word| !word.is_empty())
        .map(cell_len)
        .max()
        .unwrap_or(max_line);
    (min_word, max_line)
}

/// Python's `str.isspace()`: Rust's `White_Space` plus the four ASCII
/// information separators U+001C..=U+001F.
pub(crate) fn is_python_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Port of Python's `str.splitlines()`: every Unicode line boundary ends a
/// line, `\r\n` counts once, and a trailing boundary adds no empty line.
pub(crate) fn python_splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if matches!(
            c,
            '\n' | '\r'
                | '\x0b'
                | '\x0c'
                | '\x1c'
                | '\x1d'
                | '\x1e'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        ) {
            lines.push(&text[start..i]);
            start = i + c.len_utf8();
            if c == '\r' && chars.peek().map(|&(_, n)| n) == Some('\n') {
                chars.next();
                start += 1;
            }
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each divided line's text and its spans as (start, end, style).
    type Divided = Vec<(String, Vec<(usize, usize, String)>)>;

    /// `divide` as it was before the per-span line search: every line for
    /// every span. The search must give the same lines, spans and span order.
    fn divide_by_scanning(text: &Text, offsets: &[usize]) -> Divided {
        if offsets.is_empty() {
            let spans = text
                .spans
                .iter()
                .map(|s| (s.start, s.end, format!("{:?}", s.style)))
                .collect();
            return vec![(text.plain.clone(), spans)];
        }
        let mut bounds = vec![0];
        bounds.extend(offsets.iter().copied());
        bounds.push(text.plain.len());
        let mut lines: Divided = bounds
            .windows(2)
            .map(|w| {
                let (start, end) = (w[0].min(text.plain.len()), w[1].min(text.plain.len()));
                let plain = if start < end {
                    text.plain[start..end].to_string()
                } else {
                    String::new()
                };
                (plain, Vec::new())
            })
            .collect();
        for span in &text.spans {
            for (index, window) in bounds.windows(2).enumerate() {
                let (line_start, line_end) = (window[0], window[1]);
                let new_start = span.start.max(line_start) - line_start;
                let new_end = span.end.min(line_end).saturating_sub(line_start);
                if new_end > new_start {
                    lines[index]
                        .1
                        .push((new_start, new_end, format!("{:?}", span.style)));
                }
            }
        }
        lines
    }

    #[test]
    fn divide_matches_the_full_scan_for_sorted_offsets() {
        let mut state = 0x5eed_u64;
        let mut next = |bound: usize| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as usize % bound.max(1)
        };
        for case in 0..500 {
            let len = next(40);
            let plain: String = (0..len)
                .map(|i| if i % 7 == 3 { '\n' } else { 'a' })
                .collect();
            let mut text = Text::new(plain.clone());
            for i in 0..next(12) {
                // Empty, overlapping and past-the-end spans included.
                let start = next(len + 3);
                let end = start + next(len + 3);
                text.spans.push(Span {
                    start,
                    end,
                    style: format!("s{i}").as_str().into(),
                });
            }
            let mut offsets: Vec<usize> = (0..next(10)).map(|_| next(len + 4)).collect();
            offsets.sort_unstable();
            let divided: Divided = text
                .divide(&offsets)
                .into_iter()
                .map(|line| {
                    let spans = line
                        .spans
                        .iter()
                        .map(|s| (s.start, s.end, format!("{:?}", s.style)))
                        .collect();
                    (line.plain, spans)
                })
                .collect();
            assert_eq!(
                divided,
                divide_by_scanning(&text, &offsets),
                "case {case}: {plain:?} {offsets:?}"
            );
        }
    }

    /// An unbroken run of VS16 emoji must fold at the width like anything else.
    /// Measured per code point it did not: the heart reads one cell and the
    /// variation selector zero, so twenty hearts "fit" in thirty cells and came
    /// back as a single forty-cell row — wide enough to punch through the panel
    /// or table border drawn around it.
    ///
    /// Real rich 15.0.0, `[cell_len(l.plain) for l in Text("❤️"*20).wrap(c, 30)]`
    /// is `[30, 10]`.
    #[test]
    fn an_emoji_run_folds_at_the_width_instead_of_overflowing() {
        let hearts = "\u{2764}\u{fe0f}".repeat(20);
        let widths: Vec<usize> = wrapped_plain(&Text::new(&hearts), 30)
            .iter()
            .map(|line| cell_len(line))
            .collect();
        assert_eq!(widths, vec![30, 10]);
    }

    /// Upstream wraps and justifies **one hard line at a time**, so the line
    /// full justification leaves ragged is the last of each paragraph — not just
    /// the last of the whole text. Flattening first stretched every paragraph
    /// but the final one.
    ///
    /// Real rich 15.0.0, `Text(case, justify="full").wrap(console, width)`:
    ///
    /// ```text
    /// width 10 -> ['word', '  indented', 'line here', 'last']
    /// width 30 -> ['word', '  indented line here', 'last']   (no_wrap)
    /// ```
    ///
    /// `line here` is the giveaway: it ends its paragraph, so upstream leaves
    /// the single gap alone where we widened it to `line  here`.
    #[test]
    fn full_justify_leaves_each_paragraphs_last_line_ragged() {
        let text = Text::new("word\n  indented line here\nlast").justify(Justify::Full);
        assert_eq!(
            wrapped_plain(&text, 10),
            vec!["word", "  indented", "line here", "last"]
        );
        let no_wrap = Text::new("word\n  indented line here\nlast")
            .justify(Justify::Full)
            .no_wrap(true);
        assert_eq!(
            wrapped_plain(&no_wrap, 30),
            vec!["word", "  indented line here", "last"]
        );
    }

    /// `overflow="ignore"` is a hard stop in upstream's `Text.wrap`: the line is
    /// appended verbatim and the loop `continue`s, so it is neither justified nor
    /// truncated. Padding it out to the width added trailing spaces to content
    /// upstream returns byte-for-byte.
    ///
    /// Real rich 15.0.0, `Text(case, justify=…, overflow="ignore").wrap(c, w)`:
    ///
    /// ```text
    /// left   'hello'         @12 -> ['hello']
    /// center 'hello'         @12 -> ['hello']
    /// right  'trailing   '   @3  -> ['trailing   ']
    /// ```
    #[test]
    fn overflow_ignore_is_neither_justified_nor_truncated() {
        for justify in [Justify::Left, Justify::Center, Justify::Right] {
            let text = Text::new("hello")
                .justify(justify)
                .overflow(Overflow::Ignore);
            assert_eq!(wrapped_plain(&text, 12), vec!["hello"], "{justify:?}");
        }
        let text = Text::new("trailing   ")
            .justify(Justify::Right)
            .overflow(Overflow::Ignore);
        assert_eq!(wrapped_plain(&text, 3), vec!["trailing   "]);
    }

    /// Upstream's `Lines.justify` truncates *inside* its center and right
    /// branches, before it pads. The order matters because cell width is not
    /// additive across a cut: a leading zero-width joiner eats the character
    /// after it, so chopping the line's tail hands a cell back and the line then
    /// wants padding it did not want before. Padding first measures the un-cut
    /// line, finds no slack, and leaves the line a column short.
    ///
    /// Real rich 15.0.0:
    ///
    /// ```text
    /// right    '‍┬┴⠁├╰⠃⡁⠃╯⠆┴' @9, ellipsis -> [' ‍┬┴⠁├╰⠃⡁⠃…']
    /// right    '‍⠃╯⠆┴'         @2, crop, no_wrap -> [' ‍⠃╯']
    /// center   's ‍8-o🧠e'      @4, ellipsis -> [' s  ', '‍8-o… ']
    /// ```
    #[test]
    fn center_and_right_truncate_before_they_pad() {
        let text = Text::new("\u{200d}┬┴⠁├╰⠃⡁⠃╯⠆┴")
            .justify(Justify::Right)
            .overflow(Overflow::Ellipsis);
        assert_eq!(wrapped_plain(&text, 9), vec![" \u{200d}┬┴⠁├╰⠃⡁⠃…"]);

        let cropped = Text::new("\u{200d}⠃╯⠆┴")
            .justify(Justify::Right)
            .overflow(Overflow::Crop)
            .no_wrap(true);
        assert_eq!(wrapped_plain(&cropped, 2), vec![" \u{200d}⠃╯"]);

        let centered = Text::new("s \u{200d}8-o\u{1f9e0}e")
            .justify(Justify::Center)
            .overflow(Overflow::Ellipsis);
        assert_eq!(wrapped_plain(&centered, 4), vec![" s  ", "\u{200d}8-o… "]);
    }

    /// A line is measured and cut as one string, the way upstream's
    /// `Text.truncate` works on `self.plain` — not segment by segment. Cell
    /// width is not additive across a segment boundary: full justification
    /// splits the line into one segment per word, which strands the zero-width
    /// joiner at the end of `π‍` away from the space it swallows, so the
    /// per-segment sum reads eight cells for a seven-cell line and an ellipsis
    /// eats a character upstream keeps.
    ///
    /// Real rich 15.0.0,
    /// `Text("⚠1️;&　π‍  ψ\u{a0}τ\u{a0}γ ⡀", justify="full", overflow="ellipsis").wrap(c, 7)`:
    ///
    /// ```text
    /// ['⚠1️;&　', 'π‍  ψ τ γ', '⡀']   with cell widths [7, 7, 1]
    /// ```
    #[test]
    fn a_line_is_measured_whole_not_segment_by_segment() {
        let text = Text::new("\u{26a0}1\u{fe0f};&\u{3000}\u{3c0}\u{200d}  \u{3c8}\u{a0}\u{3c4}\u{a0}\u{3b3} \u{2840}")
            .justify(Justify::Full)
            .overflow(Overflow::Ellipsis);
        assert_eq!(
            wrapped_plain(&text, 7),
            vec![
                "\u{26a0}1\u{fe0f};&\u{3000}",
                "\u{3c0}\u{200d}  \u{3c8}\u{a0}\u{3c4}\u{a0}\u{3b3}",
                "\u{2840}"
            ]
        );
    }

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

    fn wrapped_plain(text: &Text, width: usize) -> Vec<String> {
        text.render_lines(&Theme::default_theme(), &Style::new(), Some(width))
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect())
            .collect()
    }

    /// Wrapping hands each line the space that ended it, and upstream's
    /// `Lines.justify` throws that space away (`line.rstrip()`) before centring
    /// or right-aligning — but *not* before left-aligning or full-justifying.
    ///
    /// Captured verbatim from real rich 15.0.0,
    /// `Text(case, justify=…).wrap(console, 20)`:
    ///
    /// ```text
    /// center 'abcd efgh ijklmnop qrst'  -> ' abcd efgh ijklmnop '
    /// right  'abcd efgh ijklmnop qrst'  -> '  abcd efgh ijklmnop'
    /// right  'aaaa bbbb cccc dddd eeee' -> ' aaaa bbbb cccc dddd'
    /// left   'abcd efgh ijklmnop qrst'  -> 'abcd efgh ijklmnop  '
    /// ```
    ///
    /// Counting the wrap space as content puts the centred line one column too
    /// far left and the right-aligned line a whole column short of the edge.
    #[test]
    fn center_and_right_rstrip_the_wrap_space() {
        let wrapped = "abcd efgh ijklmnop qrst";
        assert_eq!(
            wrapped_plain(&Text::new(wrapped).justify(Justify::Center), 20)[0],
            " abcd efgh ijklmnop "
        );
        assert_eq!(
            wrapped_plain(&Text::new(wrapped).justify(Justify::Right), 20)[0],
            "  abcd efgh ijklmnop"
        );
        assert_eq!(
            wrapped_plain(
                &Text::new("aaaa bbbb cccc dddd eeee").justify(Justify::Right),
                20
            )[0],
            " aaaa bbbb cccc dddd"
        );
        // Left is the control: upstream pads it without stripping, so the
        // trailing space stays part of the line and nothing shifts.
        assert_eq!(
            wrapped_plain(&Text::new(wrapped).justify(Justify::Left), 20)[0],
            "abcd efgh ijklmnop  "
        );
    }

    /// `divide_line` measures a word *with* its trailing space, so a line whose
    /// last word ends flush with the width comes back one character too long.
    /// Upstream's `Text.wrap` calls `rstrip_end(width)` on every divided line to
    /// hand that back before overflow is applied.
    ///
    /// Real rich 15.0.0, `Text(case, overflow="ellipsis").wrap(console, 10)`:
    ///
    /// ```text
    /// 'abcdefghij more'   -> ['abcdefghij', 'more']   <- no ellipsis
    /// 'abcdefghijkl more' -> ['abcdefghi…', 'more']   <- genuinely too long
    /// ```
    ///
    /// Skipping the rstrip makes the first case measure 11 cells, so the
    /// ellipsis fires and eats the `j` that upstream keeps.
    #[test]
    fn rstrip_end_stops_the_wrap_space_from_triggering_an_ellipsis() {
        assert_eq!(
            wrapped_plain(
                &Text::new("abcdefghij more").overflow(Overflow::Ellipsis),
                10
            ),
            vec!["abcdefghij", "more"]
        );
        assert_eq!(
            wrapped_plain(
                &Text::new("abcdefghijkl more").overflow(Overflow::Ellipsis),
                10
            ),
            vec!["abcdefghi…", "more"]
        );
    }

    /// `Text.apply_meta` / `Text.on`, as rich 15.0.0 records them.
    #[test]
    fn apply_meta_and_on_add_meta_spans() {
        use crate::style::{Meta, MetaValue};
        let mut text = Text::new("hello");
        text.apply_meta([("a", MetaValue::Int(1))].into_iter().collect(), 1, Some(3));
        text.on(None, &[("click", MetaValue::Str("x".into()))]);
        text.apply_meta(Meta::new(), 0, None);
        let spans = text.spans();
        assert_eq!(spans.len(), 2);
        assert_eq!((spans[0].start, spans[0].end), (1, 3));
        let meta = |span: &Span| match &span.style {
            StyleType::Style(style) => style.meta(),
            StyleType::Name(_) => Meta::new(),
        };
        assert_eq!(
            meta(&spans[0]),
            [("a", MetaValue::Int(1))].into_iter().collect()
        );
        assert_eq!(
            meta(&spans[1]),
            [("@click", MetaValue::Str("x".into()))]
                .into_iter()
                .collect()
        );
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
