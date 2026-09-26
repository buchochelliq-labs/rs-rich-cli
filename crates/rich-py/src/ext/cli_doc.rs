//! `rs_rich.ext.cli_doc`: describe a command once (`CommandSpec`,
//! `ArgSpec`), then render its help, errors as diagnostics, shell
//! completions, Markdown and man pages, config reference and config
//! precedence. The model is plain data; any argument parser (`argparse`,
//! `click`) can fill it.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyType};

use rich::protocol::Renderable;
use rich_ext::cli_doc::{
    self as cli, ArgSpec as CoreArg, Choice, CliError as CoreCliError, CliErrorKind,
    CommandSpec as CoreSpec, CompletionCatalog as CoreCatalog, CompletionKind,
    ConfigEntry as CoreEntry, ConfigReference as CoreReference, HelpView as CoreHelp,
    Layer as CoreLayer, Precedence as CorePrecedence, Shell, ValueHint,
};

use super::common::{self, names};
use super::diagnostic::Diagnostic;
use crate::renderable::{self, AsRenderable};

names!(cli_error_kind, cli_error_kind_name, CliErrorKind, "error kind", {
    "unknown_argument" => CliErrorKind::UnknownArgument,
    "missing_value" => CliErrorKind::MissingValue,
    "invalid_value" => CliErrorKind::InvalidValue,
    "missing_required" => CliErrorKind::MissingRequired,
    "unexpected_value" => CliErrorKind::UnexpectedValue,
    "conflict" => CliErrorKind::Conflict,
    "unknown_subcommand" => CliErrorKind::UnknownSubcommand,
    "other" => CliErrorKind::Other,
});

fn shell(name: &str) -> PyResult<Shell> {
    name.parse::<Shell>().map_err(PyValueError::new_err)
}

fn value_hint(name: &str) -> PyResult<ValueHint> {
    Ok(match name {
        "none" => ValueHint::None,
        "any" => ValueHint::Any,
        "file" => ValueHint::File,
        "dir" => ValueHint::Dir,
        "path" => ValueHint::Path,
        "command" => ValueHint::Command,
        "url" => ValueHint::Url,
        other => {
            return Err(PyValueError::new_err(format!(
                "invalid value hint {other:?}; expected none, any, file, dir, path, command or url \
                 (use choices= for a fixed set)"
            )))
        }
    })
}

fn choice(value: &Bound<'_, PyAny>) -> PyResult<Choice> {
    if let Ok(text) = value.extract::<String>() {
        return Ok(Choice::new(text));
    }
    let (value, help): (String, String) = value.extract()?;
    Ok(Choice::new(value).help(help))
}

// ---------------------------------------------------------------------------
// Arguments and commands

/// `ArgSpec(id, *, long=None, short=None, ...)`: one argument.
/// `ArgSpec.flag("verbose")`, `ArgSpec.option("width")` and
/// `ArgSpec.positional("path")` set the kind; the keywords fill the rest.
#[pyclass(
    name = "ArgSpec",
    module = "rs_rich.ext.cli_doc",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct ArgSpec {
    inner: CoreArg,
}

