//! `rich.syntax`: `Syntax`, plus the port's code-highlighter choice.
//!
//! Highlighting is core's (`rich::Syntax` over a `CodeHighlighter`: syntect
//! by default, lumis in a lumis build, chosen by name through the
//! `rich-ext` registry). Core's `Syntax` renders the plain case (no line
//! numbers, wrapping, line range, indent guides, background override,
//! stylized ranges or code width), and does so here. Upstream's other
//! layouts (`Syntax._get_syntax`) are not in core yet, so they are ported
//! below on top of core's highlighted `Text`; they move to core when it
//! gains them.

use std::collections::HashSet;
use std::sync::Arc;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyString;

use rich::color::{Color as CoreColor, ColorSystem};
use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::{CodeHighlighter, ConsoleCodeHighlighting, Renderable};
use rich::segment::Segment as CoreSegment;
use rich::style::{Style as CoreStyle, StyleType};
use rich::{Justify, Overflow, Text as CoreText};

use super::highlighter::{char_offsets, new_text};
use super::layout::join_lines;
use super::pretty::with_indent_guides;
use crate::convert;
use crate::renderable::{self, AsRenderable};
use crate::style::style_type;
use crate::text::Text;

/// Upstream's `Syntax` default theme. The default engine has no theme of
/// that name and falls back to its own default (DIVERGENCES #18).
pub(crate) const DEFAULT_THEME: &str = "monokai";

/// Upstream's `NUMBERS_COLUMN_DEFAULT_PADDING`.
const NUMBERS_COLUMN_DEFAULT_PADDING: usize = 2;

// ---------------------------------------------------------------------------
// The code-highlighter choice

/// The registry of code highlighters: `rich-ext`'s built-ins (`syntect`),
/// plus `lumis` in a lumis build.
fn registry() -> rich_ext::registry::ExtensionRegistry {
    #[allow(unused_mut)]
    let mut registry = rich_ext::registry::ExtensionRegistry::with_defaults();
    #[cfg(feature = "lumis")]
    registry
        .add_plugin(&rich_lumis::LumisPlugin)
        .expect("the lumis plugin registers cleanly");
    registry
}

/// The code highlighter called `name` (`None`: the console's default, else
/// syntect). An unknown name is a `ValueError` listing the known ones.
pub(crate) fn code_highlighter(name: Option<&str>) -> PyResult<Option<Arc<dyn CodeHighlighter>>> {
    let Some(name) = name else {
        return Ok(None);
    };
    let registry = registry();
    registry.code_highlighter(name).map(Some).ok_or_else(|| {
        let hint = if name == "lumis" && !cfg!(feature = "lumis") {
            " (lumis needs the rs-rich lumis build)"
        } else {
            ""
        };
        PyValueError::new_err(format!(
            "unknown code highlighter '{name}'{hint}; expected one of {}",
            registry.code_highlighter_names().join(", ")
        ))
    })
}

/// A `highlighter=` argument: `None` (the console's default), a name, or a
/// plugin code highlighter (a `rs_rich.plugins` handle or a Python object
/// with `highlight`, `default_theme` and `themes`).
pub(crate) fn code_highlighter_value(
    value: Option<&Bound<'_, PyAny>>,
) -> PyResult<Option<Arc<dyn CodeHighlighter>>> {
    match value.filter(|v| !v.is_none()) {
        None => Ok(None),
        Some(value) => match value.cast::<PyString>() {
            Ok(name) => code_highlighter(Some(name.to_cow()?.as_ref())),
            Err(_) => crate::plugins::code_highlighter_arg(value).map(Some),
        },
    }
}

