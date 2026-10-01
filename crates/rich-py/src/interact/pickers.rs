//! `TextArea`, `FilePicker`, `ColorPicker` and `AssetPicker` for
//! `rs_rich.interact` (#493): configurations built into a fresh
//! `rich_interact` component per run, like the other component classes.

use std::path::PathBuf;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};

use rich_interact::{
    AssetKind, AssetPicker as CoreAsset, ColorFormat, ColorPicker as CoreColor, FileMode,
    FilePicker as CoreFile, Key, TextArea as CoreText,
};

use super::compose::{self, Node};
use super::{execute, record, Build, Mode, Record};

fn key(name: &str) -> PyResult<Key> {
    Key::parse(name).ok_or_else(|| PyValueError::new_err(format!("unknown key name {name:?}")))
}

fn positive(value: usize, what: &str) -> PyResult<usize> {
    if value == 0 {
        return Err(PyValueError::new_err(format!("{what} must be at least 1")));
    }
    Ok(value)
}

// ---------------------------------------------------------------------------
// TextArea

/// `TextArea(prompt="Write", *, value="", placeholder=None, char_limit=None,
/// height=5, line_numbers=False, submit="ctrl+d")`: several lines of text.
/// Enter starts a line, `submit` finishes, Escape cancels. The answer is
/// the text, lines joined with `"\n"`. Without a terminal, every line of
/// input up to its end is the text.
#[pyclass(name = "TextArea", module = "rs_rich.interact", frozen)]
pub(crate) struct TextArea {
    #[pyo3(get)]
    prompt: String,
    #[pyo3(get)]
    value: String,
    #[pyo3(get)]
    placeholder: Option<String>,
    #[pyo3(get)]
    char_limit: Option<usize>,
    #[pyo3(get)]
    height: usize,
    #[pyo3(get)]
    line_numbers: bool,
    #[pyo3(get)]
    submit: String,
    parsed: Key,
}

struct TextBuild {
    prompt: String,
    value: String,
    placeholder: Option<String>,
    char_limit: Option<usize>,
    height: usize,
    line_numbers: bool,
    submit: Key,
}

impl Build for TextBuild {
    type C = CoreText;

    fn build(self) -> CoreText {
        let mut area = CoreText::new(self.prompt)
            .height(self.height)
            .line_numbers(self.line_numbers)
            .submit_key(self.submit);
        if let Some(limit) = self.char_limit {
            area = area.char_limit(limit);
        }
        if let Some(placeholder) = self.placeholder {
            area = area.placeholder(placeholder);
        }
        area.value(self.value)
    }
}

#[pymethods]
impl TextArea {
    #[new]
    #[pyo3(signature = (
        prompt="Write".to_string(), *, value=String::new(), placeholder=None, char_limit=None,
        height=5, line_numbers=false, submit="ctrl+d".to_string()
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        prompt: String,
        value: String,
        placeholder: Option<String>,
        char_limit: Option<usize>,
        height: usize,
        line_numbers: bool,
        submit: String,
    ) -> PyResult<Self> {
        if let Some(limit) = char_limit {
            positive(limit, "char_limit")?;
        }
        Ok(TextArea {
            prompt,
            value,
            placeholder,
            char_limit,
            height: positive(height, "height")?,
            line_numbers,
            parsed: key(&submit)?,
            submit,
        })
    }

    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }
}

impl TextArea {
    fn prepare(&self) -> TextBuild {
        TextBuild {
            prompt: self.prompt.clone(),
            value: self.value.clone(),
            placeholder: self.placeholder.clone(),
            char_limit: self.char_limit,
            height: self.height,
            line_numbers: self.line_numbers,
            submit: self.parsed,
        }
    }
}

// ---------------------------------------------------------------------------
// FilePicker

fn file_mode(name: &str) -> PyResult<FileMode> {
    Ok(match name {
        "file" => FileMode::File,
        "directory" => FileMode::Directory,
        "both" => FileMode::Both,
        other => {
            return Err(PyValueError::new_err(format!(
                "invalid mode {other:?}; expected file, directory or both"
            )))
        }
    })
}