fn configure_arg(mut arg: CoreArg, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<CoreArg> {
    let Some(kwargs) = kwargs else {
        return Ok(arg);
    };
    for (key, value) in kwargs.iter() {
        let key: String = key.extract()?;
        if value.is_none() {
            continue;
        }
        arg = match key.as_str() {
            "long" => arg.long(value.extract::<String>()?),
            "short" => arg.short(value.extract::<char>()?),
            "aliases" => {
                for alias in common::strings(&value)? {
                    arg = arg.alias(alias);
                }
                arg
            }
            "value_name" => arg.value_name(value.extract::<String>()?),
            "value" => arg.value(value_hint(&value.extract::<String>()?)?),
            "choices" => {
                let choices: Vec<Choice> = value
                    .try_iter()?
                    .map(|c| choice(&c?))
                    .collect::<PyResult<_>>()?;
                arg.choices(choices)
            }
            "help" => arg.help(value.extract::<String>()?),
            "long_help" => arg.long_help(value.extract::<String>()?),
            "default" => arg.default_value(value.str()?.to_string()),
            "env" => arg.env(value.extract::<String>()?),
            "config_key" => arg.config_key(value.extract::<String>()?),
            "required" => arg.required(value.extract()?),
            "multiple" => arg.multiple(value.extract()?),
            "hidden" => arg.hidden(value.extract()?),
            "global_" | "is_global" => arg.global(value.extract()?),
            "heading" => arg.heading(value.extract::<String>()?),
            other => {
                return Err(pyo3::exceptions::PyTypeError::new_err(format!(
                    "ArgSpec got an unexpected keyword argument {other:?}"
                )))
            }
        };
    }
    Ok(arg)
}

#[pymethods]
impl ArgSpec {
    #[new]
    #[pyo3(signature = (id, **kwargs))]
    fn new(id: String, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        Ok(ArgSpec {
            inner: configure_arg(CoreArg::new(id), kwargs)?,
        })
    }

    /// A `--long` switch taking no value.
    #[staticmethod]
    #[pyo3(signature = (long, **kwargs))]
    fn flag(long: String, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        Ok(ArgSpec {
            inner: configure_arg(CoreArg::flag(long), kwargs)?,
        })
    }

    /// A `--long <VALUE>` option.
    #[staticmethod]
    #[pyo3(signature = (long, **kwargs))]
    fn option(long: String, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        Ok(ArgSpec {
            inner: configure_arg(CoreArg::option(long), kwargs)?,
        })
    }

    /// A positional argument.
    #[staticmethod]
    #[pyo3(signature = (name, **kwargs))]
    fn positional(name: String, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        Ok(ArgSpec {
            inner: configure_arg(CoreArg::positional(name), kwargs)?,
        })
    }

    #[getter]
    fn id(&self) -> String {
        self.inner.id.clone()
    }
    #[getter]
    fn long(&self) -> Option<String> {
        self.inner.long.clone()
    }
    #[getter]
    fn short(&self) -> Option<char> {
        self.inner.short
    }
    #[getter]
    fn help(&self) -> String {
        self.inner.help.clone()
    }
    #[getter]
    fn takes_value(&self) -> bool {
        self.inner.takes_value()
    }
    /// The help group heading (`Options`, `Arguments` or its own).
    #[getter]
    fn group(&self) -> String {
        self.inner.group().to_string()
    }
    /// `<VALUE>`, when it takes one.
    #[getter]
    fn metavar(&self) -> Option<String> {
        self.inner.metavar()
    }
    /// `["-w", "--width"]`.
    #[getter]
    fn switches(&self) -> Vec<String> {
        self.inner.switches()
    }
    /// `-w, --width <SIZE>` as help shows it.
    #[getter]
    fn names(&self) -> String {
        self.inner.names()
    }
    /// The main name (`--width`, or the positional's).
    #[getter]
    fn primary(&self) -> String {
        self.inner.primary()
    }
    /// `(value, help)` for each allowed value.
    #[getter]
    fn choices(&self) -> Vec<(String, String)> {
        self.inner
            .choice_list()
            .iter()
            .map(|c| (c.value.clone(), c.help.clone()))
            .collect()
    }
}

/// `CommandSpec(name, *, about="", version=None, args=(), subcommands=(),
/// examples=(), sections=(), ...)`: a command, its arguments and
/// subcommands. `examples` are `(command, description)` pairs, `sections`
/// `(title, body)`, `heading_notes` `(heading, text)`.
#[pyclass(
    name = "CommandSpec",
    module = "rs_rich.ext.cli_doc",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct CommandSpec {
    pub(crate) inner: CoreSpec,
}

fn spec_arg(value: &Bound<'_, PyAny>) -> PyResult<CoreSpec> {
    Ok(value.extract::<PyRef<'_, CommandSpec>>()?.inner.clone())
}

#[pymethods]
impl CommandSpec {
    #[new]
    #[pyo3(signature = (
        name, *, about="", version=None, bin_name=None, long_about=None, aliases=None, usage=None,
        args=None, subcommands=None, examples=None, sections=None, heading_notes=None,
        subcommand_heading=None, subcommand_required=false, hidden=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: String,
        about: &str,
        version: Option<String>,
        bin_name: Option<String>,
        long_about: Option<String>,
        aliases: Option<&Bound<'_, PyAny>>,
        usage: Option<&Bound<'_, PyAny>>,
        args: Option<&Bound<'_, PyAny>>,
        subcommands: Option<&Bound<'_, PyAny>>,
        examples: Option<&Bound<'_, PyAny>>,
        sections: Option<&Bound<'_, PyAny>>,
        heading_notes: Option<&Bound<'_, PyAny>>,
        subcommand_heading: Option<String>,
        subcommand_required: bool,
        hidden: bool,
    ) -> PyResult<Self> {
        let mut spec = CoreSpec::new(name)
            .about(about)
            .subcommand_required(subcommand_required)
            .hidden(hidden);
        if let Some(version) = version {
            spec = spec.version(version);
        }
        if let Some(name) = bin_name {
            spec = spec.bin_name(name);
        }
        if let Some(text) = long_about {
            spec = spec.long_about(text);
        }
        for alias in aliases
            .map(common::strings)
            .transpose()?
            .unwrap_or_default()
        {
            spec = spec.alias(alias);
        }
        for line in usage.map(common::strings).transpose()?.unwrap_or_default() {
            spec = spec.usage(line);
        }
        if let Some(args) = args {
            for arg in args.try_iter()? {
                spec = spec.arg(arg?.extract::<PyRef<'_, ArgSpec>>()?.inner.clone());
            }
        }
        if let Some(commands) = subcommands {
            for command in commands.try_iter()? {
                spec = spec.subcommand(spec_arg(&command?)?);
            }
        }
        if let Some(items) = examples {
            for item in items.try_iter()? {
                let (command, description): (String, String) = item?.extract()?;
                spec = spec.example(command, description);
            }
        }
        if let Some(items) = sections {
            for item in items.try_iter()? {
                let (title, body): (String, String) = item?.extract()?;
                spec = spec.section(title, body);
            }
        }
        if let Some(items) = heading_notes {
            for (heading, text) in common::pairs(items)? {
                spec = spec.heading_note(heading, text.extract::<String>()?);
            }
        }
        if let Some(heading) = subcommand_heading {
            spec = spec.subcommand_heading(heading);
        }
        Ok(CommandSpec { inner: spec })
    }

    #[getter]
    fn name(&self) -> String {
        self.inner.name.clone()
    }
    #[getter]
    fn about(&self) -> String {
        self.inner.about.clone()
    }
    #[getter]
    fn version(&self) -> Option<String> {
        self.inner.version.clone()
    }
    /// The binary name when set, else the name.
    #[getter]
    fn display_name(&self) -> String {
        self.inner.display_name().to_string()
    }
    #[getter]
    fn args(&self) -> Vec<ArgSpec> {
        self.inner
            .args
            .iter()
            .map(|a| ArgSpec { inner: a.clone() })
            .collect()
    }
    #[getter]
    fn subcommands(&self) -> Vec<CommandSpec> {
        self.inner
            .subcommands
            .iter()
            .map(|c| CommandSpec { inner: c.clone() })
            .collect()
    }

    /// The subcommand called (or aliased) `name`.
    fn find_subcommand(&self, name: &str) -> Option<CommandSpec> {
        self.inner
            .find_subcommand(name)
            .map(|c| CommandSpec { inner: c.clone() })
    }

    /// The usage lines (`rich [OPTIONS] [RESOURCE]`).
    fn usage_lines(&self) -> Vec<String> {
        self.inner.usage_lines()
    }

    /// Every visible switch (`-w`, `--width`, ...), for suggestions.
    fn switch_names(&self) -> Vec<String> {
        self.inner.switch_names()
    }

    /// Every visible subcommand name and alias.
    fn subcommand_names(&self) -> Vec<String> {
        self.inner.subcommand_names()
    }

    /// The help (`long=True` for `--help`), for this command or the
    /// subcommand at `path`.
    #[pyo3(signature = (*, long=false, path=None))]
    fn help(&self, long: bool, path: Option<Vec<String>>) -> PyResult<HelpView> {
        HelpView::new(&self.inner, long, path)
    }

    /// The completion script for `bash`, `zsh`, `fish` or `powershell`.
    fn completion(&self, shell: &str) -> PyResult<String> {
        Ok(cli::generate(&self.inner, self::shell(shell)?))
    }

    /// Every completion as a renderable catalogue.
    fn completion_catalog(&self) -> CompletionCatalog {
        CompletionCatalog {
            inner: CoreCatalog::from_spec(&self.inner),
        }
    }

    /// A Markdown reference for the command and its subcommands.
    fn to_markdown(&self) -> String {
        cli::to_markdown(&self.inner)
    }

    /// The Markdown reference as a renderable.
    fn markdown(&self) -> MarkdownReference {
        MarkdownReference {
            spec: self.inner.clone(),
        }
    }

    /// A man page (roff).
    #[pyo3(signature = (section="1", date=None))]
    fn to_man(&self, section: &str, date: Option<&str>) -> String {
        cli::to_man(&self.inner, section, date)
    }

    /// Man pages for the command and every subcommand: `(file name, page)`.
    #[pyo3(signature = (section="1", date=None))]
    fn to_man_pages(&self, section: &str, date: Option<&str>) -> Vec<(String, String)> {
        cli::to_man_pages(&self.inner, section, date)
    }

    /// The configuration reference its `config_key`/`env` arguments imply.
    fn config_reference(&self) -> ConfigReference {
        ConfigReference {
            inner: CoreReference::from_spec(&self.inner),
        }
    }

    /// A `CliError` for an unknown `argument` (a switch or a subcommand),
    /// with suggestions and usage.
    fn unknown(&self, argument: &str) -> CliError {
        CliError {
            inner: CoreCliError::unknown_in(&self.inner, argument),
        }
    }
}

/// `HelpView(spec, *, long=False, path=None)`: the command's help:
/// usage, about, argument groups, subcommands, examples.
#[pyclass(name = "HelpView", module = "rs_rich.ext.cli_doc", frozen)]
pub(crate) struct HelpView {
    inner: CoreHelp,
}

impl HelpView {
    fn new(spec: &CoreSpec, long: bool, path: Option<Vec<String>>) -> PyResult<Self> {
        let view = match path {
            None => CoreHelp::new(spec),
            Some(path) => {
                let path: Vec<&str> = path.iter().map(String::as_str).collect();
                CoreHelp::for_path(spec, &path).ok_or_else(|| {
                    PyValueError::new_err(format!("no subcommand {}", path.join(" ")))
                })?
            }
        };
        Ok(HelpView {
            inner: view.long(long),
        })
    }
}

impl AsRenderable for HelpView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl HelpView {
    #[new]
    #[pyo3(signature = (spec, *, long=false, path=None))]
    fn py_new(
        spec: PyRef<'_, CommandSpec>,
        long: bool,
        path: Option<Vec<String>>,
    ) -> PyResult<Self> {
        HelpView::new(&spec.inner, long, path)
    }
}

/// A command's Markdown reference, rendered.
#[pyclass(name = "MarkdownReference", module = "rs_rich.ext.cli_doc", frozen)]
pub(crate) struct MarkdownReference {
    spec: CoreSpec,
}

impl AsRenderable for MarkdownReference {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(cli::markdown_view(&self.spec)))
    }
}

