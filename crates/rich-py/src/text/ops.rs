//! Rich's `Text` operations on core texts, in Rich's terms: character
//! offsets (core spans hold byte offsets), and Rich's own semantics where a
//! core method differs (`rstrip_end` counts characters, `divide` does not
//! copy `no_wrap`).
//!
//! Where a core method already behaves as Rich's does, the bindings call it
//! directly; these fill the gaps.

use pyo3::prelude::*;

use rich::cells::cell_len;
use rich::text::Span;
use rich::{Overflow, Style as CoreStyle, StyleType, Text as CoreText};

use crate::style::Style;

/// A span in character offsets.
pub(crate) type CharSpan = (usize, usize, StyleType);

/// The byte offset of every character boundary of `plain`, the end included.
pub(crate) fn boundaries(plain: &str) -> Vec<usize> {
    plain
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(plain.len()))
        .collect()
}

/// The number of characters (Python's `len`).
pub(crate) fn char_len(text: &CoreText) -> usize {
    text.plain().chars().count()
}

/// The byte offset of character `index`, clamped to the text.
pub(crate) fn byte_at(bounds: &[usize], index: usize) -> usize {
    bounds[index.min(bounds.len() - 1)]
}

/// The character offset of byte `offset`.
fn char_at(bounds: &[usize], offset: usize) -> usize {
    bounds.partition_point(|&boundary| boundary < offset)
}

/// A text's spans in character offsets, in order.
pub(crate) fn char_spans(text: &CoreText) -> Vec<CharSpan> {
    let bounds = boundaries(text.plain());
    text.spans()
        .iter()
        .map(|span| {
            (
                char_at(&bounds, span.start),
                char_at(&bounds, span.end),
                span.style.clone(),
            )
        })
        .collect()
}

/// A core text with `plain` and `spans` (character offsets, kept as given,
/// clamped to the text) and `like`'s base style, justify, overflow, no-wrap
/// and tab size.
pub(crate) fn rebuild(like: &CoreText, plain: &str, spans: &[CharSpan]) -> CoreText {
    let mut text = CoreText::new(plain);
    copy_settings(like, &mut text);
    let bounds = boundaries(text.plain());
    text.set_spans(
        spans
            .iter()
            .map(|(start, end, style)| Span {
                start: byte_at(&bounds, *start),
                end: byte_at(&bounds, *end),
                style: style.clone(),
            })
            .collect(),
    );
    text
}

/// Give `to` `from`'s base style, justify, overflow, no-wrap and tab size.
pub(crate) fn copy_settings(from: &CoreText, to: &mut CoreText) {
    to.set_base_style(from.base_style().clone());
    to.set_justify(from.get_justify());
    to.set_overflow(from.get_overflow());
    to.set_no_wrap(from.get_no_wrap());
    to.set_tab_size(from.get_tab_size());
}

/// Rich's `plain` setter: a shorter string trims the spans past its end.
pub(crate) fn set_plain(text: &mut CoreText, plain: &str) {
    if plain == text.plain() {
        return;
    }
    let old_length = char_len(text);
    let sanitized = CoreText::new(plain);
    let new_length = char_len(&sanitized);
    let mut spans = char_spans(text);
    if new_length < old_length {
        spans.retain(|(start, _, _)| *start < new_length);
        for span in &mut spans {
            span.1 = span.1.min(new_length);
        }
    }
    *text = rebuild(text, sanitized.plain(), &spans);
}

/// Rich's `Text.stylize` with character offsets (negative from the end).
/// Offsets out of range are clamped.
pub(crate) fn stylize(text: &mut CoreText, style: StyleType, start: isize, end: Option<isize>) {
    let length = char_len(text) as isize;
    let start = if start < 0 { length + start } else { start }.max(0);
    let end = match end {
        None => length,
        Some(end) if end < 0 => length + end,
        Some(end) => end,
    };
    if start >= length || end <= start {
        return;
    }
    let bounds = boundaries(text.plain());
    let end = end.min(length);
    text.stylize(
        style,
        byte_at(&bounds, start as usize),
        byte_at(&bounds, end as usize),
    );
}