/// `FilePicker(root=".", *, prompt="File", mode="file", extensions=(),
/// hidden=False, jail=False, default=None, query="", height=10,
/// mouse=False)`: browse from `root` and pick a path. Typing filters,
/// Right opens a directory, Left goes up, Ctrl+T shows hidden files.
/// `mode` is what may be picked (`file`, `directory` or `both`); with
/// `jail`, nothing outside `root` is listed, opened or read. The answer is
/// the path (a `pathlib.Path`); without a terminal, `default`.
#[pyclass(name = "FilePicker", module = "rs_rich.interact", frozen)]
pub(crate) struct FilePicker {
    #[pyo3(get)]
    root: PathBuf,
    #[pyo3(get)]
    prompt: String,
    #[pyo3(get)]
    mode: String,
    #[pyo3(get)]
    extensions: Vec<String>,
    #[pyo3(get)]
    hidden: bool,
    #[pyo3(get)]
    jail: bool,
    #[pyo3(get)]
    default: Option<PathBuf>,
    #[pyo3(get)]
    query: String,
    #[pyo3(get)]
    height: usize,
    #[pyo3(get)]
    mouse: bool,
    parsed: FileMode,
}

struct FileBuild {
    root: PathBuf,
    prompt: String,
    mode: FileMode,
    extensions: Vec<String>,
    hidden: bool,
    jail: bool,
    default: Option<PathBuf>,
    query: String,
    height: usize,
    mouse: bool,
}

impl Build for FileBuild {
    type C = CoreFile;

    fn build(self) -> CoreFile {
        let mut picker = CoreFile::new(self.prompt, self.root)
            .mode(self.mode)
            .extensions(&self.extensions)
            .show_hidden(self.hidden)
            .jail(self.jail)
            .height(self.height)
            .with_mouse(self.mouse);
        if let Some(default) = self.default {
            picker = picker.default(default);
        }
        if !self.query.is_empty() {
            picker = picker.query(self.query);
        }
        picker
    }

    fn action(component: &CoreFile) -> Option<String> {
        component.action().map(str::to_string)
    }
}

#[pymethods]
impl FilePicker {
    #[new]
    #[pyo3(signature = (
        root=PathBuf::from("."), *, prompt="File".to_string(), mode="file".to_string(),
        extensions=Vec::new(), hidden=false, jail=false, default=None, query=String::new(),
        height=10, mouse=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        root: PathBuf,
        prompt: String,
        mode: String,
        extensions: Vec<String>,
        hidden: bool,
        jail: bool,
        default: Option<PathBuf>,
        query: String,
        height: usize,
        mouse: bool,
    ) -> PyResult<Self> {
        Ok(FilePicker {
            parsed: file_mode(&mode)?,
            root,
            prompt,
            mode,
            extensions,
            hidden,
            jail,
            default,
            query,
            height: positive(height, "height")?,
            mouse,
        })
    }

    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }
}

impl FilePicker {
    fn prepare(&self) -> FileBuild {
        FileBuild {
            root: self.root.clone(),
            prompt: self.prompt.clone(),
            mode: self.parsed,
            extensions: self.extensions.clone(),
            hidden: self.hidden,
            jail: self.jail,
            default: self.default.clone(),
            query: self.query.clone(),
            height: self.height,
            mouse: self.mouse,
        }
    }
}

// ---------------------------------------------------------------------------
// ColorPicker