/// Every completion of a command: subcommands, options and values.
#[pyclass(name = "CompletionCatalog", module = "rs_rich.ext.cli_doc", frozen)]
pub(crate) struct CompletionCatalog {
    inner: CoreCatalog,
}

impl AsRenderable for CompletionCatalog {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl CompletionCatalog {
    /// `(path, word, description, group, kind)` per item, `kind` one of
    /// `subcommand`, `option`, `value`.
    #[allow(clippy::type_complexity)]
    fn items(&self) -> Vec<(Vec<String>, String, String, String, &'static str)> {
        self.inner
            .items
            .iter()
            .map(|item| {
                let kind = match item.kind {
                    CompletionKind::Subcommand => "subcommand",
                    CompletionKind::Option => "option",
                    CompletionKind::Value => "value",
                };
                (
                    item.path.clone(),
                    item.word.clone(),
                    item.description.clone(),
                    item.group.clone(),
                    kind,
                )
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Errors

/// `CliError(kind, message="", *, argument=None, value=None,
/// suggestions=(), possible_values=(), usage=None, help_flag=None)`: a
/// command-line error that renders as a diagnostic with "did you mean"
/// suggestions. The constructors below build the usual ones.
#[pyclass(name = "CliError", module = "rs_rich.ext.cli_doc", frozen)]
pub(crate) struct CliError {
    inner: CoreCliError,
}

impl AsRenderable for CliError {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.to_diagnostic()))
    }
}

#[pymethods]
impl CliError {
    #[new]
    #[pyo3(signature = (kind, message="", *, argument=None, value=None, suggestions=None, possible_values=None, usage=None, help_flag=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        kind: &str,
        message: &str,
        argument: Option<String>,
        value: Option<String>,
        suggestions: Option<&Bound<'_, PyAny>>,
        possible_values: Option<&Bound<'_, PyAny>>,
        usage: Option<String>,
        help_flag: Option<String>,
    ) -> PyResult<Self> {
        let mut inner = CoreCliError::new(cli_error_kind(kind)?, message);
        if let Some(argument) = argument {
            inner = inner.argument(argument);
        }
        inner.value = value;
        if let Some(items) = suggestions {
            inner = inner.suggestions(common::strings(items)?);
        }
        if let Some(items) = possible_values {
            inner.possible_values = common::strings(items)?;
        }
        if let Some(usage) = usage {
            inner = inner.usage(usage);
        }
        if let Some(flag) = help_flag {
            inner = inner.help_flag(flag);
        }
        Ok(CliError { inner })
    }