/// The names of the code highlighters this build has (`syntect`, and
/// `lumis` in a lumis build). Not in Rich.
#[pyfunction]
fn code_highlighters() -> Vec<String> {
    registry()
        .code_highlighter_names()
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// The theme names a code highlighter accepts (default: syntect). Not in
/// Rich, whose themes are Pygments styles.
#[pyfunction]
#[pyo3(signature = (highlighter=None))]
fn code_themes(highlighter: Option<&str>) -> PyResult<Vec<String>> {
    let engine = code_highlighter(Some(highlighter.unwrap_or("syntect")))?
        .unwrap_or_else(rich::SyntectHighlighter::shared);
    Ok(engine.themes())
}

// ---------------------------------------------------------------------------
// Syntax

/// One `stylize_range` call.
#[derive(Clone)]
struct StyledRange {
    style: StyleType,
    start: (isize, isize),
    end: (isize, isize),
    before: bool,
}

/// Everything a render needs, copied out of the Python object.
#[derive(Clone)]
pub(crate) struct Spec {
    pub(crate) code: String,
    pub(crate) lexer: String,
    pub(crate) theme: Option<String>,
    pub(crate) dedent: bool,
    pub(crate) line_numbers: bool,
    pub(crate) start_line: isize,
    pub(crate) line_range: Option<(Option<isize>, Option<isize>)>,
    pub(crate) highlight_lines: HashSet<isize>,
    pub(crate) code_width: Option<usize>,
    pub(crate) tab_size: usize,
    pub(crate) word_wrap: bool,
    pub(crate) background_color: Option<CoreColor>,
    pub(crate) indent_guides: bool,
    pub(crate) padding: (usize, usize, usize, usize),
    pub(crate) highlighter: Option<Arc<dyn CodeHighlighter>>,
    ranges: Vec<StyledRange>,
}

impl Spec {
    pub(crate) fn new(code: String, lexer: String) -> Spec {
        Spec {
            code,
            lexer,
            theme: None,
            dedent: false,
            line_numbers: false,
            start_line: 1,
            line_range: None,
            highlight_lines: HashSet::new(),
            code_width: None,
            tab_size: 4,
            word_wrap: false,
            background_color: None,
            indent_guides: false,
            padding: (0, 0, 0, 0),
            highlighter: None,
            ranges: Vec::new(),
        }
    }

    /// Upstream's `stylize_range`.
    pub(crate) fn stylize_range(
        &mut self,
        style: StyleType,
        start: (isize, isize),
        end: (isize, isize),
    ) {
        self.ranges.push(StyledRange {
            style,
            start,
            end,
            before: false,
        });
    }

    /// Whether core's `Syntax` renders this as upstream would.
    fn is_plain(&self) -> bool {
        let (top, right, bottom, left) = self.padding;
        !self.line_numbers
            && !self.word_wrap
            && self.line_range.is_none()
            && !self.indent_guides
            && self.background_color.is_none()
            && self.ranges.is_empty()
            && self.code_width.is_none()
            && top == right
            && right == bottom
            && bottom == left
    }

    fn core(&self, code: &str) -> rich::Syntax {
        let mut syntax = rich::Syntax::new(code, self.lexer.as_str())
            .tab_size(self.tab_size)
            .word_wrap(self.word_wrap);
        if let Some(theme) = &self.theme {
            syntax = syntax.theme(theme.as_str());
        }
        if let Some(highlighter) = &self.highlighter {
            syntax = syntax.highlighter(highlighter.clone());
        }
        syntax
    }

    /// The engine and theme core would highlight with.
    fn engine(&self, console: Option<&CoreConsole>) -> (Arc<dyn CodeHighlighter>, String) {
        let default = match &self.highlighter {
            Some(_) => None,
            None => console.and_then(|console| console.code_highlighting()),
        };
        let engine = self
            .highlighter
            .clone()
            .or_else(|| default.map(|d| d.highlighter.clone()))
            .unwrap_or_else(rich::SyntectHighlighter::shared);
        let theme = self
            .theme
            .clone()
            .or_else(|| default.and_then(|d| d.theme.clone()))
            .filter(|theme| engine.themes().contains(theme))
            .unwrap_or_else(|| engine.default_theme().to_string());
        (engine, theme)
    }

    /// The theme's background (`None`: transparent) and default foreground.
    fn theme_colors(
        &self,
        console: Option<&CoreConsole>,
    ) -> (Option<CoreColor>, Option<CoreColor>) {
        let (engine, theme) = self.engine(console);
        let language = Some(self.lexer.as_str()).filter(|l| !l.is_empty());
        match engine.highlight("", language, &theme) {
            Ok(code) => (code.background, code.default_style.color().cloned()),
            Err(_) => (None, None),
        }
    }

    /// Upstream's `_get_base_style`: the theme background, overridden by
    /// `background_color`.
    fn base_style(&self, console: Option<&CoreConsole>) -> CoreStyle {
        let (background, _) = self.theme_colors(console);
        let mut style = CoreStyle::new();
        if let Some(background) = background {
            style = style.with_bgcolor(background);
        }
        if let Some(color) = &self.background_color {
            style = style.with_bgcolor(color.clone());
        }
        style
    }

    fn background_style(&self) -> CoreStyle {
        match &self.background_color {
            Some(color) => CoreStyle::new().with_bgcolor(color.clone()),
            None => CoreStyle::new(),
        }
    }

    /// Upstream's `_process_code`: whether the code ended with a newline,
    /// and the code ending with one, dedented if asked (tabs are expanded by
    /// core's highlighting).
    fn process_code(&self, py: Python<'_>) -> PyResult<(bool, String)> {
        let ends_on_nl = self.code.ends_with('\n');
        let mut code = if ends_on_nl {
            self.code.clone()
        } else {
            format!("{}\n", self.code)
        };
        if self.dedent {
            code = py
                .import("textwrap")?
                .call_method1("dedent", (code,))?
                .extract()?;
        }
        Ok((ends_on_nl, code))
    }

    /// Upstream's `Syntax.highlight(code, line_range)`.
    pub(crate) fn highlight(
        &self,
        console: Option<&CoreConsole>,
        code: &str,
        line_range: Option<(Option<isize>, Option<isize>)>,
    ) -> CoreText {
        // Pygments ends the code with a newline (`ensurenl`).
        let code = if code.ends_with('\n') {
            code.to_string()
        } else {
            format!("{code}\n")
        };
        let syntax = self.core(&code);
        let mut text = match console {
            Some(console) => syntax.highlight_for(console),
            None => syntax.highlight(),
        };
        let base = self.base_style(console);
        let transparent = base.bgcolor().is_none();
        text.set_base_style(base);
        text.set_justify(if transparent {
            Justify::Default
        } else {
            Justify::Left
        });
        text.set_no_wrap(Some(!self.word_wrap));
        if let Some((start, end)) = line_range {
            text = limit_lines(&text, start, end);
        }
        if let Some(color) = &self.background_color {
            let length = text.plain().len();
            text.stylize(CoreStyle::new().with_bgcolor(color.clone()), 0, length);
        }
        if !self.ranges.is_empty() {
            text = self.apply_ranges(text);
        }
        text
    }

    /// Upstream's `_apply_stylized_ranges`.
    fn apply_ranges(&self, text: CoreText) -> CoreText {
        let plain = text.plain().to_string();
        let offsets = char_offsets(&plain);
        let chars = offsets.len() - 1;
        // Character offsets of each line start, plus the end sentinel.
        let mut newlines = vec![0usize];
        newlines.extend(
            plain
                .chars()
                .enumerate()
                .filter(|(_, c)| *c == '\n')
                .map(|(i, _)| i + 1),
        );
        newlines.push(chars + 1);
        let index = |(line, column): (isize, isize)| -> Option<usize> {
            let count = newlines.len() as isize;
            if line > count || count < line + 1 || line < 1 {
                return None;
            }
            let line_index = (line - 1) as usize;
            let length = newlines[line_index + 1] - newlines[line_index] - 1;
            let column = column.clamp(0, length as isize) as usize;
            Some((newlines[line_index] + column).min(chars))
        };
        let mut before = Vec::new();
        let mut after = Vec::new();
        for range in &self.ranges {
            if let (Some(start), Some(end)) = (index(range.start), index(range.end)) {
                let span = (range.style.clone(), offsets[start], offsets[end]);
                if range.before {
                    before.push(span);
                } else {
                    after.push(span);
                }
            }
        }
        if before.is_empty() {
            let mut text = text;
            for (style, start, end) in after {
                text.stylize(style, start, end);
            }
            return text;
        }
        let mut rebuilt = text.blank_copy();
        rebuilt.append(&plain, None);
        for (style, start, end) in before.into_iter().rev() {
            rebuilt.stylize(style, start, end);
        }
        for span in text.spans() {
            rebuilt.stylize(span.style.clone(), span.start, span.end);
        }
        for (style, start, end) in after {
            rebuilt.stylize(style, start, end);
        }
        rebuilt
    }

    /// Upstream's `_numbers_column_width`.
    fn numbers_column_width(&self) -> usize {
        if !self.line_numbers {
            return 0;
        }
        let last = self.start_line + self.code.matches('\n').count() as isize;
        last.to_string().len() + NUMBERS_COLUMN_DEFAULT_PADDING
    }

    /// Upstream's `_get_number_styles`.
    fn number_styles(&self, console: &CoreConsole) -> (CoreStyle, CoreStyle, CoreStyle) {
        let background_style = self.base_style(Some(console));
        let dim = CoreStyle::parse("dim").expect("valid style");
        if background_style.bgcolor().is_none() {
            return (CoreStyle::new(), dim, CoreStyle::new());
        }
        if matches!(
            console.color_system(),
            Some(ColorSystem::EightBit | ColorSystem::Truecolor)
        ) {
            let (_, foreground) = self.theme_colors(Some(console));
            let blend = |cross_fade: f64| -> Option<CoreColor> {
                let background = background_style.bgcolor()?.get_truecolor()?;
                let foreground = foreground.as_ref()?.get_truecolor()?;
                let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * cross_fade) as u8;
                Some(CoreColor::from_rgb(
                    mix(background.red, foreground.red),
                    mix(background.green, foreground.green),
                    mix(background.blue, foreground.blue),
                ))
            };
            let mut text_style = background_style.clone();
            if let Some(color) = &foreground {
                text_style = text_style.with_color(color.clone());
            }
            let mut number = text_style.clone();
            if let Some(color) = blend(0.3) {
                number = number.with_color(color);
            }
            let mut highlight = text_style.combine(&CoreStyle::parse("bold").expect("valid style"));
            if let Some(color) = blend(0.9) {
                highlight = highlight.with_color(color);
            }
            let background = self.background_style();
            (
                background_style,
                number.combine(&background),
                highlight.combine(&background),
            )
        } else {
            let not_dim = CoreStyle::parse("not dim").expect("valid style");
            (
                background_style.clone(),
                background_style.combine(&dim),
                background_style.combine(&not_dim),
            )
        }
    }

    /// Upstream's `__rich_measure__`.
    fn measure(&self) -> CoreMeasurement {
        let (_, right, _, left) = self.padding;
        let padding = left + right;
        let numbers = self.numbers_column_width();
        if let Some(code_width) = self.code_width {
            return CoreMeasurement::new(numbers, code_width + numbers + padding + 1);
        }
        let widest = python_splitlines(&self.code)
            .into_iter()
            .map(rich::cells::cell_len)
            .max()
            .unwrap_or(0);
        let mut width = numbers + padding + widest;
        if self.line_numbers {
            width += 1;
        }
        CoreMeasurement::new(numbers, width)
    }

    /// Upstream's `_get_syntax`: the lines, before padding.
    fn lines(
        &self,
        py: Python<'_>,
        console: &CoreConsole,
        options: &CoreOptions,
    ) -> PyResult<Vec<Vec<CoreSegment>>> {
        let base_style = self.base_style(Some(console));
        let transparent = base_style.bgcolor().is_none();
        let (_, pad_right, _, pad_left) = self.padding;
        let horizontal_padding = pad_left + pad_right;
        let numbers_column_width = self.numbers_column_width();
        let code_width = match self.code_width {
            Some(width) => width,
            None => {
                let width = if self.line_numbers {
                    options.max_width as isize - numbers_column_width as isize - 1
                } else {
                    options.max_width as isize
                };
                (width - horizontal_padding as isize).max(0) as usize
            }
        };
        let (ends_on_nl, code) = self.process_code(py)?;
        let mut text = self.highlight(Some(console), &code, self.line_range);
        let dim = CoreStyle::parse("dim").expect("valid style");
        let guide_style = base_style.combine(&dim).combine(&self.background_style());

        if !self.line_numbers && !self.word_wrap && self.line_range.is_none() {
            if !ends_on_nl && text.plain().ends_with('\n') {
                let length = text.plain().len();
                text = text.divide(&[length - 1]).swap_remove(0);
            }
            if self.indent_guides && !console.ascii_only() {
                text = with_indent_guides(&text, self.tab_size, StyleType::Style(guide_style));
                text.set_overflow(Some(Overflow::Crop));
            }
            let mut render_options = options.update_width(code_width);
            if transparent {
                // `Console.render(text)`: every hard line, a trailing empty
                // one included.
                let mut lines = console.render_lines_styled(&text, &render_options, None, false);
                let hard_lines = text.plain().split('\n').count();
                while lines.len() < hard_lines {
                    lines.push(Vec::new());
                }
                return Ok(lines);
            }
            render_options.height = None;
            render_options.justify = Justify::Left;
            let background = self.background_style();
            return Ok(console.render_lines_styled(
                &text,
                &render_options,
                Some(&background),
                true,
            ));
        }

        let (start_line, end_line) = self.line_range.unwrap_or((None, None));
        let line_offset = match start_line {
            Some(start) if start != 0 => (start - 1).max(0) as usize,
            _ => 0,
        };
        let mut lines = text.split("\n", false, ends_on_nl);
        if self.line_range.is_some() {
            if line_offset > lines.len() {
                return Ok(Vec::new());
            }
            let end = match end_line {
                None => lines.len(),
                Some(end) if end < 0 => (lines.len() as isize + end).max(0) as usize,
                Some(end) => (end as usize).min(lines.len()),
            };
            lines = if line_offset < end {
                lines[line_offset..end].to_vec()
            } else {
                Vec::new()
            };
        }
        if self.indent_guides && !console.ascii_only() {
            let italic_off = CoreStyle::parse("not italic").expect("valid style");
            let joined = CoreText::new("\n").join(&lines);
            lines = with_indent_guides(
                &joined,
                self.tab_size,
                StyleType::Style(guide_style.combine(&italic_off)),
            )
            .split("\n", false, true);
        }

        let mut render_options = options.update_width(code_width);
        render_options.height = None;
        let pointer = if console.legacy_windows() {
            "> "
        } else {
            "❱ "
        };
        let (background_style, number_style, highlight_number_style) = self.number_styles(console);
        let pad_style = Some(background_style.clone()).filter(|s| !s.is_null());
        let mut output = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            let line_no = self.start_line + line_offset as isize + index as isize;
            let wrapped_lines: Vec<Vec<CoreSegment>> = if self.word_wrap {
                // Upstream asks for `justify="left"`, but a line of a
                // transparent theme's text says `"default"`, which wins.
                let mut wrap_options = render_options.clone();
                wrap_options.justify = if transparent {
                    Justify::Default
                } else {
                    Justify::Left
                };
                console.render_lines_styled(
                    line,
                    &wrap_options,
                    Some(&background_style),
                    !transparent,
                )
            } else {
                let segments: Vec<CoreSegment> = line
                    .render(console.theme(), &CoreStyle::new())
                    .into_iter()
                    .filter(|segment| segment.text != "\n")
                    .collect();
                if options.no_wrap == Some(true) {
                    vec![segments]
                } else {
                    vec![adjust_line_length(
                        &segments,
                        render_options.max_width,
                        pad_style.clone(),
                        !transparent,
                    )]
                }
            };
            if self.line_numbers {
                let left_pad = CoreSegment::new(
                    " ".repeat(numbers_column_width + 1),
                    Some(background_style.clone()),
                );
                for (first, wrapped) in wrapped_lines.into_iter().enumerate() {
                    let mut row = Vec::new();
                    if first == 0 {
                        let column = format!(
                            "{:>width$} ",
                            line_no,
                            width = numbers_column_width.saturating_sub(2)
                        );
                        if self.highlight_lines.contains(&line_no) {
                            row.push(CoreSegment::new(
                                pointer,
                                Some(CoreStyle::parse("red").expect("valid style")),
                            ));
                            row.push(CoreSegment::new(
                                column,
                                Some(highlight_number_style.clone()),
                            ));
                        } else {
                            row.push(CoreSegment::new("  ", Some(highlight_number_style.clone())));
                            row.push(CoreSegment::new(column, Some(number_style.clone())));
                        }
                    } else {
                        row.push(left_pad.clone());
                    }
                    row.extend(wrapped);
                    output.push(row);
                }
            } else {
                output.extend(wrapped_lines);
            }
        }
        Ok(output)
    }
}

