//! `rs_rich.micro`: the `rs-rich-micro` crate (0.0.14 workstream 7):
//! micro assets, the layered registry, `:micro:name:` markup, the pipeline
//! that makes packages, and drawing on a terminal.
//!
//! Owner: the micro area. Rich has no micro assets, so this mirrors the
//! Rust crate. Everything renders in Rust: a `MicroAsset` and a
//! `MicroMarkup` are renderables, drawn through Kitty, iTerm2, Sixel or
//! half-blocks when the console is a terminal that can show them (see
//! `rich_micro::select`), and as their emoji, text or alt-text fallback
//! anywhere else (a capture, an export, a pipe).
//!
//! | Python | Rust |
//! |---|---|
//! | `MicroRegistry` | `MicroRegistry` (built-in, user, trusted project, inline layers) |
//! | `MicroAsset` | `MicroAsset`, rendered as `MicroAssetRef` |
//! | `MicroMarkup` | `PreparedMarkup` over a console's markup, drawn by `MicroGraphics` |
//! | `micro_markup` | `render_markup`: the expanded `Text` and diagnostics |
//! | `micro_load_package`, `micro_create_package` | `package::load_package`, `pipeline` + `create::write_package` |

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyTypeError};
use pyo3::prelude::*;

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich_micro::create::{write_package, PackageSpec};
use rich_micro::pipeline::{Pipeline, Transparency};
use rich_micro::{
    CellSize, FallbackPreference, Layer, Limits, MicroAsset as CoreAsset, MicroAssetRef,
    MicroError as CoreError, MicroGraphics, MicroRegistry as CoreRegistry, MicroRoots,
};

use crate::art::{bad_choice, buffer_bytes, path_arg, read_file};
use crate::renderable::{self, AsRenderable};
use crate::text::Text;

create_exception!(_native, MicroError, PyException);

fn micro_error(error: CoreError) -> PyErr {
    MicroError::new_err(error.to_string())
}

fn preference(name: &str) -> PyResult<FallbackPreference> {
    Ok(match name {
        "emoji" => FallbackPreference::Emoji,
        "text" => FallbackPreference::Text,
        "alt" => FallbackPreference::Alt,
        other => return Err(bad_choice("preference", other, "emoji, text or alt")),
    })
}

fn layer(name: &str) -> PyResult<Layer> {
    Ok(match name {
        "built-in" | "builtin" => Layer::BuiltIn,
        "user" => Layer::User,
        "project" => Layer::Project,
        "inline" => Layer::Inline,
        other => return Err(bad_choice("layer", other, "built-in, user, project or inline")),
    })
}

fn optional_path(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<PathBuf>> {
    match value {
        None => Ok(None),
        Some(value) if value.is_none() => Ok(None),
        Some(value) => path_arg(value)?
            .map(Some)
            .ok_or_else(|| PyTypeError::new_err("expected a str or os.PathLike path")),
    }
}

/// A micro asset: an emoji-sized image or animation with alt text and a
/// fallback. It renders as exactly its cells.
#[pyclass(name = "MicroAsset", module = "rs_rich.micro", frozen)]
pub(crate) struct MicroAsset {
    inner: Arc<CoreAsset>,
}

impl MicroAsset {
    fn wrap(inner: Arc<CoreAsset>) -> MicroAsset {
        MicroAsset { inner }
    }
}

impl AsRenderable for MicroAsset {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(MicroAssetRef::new(Arc::clone(&self.inner))))
    }
}

#[pymethods]
impl MicroAsset {
    /// An asset with no image (its fallback always shows), for the inline
    /// layer. `alt` is mandatory; `emoji` and `text` must fit `size`.
    #[new]
    #[pyo3(signature = (name, alt, *, emoji=None, text=None, size="2x1"))]
    fn new(
        name: &str,
        alt: &str,
        emoji: Option<&str>,
        text: Option<&str>,
        size: &str,
    ) -> PyResult<Self> {
        let size: CellSize = size.parse().map_err(micro_error)?;
        let mut asset = CoreAsset::new(name, alt)
            .and_then(|asset| asset.with_size(size))
            .map_err(micro_error)?;
        if let Some(emoji) = emoji {
            asset = asset.with_emoji(emoji).map_err(micro_error)?;
        }
        if let Some(text) = text {
            asset = asset.with_text(text).map_err(micro_error)?;
        }
        Ok(MicroAsset::wrap(Arc::new(asset)))
    }

    #[getter]
    fn name(&self) -> &str {
        self.inner.name()
    }

    #[getter]
    fn alt(&self) -> &str {
        self.inner.alt()
    }

    /// `"2x1"` or `"1x1"`.
    #[getter]
    fn size(&self) -> String {
        self.inner.size().to_string()
    }