    #[classmethod]
    fn unknown_argument(
        _cls: &Bound<'_, PyType>,
        argument: String,
        candidates: Vec<String>,
    ) -> Self {
        CliError {
            inner: CoreCliError::unknown_argument(argument, candidates),
        }
    }

    #[classmethod]
    fn unknown_subcommand(_cls: &Bound<'_, PyType>, name: String, candidates: Vec<String>) -> Self {
        CliError {
            inner: CoreCliError::unknown_subcommand(name, candidates),
        }
    }

    #[classmethod]
    fn invalid_value(
        _cls: &Bound<'_, PyType>,
        argument: String,
        value: String,
        possible: Vec<String>,
    ) -> Self {
        CliError {
            inner: CoreCliError::invalid_value(argument, value, possible),
        }
    }

    #[classmethod]
    fn missing_value(_cls: &Bound<'_, PyType>, argument: String) -> Self {
        CliError {
            inner: CoreCliError::missing_value(argument),
        }
    }

    #[classmethod]
    fn missing_required(_cls: &Bound<'_, PyType>, arguments: Vec<String>) -> Self {
        CliError {
            inner: CoreCliError::missing_required(arguments),
        }
    }

    #[getter]
    fn kind(&self) -> &'static str {
        cli_error_kind_name(self.inner.kind)
    }
    #[getter]
    fn argument(&self) -> Option<String> {
        self.inner.argument.clone()
    }
    #[getter]
    fn suggestions(&self) -> Vec<String> {
        self.inner.suggestions.clone()
    }
    /// 2, as argument parsers exit with on a usage error.
    #[getter]
    fn exit_code(&self) -> i32 {
        self.inner.exit_code()
    }
    /// The one-line message.
    #[getter]
    fn headline(&self) -> String {
        self.inner.headline()
    }
    /// The error as a `Diagnostic`.
    fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::from_core(
            self.inner.to_diagnostic(),
            rich_ext::event::EventView::Expanded,
        )
    }

    fn __str__(&self) -> String {
        self.inner.headline()
    }
}