/// Upstream's `Padding(Segments(lines), style=style, pad=padding)` at
/// `width`: every line fitted to the inner width, then framed. (Core's
/// `Padding` re-splits its child's output, which loses a trailing blank
/// line.)
fn pad_lines(
    lines: Vec<Vec<CoreSegment>>,
    (top, right, bottom, left): (usize, usize, usize, usize),
    style: &CoreStyle,
    width: usize,
) -> Vec<Vec<CoreSegment>> {
    if top + right + bottom + left == 0 {
        return lines;
    }
    let style = Some(style.clone()).filter(|style| !style.is_null());
    let inner = width.saturating_sub(left + right);
    let blank = vec![CoreSegment::new(" ".repeat(width), style.clone())];
    let mut padded = vec![blank.clone(); top];
    for line in lines {
        let line = match &style {
            Some(style) => CoreSegment::apply_style(&line, style),
            None => line,
        };
        let mut row = Vec::new();
        if left > 0 {
            row.push(CoreSegment::new(" ".repeat(left), style.clone()));
        }
        row.extend(CoreSegment::adjust_line_length(&line, inner, style.clone()));
        if right > 0 {
            row.push(CoreSegment::new(" ".repeat(right), style.clone()));
        }
        padded.push(row);
    }
    padded.extend(vec![blank; bottom]);
    padded
}