/// Rich's `Text.right_crop`: drop the last `amount` characters.
pub(crate) fn right_crop(text: &mut CoreText, amount: usize) {
    if amount == 0 {
        return;
    }
    let length = char_len(text);
    let bounds = boundaries(text.plain());
    let keep = byte_at(&bounds, length.saturating_sub(amount));
    text.right_crop(text.plain().len() - keep);
}

/// Python's `str.isspace` for one character (what `\s` matches).
pub(crate) fn is_python_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Rich's `Text.rstrip_end`: drop trailing whitespace beyond `size`
/// characters.
pub(crate) fn rstrip_end(text: &mut CoreText, size: usize) {
    let length = char_len(text);
    if length <= size {
        return;
    }
    let excess = length - size;
    let whitespace = text
        .plain()
        .chars()
        .rev()
        .take_while(|c| is_python_space(*c))
        .count();
    if whitespace > 0 {
        right_crop(text, whitespace.min(excess));
    }
}

/// Rich's `Text.rstrip`.
pub(crate) fn rstrip(text: &mut CoreText) {
    let plain = text.plain().trim_end_matches(is_python_space).to_string();
    set_plain(text, &plain);
}

/// Rich's `Text.set_length`: pad or crop to `length` characters.
pub(crate) fn set_length(text: &mut CoreText, length: usize) {
    let current = char_len(text);
    if current < length {
        text.pad_right(length - current, ' ');
    } else if current > length {
        right_crop(text, current - length);
    }
}

/// Rich's `Text.extend_style`: append spaces that take the style of every
/// span reaching the end.
pub(crate) fn extend_style(text: &mut CoreText, spaces: usize) {
    if spaces == 0 {
        return;
    }
    let length = char_len(text);
    let mut spans = char_spans(text);
    for span in &mut spans {
        if span.1 >= length {
            span.1 += spaces;
        }
    }
    let plain = format!("{}{}", text.plain(), " ".repeat(spaces));
    *text = rebuild(text, &plain, &spans);
}

/// Rich's `Text.divide` at character offsets. The pieces keep the style,
/// justify and overflow, but not no-wrap or the tab size, as Rich's do.
pub(crate) fn divide(text: &CoreText, offsets: &[isize]) -> Vec<CoreText> {
    if offsets.is_empty() {
        return vec![text.clone()];
    }
    let length = char_len(text) as isize;
    let bounds = boundaries(text.plain());
    let bytes: Vec<usize> = offsets
        .iter()
        .map(|&offset| byte_at(&bounds, offset.clamp(0, length) as usize))
        .collect();
    let mut lines = text.divide(&bytes);
    for line in &mut lines {
        line.set_no_wrap(None);
        line.set_tab_size(None);
    }
    lines
}

/// Rich's `Text.split`.
pub(crate) fn split(
    text: &CoreText,
    separator: &str,
    include_separator: bool,
    allow_blank: bool,
) -> Vec<CoreText> {
    let mut lines = text.split(separator, include_separator, allow_blank);
    if text.plain().contains(separator) {
        for line in &mut lines {
            line.set_no_wrap(None);
            line.set_tab_size(None);
        }
    }
    lines
}

/// `console.get_style(style)` for a core style type; `default` is used
/// for a missing name when given.
fn console_style(
    console: &Bound<'_, PyAny>,
    style: &StyleType,
    default: Option<&str>,
) -> PyResult<CoreStyle> {
    let py = console.py();
    let value = crate::style::py_style_type(py, style)?;
    let kwargs = pyo3::types::PyDict::new(py);
    if let Some(default) = default {
        kwargs.set_item("default", default)?;
    }
    let resolved = console.call_method("get_style", (value,), Some(&kwargs))?;
    Ok(resolved.cast::<Style>()?.get().inner.clone())
}

/// Rich's `Text.get_style_at_offset`: the style of one character.
pub(crate) fn style_at_offset(
    console: &Bound<'_, PyAny>,
    text: &CoreText,
    offset: isize,
) -> PyResult<CoreStyle> {
    let offset = if offset < 0 {
        char_len(text) as isize + offset
    } else {
        offset
    };
    let mut style = console_style(console, text.base_style(), None)?;
    for (start, end, span_style) in char_spans(text) {
        if (end as isize) > offset && offset >= start as isize {
            style = style.combine(&console_style(console, &span_style, Some(""))?);
        }
    }
    Ok(style)
}