/// `suggest(input, candidates)`: the candidates close to `input`, best
/// first (Jaro-Winkler), for "did you mean".
#[pyfunction]
fn suggest(input: &str, candidates: Vec<String>) -> Vec<String> {
    cli::suggest(input, candidates)
}

// ---------------------------------------------------------------------------
// Configuration

/// `ConfigEntry(key, kind, *, default=None, choices=(), env=None,
/// flag=None, description="")`: one setting of a `ConfigReference`.
#[pyclass(
    name = "ConfigEntry",
    module = "rs_rich.ext.cli_doc",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct ConfigEntry {
    inner: CoreEntry,
}

#[pymethods]
impl ConfigEntry {
    #[new]
    #[pyo3(signature = (key, kind, *, default=None, choices=None, env=None, flag=None, description=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        key: String,
        kind: String,
        default: Option<String>,
        choices: Option<&Bound<'_, PyAny>>,
        env: Option<String>,
        flag: Option<String>,
        description: Option<String>,
    ) -> PyResult<Self> {
        let mut inner = CoreEntry::new(key, kind);
        if let Some(value) = default {
            inner = inner.default_value(value);
        }
        if let Some(choices) = choices {
            inner = inner.choices(common::strings(choices)?);
        }
        if let Some(env) = env {
            inner = inner.env(env);
        }
        if let Some(flag) = flag {
            inner = inner.flag(flag);
        }
        if let Some(text) = description {
            inner = inner.description(text);
        }
        Ok(ConfigEntry { inner })
    }
}