/// `Segment.adjust_line_length(line, length, style, pad)`.
fn adjust_line_length(
    line: &[CoreSegment],
    length: usize,
    style: Option<CoreStyle>,
    pad: bool,
) -> Vec<CoreSegment> {
    let width: usize = line.iter().map(CoreSegment::cell_length).sum();
    if width < length && !pad {
        return line.to_vec();
    }
    CoreSegment::adjust_line_length(line, length, style)
}

/// The part of a highlighted text upstream's `highlight(code, line_range)`
/// keeps: tokens before the first line lose their style, and nothing after
/// the last line is kept.
fn limit_lines(text: &CoreText, start: Option<isize>, end: Option<isize>) -> CoreText {
    let plain = text.plain();
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(plain.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let first = match start {
        Some(start) if start > 0 => (start - 1) as usize,
        _ => 0,
    };
    let styled_from = line_starts.get(first).copied().unwrap_or(plain.len());
    let cut = match end {
        Some(end) if end > 0 => line_starts
            .get(first.max(end as usize))
            .copied()
            .unwrap_or(plain.len()),
        _ => plain.len(),
    };
    let mut limited = text.blank_copy();
    limited.append(&plain[..cut], None);
    for span in text.spans() {
        let (span_start, span_end) = (span.start.max(styled_from), span.end.min(cut));
        if span_start < span_end {
            limited.stylize(span.style.clone(), span_start, span_end);
        }
    }
    limited
}

/// Python's `str.splitlines()`.
fn python_splitlines(text: &str) -> Vec<&str> {
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

/// A `Syntax` rendered by the port of upstream's layout.
pub(crate) struct Render {
    pub(crate) spec: Spec,
}

impl Renderable for Render {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let spec = &self.spec;
        // Core pads every line, as upstream does only for a theme with a
        // background; a transparent theme (`ansi_dark`) takes the port below.
        if spec.is_plain() && spec.base_style(Some(console)).bgcolor().is_some() {
            let mut code = spec.code.clone();
            if spec.dedent {
                if let Ok((_, dedented)) = Python::attach(|py| spec.process_code(py)) {
                    code = if spec.code.ends_with('\n') {
                        dedented
                    } else {
                        dedented.trim_end_matches('\n').to_string()
                    };
                }
            }
            return spec
                .core(&code)
                .padding(spec.padding.0)
                .rich_render(console, options);
        }
        let lines = match Python::attach(|py| spec.lines(py, console, options)) {
            Ok(lines) => lines,
            Err(_) => return Vec::new(),
        };
        join_lines(pad_lines(
            lines,
            spec.padding,
            &spec.base_style(Some(console)),
            options.max_width,
        ))
    }

    fn measure(&self, _console: &CoreConsole, _options: &CoreOptions) -> CoreMeasurement {
        self.spec.measure()
    }
}

/// `rich.syntax.Syntax`: syntax-highlighted code.
#[pyclass(name = "Syntax", module = "rs_rich.syntax")]
pub(crate) struct Syntax {
    pub(crate) spec: Spec,
    highlighter_arg: Option<Py<PyAny>>,
    background_name: Option<String>,
}

fn color(value: Option<&str>) -> PyResult<Option<CoreColor>> {
    value
        .map(|name| {
            CoreColor::parse(name)
                .map_err(|e| crate::errors::StyleSyntaxError::new_err(e.to_string()))
        })
        .transpose()
}

fn theme_name(theme: Option<&Bound<'_, PyAny>>) -> PyResult<Option<String>> {
    match theme {
        None => Ok(Some(DEFAULT_THEME.to_string())),
        Some(theme) if theme.is_none() => Ok(Some(DEFAULT_THEME.to_string())),
        Some(theme) => theme.extract::<String>().map(Some).map_err(|_| {
            PyTypeError::new_err(
                "theme must be a theme name: rs_rich has no Pygments SyntaxTheme objects",
            )
        }),
    }
}

fn lexer_name(lexer: &Bound<'_, PyAny>) -> PyResult<String> {
    if let Ok(name) = lexer.cast::<PyString>() {
        return Ok(name.to_string());
    }
    // A Pygments lexer object: use its first alias.
    if let Some(aliases) = lexer.getattr_opt("aliases")? {
        if let Ok(Some(first)) = aliases.try_iter().map(|mut i| i.next()) {
            return first?.extract();
        }
    }
    Err(PyTypeError::new_err("lexer must be a lexer name"))
}

fn line_range(
    value: Option<&Bound<'_, PyAny>>,
) -> PyResult<Option<(Option<isize>, Option<isize>)>> {
    match value.filter(|v| !v.is_none()) {
        None => Ok(None),
        Some(value) => Ok(Some(value.extract()?)),
    }
}

impl Syntax {
    #[allow(clippy::too_many_arguments)]
    fn build(
        code: String,
        lexer: &Bound<'_, PyAny>,
        theme: Option<&Bound<'_, PyAny>>,
        dedent: bool,
        line_numbers: bool,
        start_line: isize,
        range: Option<&Bound<'_, PyAny>>,
        highlight_lines: Option<HashSet<isize>>,
        code_width: Option<usize>,
        tab_size: usize,
        word_wrap: bool,
        background_color: Option<String>,
        indent_guides: bool,
        padding: &Bound<'_, PyAny>,
        highlighter: Option<Py<PyAny>>,
    ) -> PyResult<Syntax> {
        let mut spec = Spec::new(code, lexer_name(lexer)?);
        spec.theme = theme_name(theme)?;
        spec.dedent = dedent;
        spec.line_numbers = line_numbers;
        spec.start_line = start_line;
        spec.line_range = line_range(range)?;
        spec.highlight_lines = highlight_lines.unwrap_or_default();
        spec.code_width = code_width;
        spec.tab_size = tab_size;
        spec.word_wrap = word_wrap;
        spec.background_color = color(background_color.as_deref())?;
        spec.indent_guides = indent_guides;
        spec.padding = convert::padding(padding)?;
        spec.highlighter =
            code_highlighter_value(highlighter.as_ref().map(|h| h.bind(lexer.py())))?;
        Ok(Syntax {
            spec,
            highlighter_arg: highlighter,
            background_name: background_color,
        })
    }
}

impl AsRenderable for Syntax {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Render {
            spec: self.spec.clone(),
        }))
    }
}

