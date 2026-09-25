//! `rich.rule`: `Rule`. Port of upstream `rich/rule.py`.
//!
//! Core's `Rule` has no `Text` titles, no `end` and no ASCII fallback, so the
//! renderer is ported here in full.

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyString;
use pyo3::{PyTraverseError, PyVisit};

use rich::cells::cell_len;
use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions, Overflow};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::style::StyleType;
use rich::text::DEFAULT_TAB_SIZE;
use rich::Text as CoreText;

use crate::errors::MarkupError;
use crate::renderable::{self, AsRenderable};
use crate::style::style_type;
use crate::text::Text;

use super::ascii_only;

/// A rule's title, as given.
enum Title {
    Markup(String),
    Text(CoreText),
}

struct RuleRender {
    title: Option<Title>,
    characters: String,
    style: StyleType,
    end: String,
    align: String,
}

/// `Text(string, style)` with `plain = set_cell_size(plain, width)` after a
/// `truncate(width)`.
fn sized(plain: &str, style: &StyleType, width: usize) -> CoreText {
    let mut text = CoreText::styled(plain, style.clone());
    text.truncate(width, Some(Overflow::Crop), true);
    text
}

/// `text.append(plain, style)`.
fn append(text: &mut CoreText, plain: &str, style: Option<&StyleType>) {
    text.append(plain, style.cloned());
}

impl RuleRender {
    /// `Rule._rule_line`: note upstream fills with `self.characters` here,
    /// not the ASCII substitute.
    fn rule_line(&self, chars_len: usize, width: usize) -> CoreText {
        let repeated = self.characters.repeat(width / chars_len.max(1) + 1);
        sized(&repeated, &self.style, width)
    }

    fn title_text(&self, console: &CoreConsole) -> Option<CoreText> {
        let parsed = match self.title.as_ref()? {
            Title::Text(text) => text.clone(),
            Title::Markup(markup) => {
                // `console.render_str(title, style="rule.text")`: highlighting
                // builds a new `Text`, which drops the `style=`.
                let highlight = renderable::ambient().map_or(true, |ambient| ambient.highlight);
                let mut text = console.render_str(markup, Some(highlight));
                if !highlight {
                    text.set_base_style("rule.text");
                }
                text
            }
        };
        if parsed.plain().is_empty() {
            return None;
        }
        // `title_text.plain = title_text.plain.replace("\n", " ")`.
        let mut title = parsed.blank_copy();
        title.append(&parsed.plain().replace('\n', " "), None);
        for span in parsed.spans() {
            title.stylize(span.style.clone(), span.start, span.end);
        }
        title.expand_tabs(DEFAULT_TAB_SIZE);
        Some(title)
    }

    fn build(&self, console: &CoreConsole, width: usize) -> CoreText {
        let characters: &str = if ascii_only(console) && !self.characters.is_ascii() {
            "-"
        } else {
            &self.characters
        };
        let chars_len = cell_len(characters).max(1);
        let Some(mut title) = self.title_text(console) else {
            return self.rule_line(chars_len, width);
        };
        let required_space = if self.align == "center" { 4 } else { 2 };
        let truncate_width = width.saturating_sub(required_space);
        if truncate_width == 0 {
            return self.rule_line(chars_len, width);
        }
        let style = Some(&self.style);
        let mut rule = CoreText::new("");
        title.truncate(truncate_width, Some(Overflow::Ellipsis), false);
        match self.align.as_str() {
            "center" => {
                let title_len = cell_len(title.plain());
                let side_width = (width - title_len) / 2;
                let fill = characters.repeat(side_width / chars_len + 1);
                let mut left = CoreText::new(fill.clone());
                left.truncate(side_width.saturating_sub(1), None, false);
                let right_length = width
                    .saturating_sub(cell_len(left.plain()))
                    .saturating_sub(title_len);
                let mut right = CoreText::new(fill);
                right.truncate(right_length, None, false);
                append(&mut rule, &format!("{} ", left.plain()), style);
                rule = rule.append_text(&title);
                append(&mut rule, &format!(" {}", right.plain()), style);
            }
            "left" => {
                rule = rule.append_text(&title);
                rule.append(" ", None);
                let count = width.saturating_sub(rule.cell_len());
                append(&mut rule, &characters.repeat(count), style);
            }
            _ => {
                let count = width.saturating_sub(title.cell_len()).saturating_sub(1);
                append(&mut rule, &characters.repeat(count), style);
                rule.append(" ", None);
                rule = rule.append_text(&title);
            }
        }
        // `rule_text.plain = set_cell_size(rule_text.plain, width)`.
        rule.truncate(width, Some(Overflow::Crop), true);
        rule
    }
}