/// `ConfigReference(title, *, description="", sources=(), entries=())`:
/// where settings come from (`(name, location, description)` sources,
/// lowest precedence first) and every setting.
#[pyclass(name = "ConfigReference", module = "rs_rich.ext.cli_doc", frozen)]
pub(crate) struct ConfigReference {
    inner: CoreReference,
}

impl AsRenderable for ConfigReference {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl ConfigReference {
    #[new]
    #[pyo3(signature = (title, *, description=None, sources=None, entries=None))]
    fn new(
        title: String,
        description: Option<String>,
        sources: Option<&Bound<'_, PyAny>>,
        entries: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut inner = CoreReference::new(title);
        if let Some(text) = description {
            inner = inner.description(text);
        }
        if let Some(sources) = sources {
            for source in sources.try_iter()? {
                let (name, location, description): (String, String, String) = source?.extract()?;
                inner = inner.source(name, location, description);
            }
        }
        if let Some(entries) = entries {
            for entry in entries.try_iter()? {
                inner = inner.entry(entry?.extract::<PyRef<'_, ConfigEntry>>()?.inner.clone());
            }
        }
        Ok(ConfigReference { inner })
    }

    /// The reference a command's `config_key`/`env` arguments imply, plus
    /// a description, sources and more entries.
    #[classmethod]
    #[pyo3(signature = (spec, *, description=None, sources=None, entries=None))]
    fn from_spec(
        _cls: &Bound<'_, PyType>,
        spec: PyRef<'_, CommandSpec>,
        description: Option<String>,
        sources: Option<&Bound<'_, PyAny>>,
        entries: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut inner = CoreReference::from_spec(&spec.inner);
        if let Some(text) = description {
            inner = inner.description(text);
        }
        if let Some(sources) = sources {
            for source in sources.try_iter()? {
                let (name, location, description): (String, String, String) = source?.extract()?;
                inner = inner.source(name, location, description);
            }
        }
        if let Some(entries) = entries {
            for entry in entries.try_iter()? {
                inner = inner.entry(entry?.extract::<PyRef<'_, ConfigEntry>>()?.inner.clone());
            }
        }
        Ok(ConfigReference { inner })
    }

    /// The reference as Markdown.
    fn to_markdown(&self) -> String {
        self.inner.to_markdown()
    }
}

/// `Layer(name, values=None, *, origin=None)`: one source of settings
/// (`values` a dict or `(key, value)` pairs).
#[pyclass(
    name = "ConfigLayer",
    module = "rs_rich.ext.cli_doc",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct ConfigLayer {
    inner: CoreLayer,
}

#[pymethods]
impl ConfigLayer {
    #[new]
    #[pyo3(signature = (name, values=None, *, origin=None))]
    fn new(
        name: String,
        values: Option<&Bound<'_, PyAny>>,
        origin: Option<String>,
    ) -> PyResult<Self> {
        let mut inner = CoreLayer::new(name);
        if let Some(origin) = origin {
            inner = inner.origin(origin);
        }
        if let Some(values) = values {
            for (key, value) in common::pairs(values)? {
                inner = inner.value(key, value.str()?.to_string());
            }
        }
        Ok(ConfigLayer { inner })
    }
}

/// `Precedence(layers)`: settings from several layers (lowest first),
/// resolved. Renders as a table showing which layer won each key.
#[pyclass(name = "Precedence", module = "rs_rich.ext.cli_doc", frozen)]
pub(crate) struct Precedence {
    inner: CorePrecedence,
}

impl AsRenderable for Precedence {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.view()))
    }
}