/// Upstream's `Syntax.guess_lexer`, answered by the default engine: the
/// language for the path's name or extension, lower-cased, else `"default"`.
fn guess(path: &str) -> String {
    rich::SyntectHighlighter::shared()
        .language_for_path(std::path::Path::new(path))
        .map(|name| name.to_lowercase())
        .unwrap_or_else(|| "default".to_string())
}

#[pymethods]
impl Syntax {
    #[new]
    #[pyo3(signature = (
        code, lexer, *, theme=None, dedent=false, line_numbers=false, start_line=1,
        line_range=None, highlight_lines=None, code_width=None, tab_size=4, word_wrap=false,
        background_color=None, indent_guides=false, padding=None, highlighter=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        code: String,
        lexer: &Bound<'_, PyAny>,
        theme: Option<&Bound<'_, PyAny>>,
        dedent: bool,
        line_numbers: bool,
        start_line: isize,
        line_range: Option<&Bound<'_, PyAny>>,
        highlight_lines: Option<HashSet<isize>>,
        code_width: Option<usize>,
        tab_size: usize,
        word_wrap: bool,
        background_color: Option<String>,
        indent_guides: bool,
        padding: Option<&Bound<'_, PyAny>>,
        highlighter: Option<Py<PyAny>>,
    ) -> PyResult<Self> {
        let zero = 0i32.into_pyobject(py)?.into_any();
        Syntax::build(
            code,
            lexer,
            theme,
            dedent,
            line_numbers,
            start_line,
            line_range,
            highlight_lines,
            code_width,
            tab_size,
            word_wrap,
            background_color,
            indent_guides,
            padding.unwrap_or(&zero),
            highlighter,
        )
    }

    /// Build a `Syntax` from a file; the lexer is guessed from the path when
    /// not given.
    #[classmethod]
    #[pyo3(signature = (
        path, encoding="utf-8", lexer=None, theme=None, dedent=false, line_numbers=false,
        line_range=None, start_line=1, highlight_lines=None, code_width=None, tab_size=4,
        word_wrap=false, background_color=None, indent_guides=false, padding=None,
        highlighter=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn from_path(
        cls: &Bound<'_, pyo3::types::PyType>,
        path: &Bound<'_, PyAny>,
        encoding: &str,
        lexer: Option<&Bound<'_, PyAny>>,
        theme: Option<&Bound<'_, PyAny>>,
        dedent: bool,
        line_numbers: bool,
        line_range: Option<&Bound<'_, PyAny>>,
        start_line: isize,
        highlight_lines: Option<HashSet<isize>>,
        code_width: Option<usize>,
        tab_size: usize,
        word_wrap: bool,
        background_color: Option<String>,
        indent_guides: bool,
        padding: Option<&Bound<'_, PyAny>>,
        highlighter: Option<Py<PyAny>>,
    ) -> PyResult<Self> {
        let py = cls.py();
        let path_str: String = py
            .import("os")?
            .call_method1("fspath", (path,))?
            .str()?
            .to_string();
        let code: String = py
            .import("pathlib")?
            .getattr("Path")?
            .call1((path,))?
            .call_method1("read_text", (encoding,))?
            .extract()?;
        let guessed;
        let lexer = match lexer.filter(|l| !l.is_none() && l.is_truthy().unwrap_or(false)) {
            Some(lexer) => lexer.clone(),
            None => {
                guessed = PyString::new(py, &guess(&path_str)).into_any();
                guessed
            }
        };
        let zero = 0i32.into_pyobject(py)?.into_any();
        Syntax::build(
            code,
            &lexer,
            theme,
            dedent,
            line_numbers,
            start_line,
            line_range,
            highlight_lines,
            code_width,
            tab_size,
            word_wrap,
            background_color,
            indent_guides,
            padding.unwrap_or(&zero),
            highlighter,
        )
    }

    /// Guess a lexer name from a path (`code` is accepted for Rich's
    /// signature; the default engine decides by the name alone).
    #[classmethod]
    #[pyo3(signature = (path, code=None))]
    fn guess_lexer(
        _cls: &Bound<'_, pyo3::types::PyType>,
        path: &Bound<'_, PyAny>,
        code: Option<&str>,
    ) -> PyResult<String> {
        let _ = code;
        let path: String = path.str()?.to_string();
        Ok(guess(&path))
    }

    /// The theme name the default engine will use for `name` (its own
    /// default when it has no theme of that name). Rich returns a
    /// `SyntaxTheme` object; rs_rich themes are names.
    #[classmethod]
    #[pyo3(name = "get_theme")]
    fn theme_for(_cls: &Bound<'_, pyo3::types::PyType>, name: &str) -> String {
        let engine = rich::SyntectHighlighter::shared();
        if engine.themes().iter().any(|theme| theme == name) {
            name.to_string()
        } else {
            engine.default_theme().to_string()
        }
    }

    /// `Syntax.highlight(code, line_range=None)`: the highlighted `Text`.
    #[pyo3(signature = (code, line_range=None))]
    fn highlight<'py>(
        &self,
        py: Python<'py>,
        code: &str,
        line_range: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, Text>> {
        let range = self::line_range(line_range)?;
        new_text(py, self.spec.highlight(None, code, range))
    }

    /// Style a part of the code (lines from 1, columns from 0) when it
    /// renders.
    #[pyo3(signature = (style, start, end, style_before=false))]
    fn stylize_range(
        &mut self,
        style: &Bound<'_, PyAny>,
        start: (isize, isize),
        end: (isize, isize),
        style_before: bool,
    ) -> PyResult<()> {
        let Some(style) = style_type(Some(style))? else {
            return Ok(());
        };
        self.spec.ranges.push(StyledRange {
            style,
            start,
            end,
            before: style_before,
        });
        Ok(())
    }

    #[getter]
    fn code(&self) -> &str {
        &self.spec.code
    }
    #[setter]
    fn set_code(&mut self, code: String) {
        self.spec.code = code;
    }
    #[getter]
    fn lexer(&self) -> &str {
        &self.spec.lexer
    }
    #[getter(theme)]
    fn theme_attr(&self) -> Option<&str> {
        self.spec.theme.as_deref()
    }
    #[getter]
    fn dedent(&self) -> bool {
        self.spec.dedent
    }
    #[setter]
    fn set_dedent(&mut self, value: bool) {
        self.spec.dedent = value;
    }
    #[getter]
    fn line_numbers(&self) -> bool {
        self.spec.line_numbers
    }
    #[setter]
    fn set_line_numbers(&mut self, value: bool) {
        self.spec.line_numbers = value;
    }
    #[getter]
    fn start_line(&self) -> isize {
        self.spec.start_line
    }
    #[setter]
    fn set_start_line(&mut self, value: isize) {
        self.spec.start_line = value;
    }
    #[getter(line_range)]
    fn get_line_range(&self) -> Option<(Option<isize>, Option<isize>)> {
        self.spec.line_range
    }
    #[setter(line_range)]
    fn set_line_range(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.spec.line_range = line_range(value)?;
        Ok(())
    }
    #[getter]
    fn highlight_lines(&self) -> HashSet<isize> {
        self.spec.highlight_lines.clone()
    }
    #[setter]
    fn set_highlight_lines(&mut self, value: Option<HashSet<isize>>) {
        self.spec.highlight_lines = value.unwrap_or_default();
    }
    #[getter]
    fn code_width(&self) -> Option<usize> {
        self.spec.code_width
    }
    #[setter]
    fn set_code_width(&mut self, value: Option<usize>) {
        self.spec.code_width = value;
    }
    #[getter]
    fn tab_size(&self) -> usize {
        self.spec.tab_size
    }
    #[setter]
    fn set_tab_size(&mut self, value: usize) {
        self.spec.tab_size = value;
    }
    #[getter]
    fn word_wrap(&self) -> bool {
        self.spec.word_wrap
    }
    #[setter]
    fn set_word_wrap(&mut self, value: bool) {
        self.spec.word_wrap = value;
    }
    #[getter]
    fn background_color(&self) -> Option<&str> {
        self.background_name.as_deref()
    }
    #[getter]
    fn indent_guides(&self) -> bool {
        self.spec.indent_guides
    }
    #[setter]
    fn set_indent_guides(&mut self, value: bool) {
        self.spec.indent_guides = value;
    }
    #[getter]
    fn padding(&self) -> (usize, usize, usize, usize) {
        self.spec.padding
    }
    #[setter]
    fn set_padding(&mut self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        self.spec.padding = convert::padding(value)?;
        Ok(())
    }
    /// The code highlighter's name (`None`: the console's default, else
    /// syntect). Not in Rich.
    #[getter]
    fn highlighter(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.highlighter_arg.as_ref().map(|h| h.clone_ref(py))
    }

    fn __rich_measure__(
        &self,
        _console: &Bound<'_, PyAny>,
        _options: &Bound<'_, PyAny>,
    ) -> crate::protocol::Measurement {
        crate::protocol::Measurement::from_core(self.spec.measure())
    }

    fn __repr__(&self) -> String {
        format!("<Syntax lexer={:?}>", self.spec.lexer)
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Syntax>(m)?;
    m.add_function(wrap_pyfunction!(code_highlighters, m)?)?;
    m.add_function(wrap_pyfunction!(code_themes, m)?)?;
    Ok(())
}