    /// Columns it always takes.
    #[getter]
    fn cols(&self) -> usize {
        self.inner.cols()
    }

    /// `"static"` or `"animated"`.
    #[getter]
    fn kind(&self) -> &'static str {
        self.inner.kind().as_str()
    }

    #[getter]
    fn emoji(&self) -> Option<&str> {
        self.inner.fallback().emoji.as_deref()
    }

    #[getter]
    fn text(&self) -> Option<&str> {
        self.inner.fallback().text.as_deref()
    }

    #[getter]
    fn aliases(&self) -> Vec<String> {
        self.inner.aliases().to_vec()
    }

    #[getter]
    fn license(&self) -> Option<&str> {
        self.inner.license()
    }

    #[getter]
    fn version(&self) -> Option<&str> {
        self.inner.version()
    }

    #[getter]
    fn author(&self) -> Option<&str> {
        self.inner.author()
    }

    /// The layer it resolved from: `built-in`, `user`, `project` or `inline`.
    #[getter]
    fn layer(&self) -> &'static str {
        self.inner.origin().layer.as_str()
    }

    /// Where it came from, as text.
    #[getter]
    fn origin(&self) -> String {
        self.inner.origin().to_string()
    }

    /// The cells shown where no image is drawn: exactly `cols` cells.
    #[pyo3(signature = (preference="emoji"))]
    fn fallback(&self, preference: &str) -> PyResult<String> {
        Ok(rich_micro::fallback_cells(
            &self.inner,
            self::preference(preference)?,
        ))
    }

    /// One occurrence as a `Text`: its fallback cells, tagged so a
    /// renderer can draw the image over them.
    #[pyo3(signature = (preference="emoji"))]
    fn placeholder(&self, preference: &str) -> PyResult<Text> {
        Ok(Text::from_core(rich_micro::placeholder(
            &self.inner,
            self::preference(preference)?,
        )))
    }

    fn __repr__(&self) -> String {
        format!(
            "<MicroAsset {:?} {} {}>",
            self.inner.name(),
            self.inner.size(),
            self.inner.kind().as_str()
        )
    }
}

/// Assets by name in four layers: built-in < user < trusted project <
/// inline. The project's assets load only when `trust_project` is true.
#[pyclass(name = "MicroRegistry", module = "rs_rich.micro", frozen)]
pub(crate) struct MicroRegistry {
    inner: std::sync::RwLock<Arc<CoreRegistry>>,
    rejected: Vec<String>,
}

impl MicroRegistry {
    fn shared(&self) -> Arc<CoreRegistry> {
        Arc::clone(&self.inner.read().unwrap_or_else(|e| e.into_inner()))
    }
}

#[pymethods]
impl MicroRegistry {
    /// `builtin`: the built-in library. `user` and `project`: directories
    /// of packages and packs (`~/.config/rich/micro`, `.rich/micro`);
    /// `project` loads only with `trust_project=True`.
    #[new]
    #[pyo3(signature = (*, builtin=true, user=None, project=None, trust_project=false))]
    fn new(
        builtin: bool,
        user: Option<&Bound<'_, PyAny>>,
        project: Option<&Bound<'_, PyAny>>,
        trust_project: bool,
    ) -> PyResult<Self> {
        let roots = MicroRoots {
            builtin_set: builtin,
            builtin: None,
            user: optional_path(user)?,
            project: optional_path(project)?,
            project_trusted: trust_project,
        };
        let (registry, report) = CoreRegistry::load(&roots, &Limits::default());
        let mut rejected: Vec<String> = report
            .rejected
            .iter()
            .map(|r| format!("{}: {}", r.path.display(), r.error))
            .collect();
        rejected.extend(report.untrusted_project.map(|dir| {
            format!("{}: not loaded, the project is not trusted", dir.display())
        }));
        Ok(MicroRegistry {
            inner: std::sync::RwLock::new(Arc::new(registry)),
            rejected,
        })
    }

    /// What did not load, and why.
    #[getter]
    fn rejected(&self) -> Vec<String> {
        self.rejected.clone()
    }