/// `ColorPicker(prompt="Colour", *, format="hex", value="", default=None,
/// height=10, palette=False, mouse=False)`: rich's named colours, the 256
/// palette (Tab) or a colour typed as hex or `rgb(r,g,b)`, with a live
/// swatch. The answer is a colour string rich parses: `#rrggbb` (`hex`), a
/// name (`name`, falling back to `color(N)` or hex) or `rgb(r,g,b)`.
#[pyclass(name = "ColorPicker", module = "rs_rich.interact", frozen)]
pub(crate) struct ColorPicker {
    #[pyo3(get)]
    prompt: String,
    #[pyo3(get)]
    format: String,
    #[pyo3(get)]
    value: String,
    #[pyo3(get)]
    default: Option<String>,
    #[pyo3(get)]
    height: usize,
    #[pyo3(get)]
    palette: bool,
    #[pyo3(get)]
    mouse: bool,
    parsed: ColorFormat,
}

struct ColorBuild {
    prompt: String,
    format: ColorFormat,
    value: String,
    default: Option<String>,
    height: usize,
    palette: bool,
    mouse: bool,
}

impl Build for ColorBuild {
    type C = CoreColor;

    fn build(self) -> CoreColor {
        let mut picker = CoreColor::new(self.prompt)
            .format(self.format)
            .value(self.value)
            .height(self.height)
            .palette(self.palette)
            .with_mouse(self.mouse);
        if let Some(default) = self.default {
            picker = picker.default(default);
        }
        picker
    }
}

#[pymethods]
impl ColorPicker {
    #[new]
    #[pyo3(signature = (
        prompt="Colour".to_string(), *, format="hex".to_string(), value=String::new(),
        default=None, height=10, palette=false, mouse=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        prompt: String,
        format: String,
        value: String,
        default: Option<String>,
        height: usize,
        palette: bool,
        mouse: bool,
    ) -> PyResult<Self> {
        let parsed = ColorFormat::parse(&format).ok_or_else(|| {
            PyValueError::new_err(format!(
                "invalid format {format:?}; expected hex, name or rgb"
            ))
        })?;
        if let Some(default) = default.as_deref() {
            if !CoreColor::is_color(default) {
                return Err(PyValueError::new_err(format!(
                    "invalid default {default:?}; expected a colour"
                )));
            }
        }
        Ok(ColorPicker {
            prompt,
            format,
            value,
            default,
            height: positive(height, "height")?,
            palette,
            mouse,
            parsed,
        })
    }

    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }
}

impl ColorPicker {
    fn prepare(&self) -> ColorBuild {
        ColorBuild {
            prompt: self.prompt.clone(),
            format: self.parsed,
            value: self.value.clone(),
            default: self.default.clone(),
            height: self.height,
            palette: self.palette,
            mouse: self.mouse,
        }
    }
}

// ---------------------------------------------------------------------------
// AssetPicker

/// `AssetPicker(kind="emoji", *, prompt=None, query="", default=None,
/// height=10, mouse=False)`: an emoji (by its shortcode name), a box
/// style or a spinner, with a preview. The answer is the emoji itself, or
/// the box style's or spinner's name (`"rounded"`, `"dots"`).
#[pyclass(name = "AssetPicker", module = "rs_rich.interact", frozen)]
pub(crate) struct AssetPicker {
    #[pyo3(get)]
    kind: String,
    #[pyo3(get)]
    prompt: Option<String>,
    #[pyo3(get)]
    query: String,
    #[pyo3(get)]
    default: Option<String>,
    #[pyo3(get)]
    height: usize,
    #[pyo3(get)]
    mouse: bool,
    parsed: AssetKind,
}

struct AssetBuild {
    kind: AssetKind,
    prompt: String,
    query: String,
    default: Option<String>,
    height: usize,
    mouse: bool,
}

impl Build for AssetBuild {
    type C = CoreAsset;

    fn build(self) -> CoreAsset {
        let mut picker = CoreAsset::new(self.prompt, self.kind)
            .height(self.height)
            .with_mouse(self.mouse);
        if let Some(default) = &self.default {
            picker = picker.default(default);
        }
        if !self.query.is_empty() {
            picker = picker.query(self.query);
        }
        picker
    }

    fn action(component: &CoreAsset) -> Option<String> {
        component.action().map(str::to_string)
    }
}