impl Renderable for RuleRender {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let text = self.build(console, options.max_width);
        let mut segments = text.rich_render(console, options);
        // The rule `Text`'s `end`, in core's convention (the last newline is
        // implied). Upstream's rule without a title ends with a newline.
        let titled = match &self.title {
            Some(Title::Markup(markup)) => !markup.is_empty(),
            Some(Title::Text(text)) => !text.plain().is_empty(),
            None => false,
        };
        let end = if titled { &self.end } else { "\n" };
        if !end.is_empty() {
            segments.push(CoreSegment::new(end, None));
        }
        renderable::unterminated(segments)
    }

    fn measure(&self, _console: &CoreConsole, _options: &CoreOptions) -> CoreMeasurement {
        CoreMeasurement::new(1, 1)
    }
}

fn check_align(align: &str) -> PyResult<()> {
    if !matches!(align, "left" | "center" | "right") {
        return Err(PyValueError::new_err(format!(
            "invalid value for align, expected \"left\", \"center\", \"right\" (not '{align}')"
        )));
    }
    Ok(())
}

fn check_characters(characters: &str) -> PyResult<()> {
    if cell_len(characters) < 1 {
        return Err(PyValueError::new_err(
            "'characters' argument must have a cell width of at least 1",
        ));
    }
    Ok(())
}

/// `rich.rule.Rule`: a horizontal line, optionally with a title.
#[pyclass(name = "Rule", module = "rs_rich.rule")]
pub(crate) struct Rule {
    title: Py<PyAny>,
    characters: String,
    style: Py<PyAny>,
    #[pyo3(get, set)]
    end: String,
    align: String,
}

impl AsRenderable for Rule {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let title = self.title.bind(py);
        let title = if let Ok(text) = title.extract::<PyRef<'_, Text>>() {
            Some(Title::Text(text.inner.clone()))
        } else if let Ok(markup) = title.cast::<PyString>() {
            let markup = markup.to_cow()?.into_owned();
            CoreText::from_markup(&markup).map_err(|e| MarkupError::new_err(e.to_string()))?;
            Some(Title::Markup(markup))
        } else if title.is_none() || !title.is_truthy()? {
            None
        } else {
            return Err(PyTypeError::new_err("a Rule title must be a str or a Text"));
        };
        let style = style_type(Some(self.style.bind(py)))?
            .unwrap_or_else(|| StyleType::Style(Default::default()));
        Ok(Box::new(RuleRender {
            title,
            characters: self.characters.clone(),
            style,
            end: self.end.clone(),
            align: self.align.clone(),
        }))
    }
}

#[pymethods]
impl Rule {
    #[new]
    #[pyo3(signature = (title=None, *, characters="─", style=None, end="\n", align="center"))]
    fn new(
        py: Python<'_>,
        title: Option<Py<PyAny>>,
        characters: &str,
        style: Option<Py<PyAny>>,
        end: &str,
        align: &str,
    ) -> PyResult<Self> {
        check_characters(characters)?;
        check_align(align)?;
        let style = style.unwrap_or_else(|| PyString::new(py, "rule.line").into_any().unbind());
        style_type(Some(style.bind(py)))?;
        Ok(Rule {
            title: title.unwrap_or_else(|| PyString::new(py, "").into_any().unbind()),
            characters: characters.to_string(),
            style,
            end: end.to_string(),
            align: align.to_string(),
        })
    }

    #[getter]
    fn title(&self, py: Python<'_>) -> Py<PyAny> {
        self.title.clone_ref(py)
    }

    #[setter]
    fn set_title(&mut self, title: Py<PyAny>) {
        self.title = title;
    }

    #[getter]
    fn characters(&self) -> String {
        self.characters.clone()
    }

    #[setter]
    fn set_characters(&mut self, characters: &str) -> PyResult<()> {
        check_characters(characters)?;
        self.characters = characters.to_string();
        Ok(())
    }

    #[getter]
    fn style(&self, py: Python<'_>) -> Py<PyAny> {
        self.style.clone_ref(py)
    }

    #[setter]
    fn set_style(&mut self, style: Bound<'_, PyAny>) -> PyResult<()> {
        style_type(Some(&style))?;
        self.style = style.unbind();
        Ok(())
    }

    #[getter]
    fn align(&self) -> String {
        self.align.clone()
    }

    #[setter]
    fn set_align(&mut self, align: &str) -> PyResult<()> {
        check_align(align)?;
        self.align = align.to_string();
        Ok(())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Rule({}, {})",
            self.title.bind(py).repr()?,
            PyString::new(py, &self.characters).repr()?
        ))
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.title)?;
        visit.call(&self.style)
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Rule>(m)
}

/// Whether `value` is a titled `Rule` whose `end` does not end its line
/// (Rich then prints nothing after it: `Rule("x", end="")` shares its line
/// with what follows). Rich's rule without a title ignores `end`.
pub(crate) fn ends_inline(value: &Bound<'_, PyAny>) -> bool {
    let Ok(rule) = value.cast::<Rule>() else {
        return false;
    };
    let rule = rule.borrow();
    !rule.end.ends_with('\n') && rule.title.bind(value.py()).is_truthy().unwrap_or(false)
}