/// Rich's `Lines.justify` over core texts. With `"full"` every line but the
/// last is replaced; the others change in place.
pub(crate) fn justify_lines(
    console: &Bound<'_, PyAny>,
    lines: &mut [CoreText],
    width: usize,
    justify: &str,
    overflow: Overflow,
) -> PyResult<()> {
    match justify {
        "left" => {
            for line in lines.iter_mut() {
                line.truncate(width, Some(overflow), true);
            }
        }
        "center" => {
            for line in lines.iter_mut() {
                rstrip(line);
                line.truncate(width, Some(overflow), false);
                line.pad_left(width.saturating_sub(line.cell_len()) / 2, ' ');
                line.pad_right(width.saturating_sub(line.cell_len()), ' ');
            }
        }
        "right" => {
            for line in lines.iter_mut() {
                rstrip(line);
                line.truncate(width, Some(overflow), false);
                line.pad_left(width.saturating_sub(line.cell_len()), ' ');
            }
        }
        "full" => {
            let count = lines.len();
            #[allow(clippy::needless_range_loop)] // `lines[index]` is replaced below
            for index in 0..count.saturating_sub(1) {
                let line = &lines[index];
                let words = split(line, " ", false, false);
                let words_size: usize = words.iter().map(|word| cell_len(word.plain())).sum();
                let mut num_spaces = words.len().saturating_sub(1);
                let mut spaces = vec![1usize; num_spaces];
                let mut position = 0;
                if !spaces.is_empty() {
                    while words_size + num_spaces < width {
                        let slot = spaces.len() - position - 1;
                        spaces[slot] += 1;
                        num_spaces += 1;
                        position = (position + 1) % spaces.len();
                    }
                }
                let mut tokens: Vec<CoreText> = Vec::new();
                for (position, word) in words.iter().enumerate() {
                    tokens.push(word.clone());
                    if position < spaces.len() {
                        let style = style_at_offset(console, word, -1)?;
                        let next_style = style_at_offset(console, &words[position + 1], -1)?;
                        let space_style = if style == next_style {
                            StyleType::Style(style)
                        } else {
                            line.base_style().clone()
                        };
                        tokens.push(CoreText::styled(" ".repeat(spaces[position]), space_style));
                    }
                }
                lines[index] = CoreText::new("").join(&tokens);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Rich's `Text.wrap` over a core text.
#[allow(clippy::too_many_arguments)]
pub(crate) fn wrap(
    console: &Bound<'_, PyAny>,
    text: &CoreText,
    width: usize,
    justify: Option<&str>,
    overflow: Option<Overflow>,
    tab_size: usize,
    no_wrap: Option<bool>,
) -> PyResult<Vec<CoreText>> {
    let own_justify = crate::convert::justify_name(text.get_justify());
    let wrap_justify = justify.or(own_justify).unwrap_or("default");
    let wrap_overflow = overflow.or(text.get_overflow()).unwrap_or(Overflow::Fold);
    let ignore = overflow == Some(Overflow::Ignore);
    let no_wrap = no_wrap.or(text.get_no_wrap()).unwrap_or(false) || ignore;

    let mut lines: Vec<CoreText> = Vec::new();
    for mut line in split(text, "\n", false, true) {
        if line.plain().contains('\t') {
            line.expand_tabs(tab_size);
        }
        let mut new_lines = if no_wrap {
            if ignore {
                lines.push(line);
                continue;
            }
            vec![line]
        } else {
            let offsets: Vec<isize> =
                rich::wrap::divide_line(line.plain(), width, wrap_overflow == Overflow::Fold)
                    .into_iter()
                    .map(|offset| offset as isize)
                    .collect();
            let mut divided = divide(&line, &offsets);
            for piece in &mut divided {
                rstrip_end(piece, width);
            }
            divided
        };
        justify_lines(console, &mut new_lines, width, wrap_justify, wrap_overflow)?;
        for piece in &mut new_lines {
            piece.truncate(width, Some(wrap_overflow), false);
        }
        lines.extend(new_lines);
    }
    Ok(lines)
}

/// The justify Rich's `JustifyMethod` names, for [`wrap`].
pub(crate) fn justify_arg(value: Option<&str>) -> PyResult<Option<&str>> {
    match value {
        None => Ok(None),
        Some(name) => {
            crate::convert::justify(Some(name))?;
            Ok(Some(name))
        }
    }
}