#[pymethods]
impl AssetPicker {
    #[new]
    #[pyo3(signature = (
        kind="emoji".to_string(), *, prompt=None, query=String::new(), default=None, height=10,
        mouse=false
    ))]
    fn new(
        kind: String,
        prompt: Option<String>,
        query: String,
        default: Option<String>,
        height: usize,
        mouse: bool,
    ) -> PyResult<Self> {
        let parsed = AssetKind::parse(&kind).ok_or_else(|| {
            PyValueError::new_err(format!(
                "invalid kind {kind:?}; expected emoji, box or spinner"
            ))
        })?;
        Ok(AssetPicker {
            kind,
            prompt,
            query,
            default,
            height: positive(height, "height")?,
            mouse,
            parsed,
        })
    }

    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }
}

impl AssetPicker {
    fn prepare(&self) -> AssetBuild {
        let prompt = self.prompt.clone().unwrap_or_else(|| {
            match self.parsed {
                AssetKind::Emoji => "Emoji",
                AssetKind::Box => "Box style",
                AssetKind::Spinner => "Spinner",
            }
            .to_string()
        });
        AssetBuild {
            kind: self.parsed,
            prompt,
            query: self.query.clone(),
            default: self.default.clone(),
            height: self.height,
            mouse: self.mouse,
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch

/// Run `component` if it is one of these classes; otherwise give `mode`
/// back for the next kind to try.
pub(crate) fn drive(
    py: Python<'_>,
    component: &Bound<'_, PyAny>,
    mode: Mode,
) -> PyResult<Result<Record, Mode>> {
    let text = |value: String| Ok(PyString::new(py, &value).into_any().unbind());
    if let Ok(area) = component.cast::<TextArea>() {
        let ran = execute(py, area.get().prepare(), mode)?;
        return record(py, ran, text).map(Ok);
    }
    if let Ok(picker) = component.cast::<FilePicker>() {
        let ran = execute(py, picker.get().prepare(), mode)?;
        return record(py, ran, |path| {
            Ok(path.into_pyobject(py)?.into_any().unbind())
        })
        .map(Ok);
    }
    if let Ok(picker) = component.cast::<ColorPicker>() {
        let ran = execute(py, picker.get().prepare(), mode)?;
        return record(py, ran, text).map(Ok);
    }
    if let Ok(picker) = component.cast::<AssetPicker>() {
        let ran = execute(py, picker.get().prepare(), mode)?;
        return record(py, ran, text).map(Ok);
    }
    Ok(Err(mode))
}

/// The node for a picker, as a child of a container (see `compose::leaf`),
/// if `component` is one.
pub(super) fn leaf(component: &Bound<'_, PyAny>) -> PyResult<Option<Node>> {
    let text = |py: Python<'_>, value: String| -> PyResult<Py<PyAny>> {
        Ok(PyString::new(py, &value).into_any().unbind())
    };
    if let Ok(area) = component.cast::<TextArea>() {
        return Ok(Some(compose::leaf(area.get().prepare(), text)));
    }
    if let Ok(picker) = component.cast::<FilePicker>() {
        return Ok(Some(compose::leaf(picker.get().prepare(), |py, path| {
            Ok(path.into_pyobject(py)?.into_any().unbind())
        })));
    }
    if let Ok(picker) = component.cast::<ColorPicker>() {
        return Ok(Some(compose::leaf(picker.get().prepare(), text)));
    }
    if let Ok(picker) = component.cast::<AssetPicker>() {
        return Ok(Some(compose::leaf(picker.get().prepare(), text)));
    }
    Ok(None)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    for (name, class) in [
        ("InteractTextArea", py.get_type::<TextArea>()),
        ("InteractFilePicker", py.get_type::<FilePicker>()),
        ("InteractColorPicker", py.get_type::<ColorPicker>()),
        ("InteractAssetPicker", py.get_type::<AssetPicker>()),
    ] {
        m.add(name, class)?;
    }
    Ok(())
}