    /// Every effective asset name, sorted.
    fn names(&self) -> Vec<String> {
        self.shared()
            .names()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    /// Every effective asset, by name; or one `layer`'s.
    #[pyo3(signature = (layer=None))]
    fn assets(&self, layer: Option<&str>) -> PyResult<Vec<MicroAsset>> {
        let registry = self.shared();
        Ok(match layer {
            Some(name) => registry
                .layer(self::layer(name)?)
                .map(|asset| MicroAsset::wrap(Arc::clone(asset)))
                .collect(),
            None => registry
                .assets()
                .into_iter()
                .map(|asset| MicroAsset::wrap(Arc::clone(asset)))
                .collect(),
        })
    }

    /// The asset `name` (or an alias) resolves to, or `None`.
    fn get(&self, name: &str) -> Option<MicroAsset> {
        self.shared()
            .resolve(name)
            .map(|asset| MicroAsset::wrap(Arc::clone(asset)))
    }

    /// The asset `name` resolves to; `MicroError` when there is none.
    fn require(&self, name: &str) -> PyResult<MicroAsset> {
        self.shared()
            .require(name)
            .map(|asset| MicroAsset::wrap(Arc::clone(asset)))
            .map_err(micro_error)
    }

    /// Add `asset` to the inline layer (the highest). Returns the
    /// collisions it caused, as text.
    fn add(&self, asset: PyRef<'_, MicroAsset>) -> PyResult<Vec<String>> {
        let mut slot = self.inner.write().unwrap_or_else(|e| e.into_inner());
        let mut registry = (**slot).clone();
        let collisions = registry
            .add(Layer::Inline, (*asset.inner).clone())
            .map_err(micro_error)?;
        *slot = Arc::new(registry);
        Ok(collisions
            .into_iter()
            .map(|c| format!("{}: kept {}, dropped {}", c.name, c.kept, c.dropped))
            .collect())
    }

    /// How `name` resolved, as text: the winning layer and what it hides.
    fn explain(&self, name: &str) -> Option<String> {
        let explanation = self.shared().explain(name)?;
        let console = CoreConsole::builder().width(100).no_color(true).build();
        Some(console.render_to_string(&explanation))
    }

    fn __contains__(&self, name: &str) -> bool {
        self.shared().resolve(name).is_some()
    }

    fn __len__(&self) -> usize {
        self.shared().names().len()
    }

    fn __repr__(&self) -> String {
        format!("<MicroRegistry {} assets>", self.shared().names().len())
    }
}

/// The built-in registry, made once.
fn builtin() -> Arc<CoreRegistry> {
    static BUILTIN: OnceLock<Arc<CoreRegistry>> = OnceLock::new();
    Arc::clone(BUILTIN.get_or_init(|| Arc::new(CoreRegistry::builtin())))
}

fn registry_arg(registry: Option<PyRef<'_, MicroRegistry>>) -> Arc<CoreRegistry> {
    registry.map_or_else(builtin, |registry| registry.shared())
}

/// Console markup with `:micro:name:` tokens, drawn as the console can:
/// images on a terminal that shows them (`mode="auto"`), else each asset's
/// fallback. `mode="text"` never draws images.
#[pyclass(name = "MicroMarkup", module = "rs_rich.micro", frozen)]
pub(crate) struct MicroMarkup {
    markup: String,
    registry: Arc<CoreRegistry>,
    preference: FallbackPreference,
    draw: bool,
    graphics: Arc<OnceLock<MicroGraphics>>,
}

/// What a `MicroMarkup` renders as.
struct Drawn {
    markup: String,
    registry: Arc<CoreRegistry>,
    preference: FallbackPreference,
    draw: bool,
    graphics: Arc<OnceLock<MicroGraphics>>,
}

impl Drawn {
    fn text(&self, console: &CoreConsole) -> rich::Text {
        let prepared = rich_micro::PreparedMarkup::new(&self.markup, &self.registry, self.preference);
        prepared.finish(&console.build_text(prepared.markup()))
    }
}

impl Renderable for Drawn {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let text = self.text(console);
        if self.draw && console.is_terminal() {
            let graphics = self
                .graphics
                .get_or_init(|| MicroGraphics::detect(Arc::clone(&self.registry)));
            graphics.view(text).rich_render(console, options)
        } else {
            text.rich_render(console, options)
        }
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> rich::measure::Measurement {
        self.text(console).measure(console, options)
    }
}

impl AsRenderable for MicroMarkup {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Drawn {
            markup: self.markup.clone(),
            registry: Arc::clone(&self.registry),
            preference: self.preference,
            draw: self.draw,
            graphics: Arc::clone(&self.graphics),
        }))
    }
}

#[pymethods]
impl MicroMarkup {
    #[new]
    #[pyo3(signature = (markup, *, registry=None, preference="emoji", mode="auto"))]
    fn new(
        markup: String,
        registry: Option<PyRef<'_, MicroRegistry>>,
        preference: &str,
        mode: &str,
    ) -> PyResult<Self> {
        let draw = match mode {
            "auto" => true,
            "text" => false,
            other => return Err(bad_choice("mode", other, "auto or text")),
        };
        Ok(MicroMarkup {
            markup,
            registry: registry_arg(registry),
            preference: self::preference(preference)?,
            draw,
            graphics: Arc::new(OnceLock::new()),
        })
    }

    #[getter]
    fn markup(&self) -> &str {
        &self.markup
    }

    fn __repr__(&self) -> String {
        format!("<MicroMarkup {:?}>", self.markup)
    }
}