/// How one key was resolved: renders its layers, winner first.
#[pyclass(name = "PrecedenceExplanation", module = "rs_rich.ext.cli_doc", frozen)]
pub(crate) struct PrecedenceExplanation {
    inner: cli::Explanation,
}

impl AsRenderable for PrecedenceExplanation {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl PrecedenceExplanation {
    #[getter]
    fn key(&self) -> String {
        self.inner.resolved().key.clone()
    }
    #[getter]
    fn value(&self) -> String {
        self.inner.resolved().value.clone()
    }
    /// The winning layer's index.
    #[getter]
    fn winner(&self) -> usize {
        self.inner.resolved().winner
    }
    /// `(layer index, value)` the winner overrode.
    #[getter]
    fn shadowed(&self) -> Vec<(usize, String)> {
        self.inner.resolved().shadowed.clone()
    }
}

#[pymethods]
impl Precedence {
    #[new]
    fn new(layers: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut inner = CorePrecedence::new();
        for layer in layers.try_iter()? {
            inner = inner.layer(layer?.extract::<PyRef<'_, ConfigLayer>>()?.inner.clone());
        }
        Ok(Precedence { inner })
    }

    /// `{key: value}` after every layer.
    fn resolve(&self) -> Vec<(String, String)> {
        self.inner
            .resolve()
            .into_iter()
            .map(|r| (r.key, r.value))
            .collect()
    }

    /// How `key` was resolved, or `None` if no layer sets it.
    fn explain(&self, key: &str) -> Option<PrecedenceExplanation> {
        self.inner
            .explain(key)
            .map(|inner| PrecedenceExplanation { inner })
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ArgSpec>()?;
    m.add_class::<CommandSpec>()?;
    renderable::add_renderable_class::<HelpView>(m)?;
    renderable::add_renderable_class::<MarkdownReference>(m)?;
    renderable::add_renderable_class::<CompletionCatalog>(m)?;
    renderable::add_renderable_class::<CliError>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(suggest, m)?)?;
    m.add_class::<ConfigEntry>()?;
    renderable::add_renderable_class::<ConfigReference>(m)?;
    m.add_class::<ConfigLayer>()?;
    renderable::add_renderable_class::<Precedence>(m)?;
    renderable::add_renderable_class::<PrecedenceExplanation>(m)?;
    m.add("HELP_STYLES", cli::STYLES.to_vec())?;
    m.add("HELP_STACK_BELOW", cli::STACK_BELOW)?;
    m.add(
        "COMPLETION_SHELLS",
        Shell::ALL.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )?;
    Ok(())
}