/// `markup` parsed with its `:micro:name:` tokens as placeholders: the
/// `Text`, and each token left as written (unknown or malformed), as text.
#[pyfunction]
#[pyo3(signature = (markup, registry=None, preference="emoji"))]
fn micro_markup(
    markup: &str,
    registry: Option<PyRef<'_, MicroRegistry>>,
    preference: &str,
) -> PyResult<(Text, Vec<String>)> {
    let registry = registry_arg(registry);
    let console = CoreConsole::builder().width(80).build();
    let (text, diagnostics) =
        rich_micro::render_markup(&console, markup, &registry, self::preference(preference)?);
    Ok((
        Text::from_core(text),
        diagnostics.iter().map(ToString::to_string).collect(),
    ))
}

/// Load a package (a folder or `.richmicro` zip) under the default limits.
#[pyfunction]
fn micro_load_package(path: &Bound<'_, PyAny>) -> PyResult<MicroAsset> {
    let path = path_arg(path)?.ok_or_else(|| PyTypeError::new_err("expected a path"))?;
    rich_micro::package::load_package(&path, Layer::Inline, &Limits::default())
        .map(|asset| MicroAsset::wrap(Arc::new(asset)))
        .map_err(micro_error)
}

fn fit(name: &str) -> PyResult<rich_art::ImageFit> {
    Ok(match name {
        "contain" => rich_art::ImageFit::Contain,
        "cover" => rich_art::ImageFit::Cover,
        "stretch" => rich_art::ImageFit::Stretch,
        other => return Err(bad_choice("fit", other, "contain, cover or stretch")),
    })
}

/// Run `image` (a path, or PNG, APNG, GIF or JPEG bytes) through the
/// pipeline and write a package at `dest` (a folder, or a `.richmicro`
/// zip with `archive=True`). Returns the asset as the registry reads it.
#[pyfunction]
#[pyo3(signature = (
    image, dest, *, name, alt, emoji=None, text=None, size="2x1", fit="contain",
    contrast=1.0, brightness=1.0, gamma=1.0, sharpen=None, transparency=128,
    license=None, author=None, version=None, archive=false,
))]
#[allow(clippy::too_many_arguments)]
fn micro_create_package(
    image: &Bound<'_, PyAny>,
    dest: &Bound<'_, PyAny>,
    name: &str,
    alt: &str,
    emoji: Option<String>,
    text: Option<String>,
    size: &str,
    fit: &str,
    contrast: f32,
    brightness: f32,
    gamma: f32,
    sharpen: Option<f32>,
    transparency: Option<u8>,
    license: Option<String>,
    author: Option<String>,
    version: Option<String>,
    archive: bool,
) -> PyResult<MicroAsset> {
    let bytes = match buffer_bytes(image)? {
        Some(bytes) => bytes,
        None => {
            let path = path_arg(image)?
                .ok_or_else(|| PyTypeError::new_err("expected a path or image bytes"))?;
            read_file(&path)?
        }
    };
    let dest = path_arg(dest)?.ok_or_else(|| PyTypeError::new_err("expected a path"))?;
    let mut pipeline = Pipeline::new(size.parse().map_err(micro_error)?);
    pipeline.fit = self::fit(fit)?;
    pipeline.transforms.contrast = contrast;
    pipeline.transforms.brightness = brightness;
    pipeline.transforms.gamma = gamma;
    pipeline.sharpen = sharpen;
    pipeline.transparency = match transparency {
        Some(cutoff) => Transparency::Threshold(cutoff),
        None => Transparency::Keep,
    };
    let processed = pipeline.process_bytes(&bytes).map_err(micro_error)?;
    let spec = PackageSpec {
        name: name.to_string(),
        alt: alt.to_string(),
        emoji,
        text,
        aliases: Vec::new(),
        version,
        license,
        author,
    };
    write_package(&dest, &spec, &processed, archive)
        .map(|asset| MicroAsset::wrap(Arc::new(asset)))
        .map_err(micro_error)
}

/// How micro assets would be drawn on this process's terminal: `(mode,
/// reason)`, mode one of `kitty`, `iterm`, `sixel`, `blocks`, `text`.
#[pyfunction]
fn micro_mode() -> (&'static str, String) {
    let selection = rich_micro::select(
        &rich_ext::graphics::GraphicsEnvironment::system(),
        &rich_ext::capabilities::SystemEnvironment,
    );
    (selection.mode.name(), selection.reason)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("MicroError", py.get_type::<MicroError>())?;
    renderable::add_renderable_class::<MicroAsset>(m)?;
    renderable::add_renderable_class::<MicroMarkup>(m)?;
    m.add_class::<MicroRegistry>()?;
    m.add_function(pyo3::wrap_pyfunction!(micro_markup, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(micro_load_package, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(micro_create_package, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(micro_mode, m)?)?;
    Ok(())
}
