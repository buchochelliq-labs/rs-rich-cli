//! Still images: `ArtImage` (a decoded image), `ImageArt` (every mode, fit,
//! background, colour mode, dither and adjustment), `ImageOptions`,
//! `RenderCapabilities`, and the single-backend renderables `AsciiArt`,
//! `BlockArt`, `BrailleArt`, `QuadrantArt` and `SixelArt`.

use std::sync::Arc;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyTuple};

use rich::color::Color as CoreColor;
use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich_art::image as img;
use rich_art::image::DynamicImage;
use rich_art::{
    ColorDistance, Dither, ImageAnchor, ImageBackground, ImageColorMode, ImageFit, ImageMode,
    ImageTransforms, Rotation,
};

use super::{
    bad_choice, buffer_bytes, core_console, fail_render, kinded, normalized, path_arg, read_file,
    repr_str, ImageArtError, ImageDecodeError, Shared,
};
use crate::renderable::{self, AsRenderable};

// ---------------------------------------------------------------------------
// Loading images

/// Decode an encoded image (PNG, JPEG or GIF).
pub(crate) fn decode(bytes: &[u8]) -> PyResult<DynamicImage> {
    img::load_from_memory(bytes).map_err(|error| decode_error(&error))
}

pub(crate) fn decode_error(error: &img::ImageError) -> PyErr {
    ImageDecodeError::new_err(format!("could not decode the image: {error}"))
}

/// Whether `value` looks like a Pillow image.
fn is_pillow(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    for name in ["mode", "size", "convert", "tobytes"] {
        if !value.hasattr(name)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A Pillow image, through its raw RGBA bytes (Pillow itself is optional).
fn from_pillow(value: &Bound<'_, PyAny>) -> PyResult<DynamicImage> {
    let mode: String = value.getattr("mode")?.extract()?;
    let rgba = if mode == "RGBA" {
        value.clone()
    } else {
        value.call_method1("convert", ("RGBA",))?
    };
    let (width, height): (u32, u32) = rgba.getattr("size")?.extract()?;
    let data = rgba.call_method0("tobytes")?;
    let data = buffer_bytes(&data)?
        .ok_or_else(|| PyTypeError::new_err("the image's tobytes() did not return bytes"))?;
    let raster = img::RgbaImage::from_raw(width, height, data).ok_or_else(|| {
        PyValueError::new_err("the image's tobytes() does not match its size in RGBA")
    })?;
    Ok(DynamicImage::ImageRgba8(raster))
}

/// Any image argument: an `ArtImage`, a path, encoded bytes, or a Pillow
/// image.
pub(crate) fn load_image(value: &Bound<'_, PyAny>) -> PyResult<Arc<DynamicImage>> {
    if let Ok(image) = value.cast::<ArtImage>() {
        return Ok(Arc::clone(&image.get().inner));
    }
    if let Some(path) = path_arg(value)? {
        return Ok(Arc::new(decode(&read_file(&path)?)?));
    }
    if let Some(bytes) = buffer_bytes(value)? {
        return Ok(Arc::new(decode(&bytes)?));
    }
    if is_pillow(value)? {
        return Ok(Arc::new(from_pillow(value)?));
    }
    Err(PyTypeError::new_err(format!(
        "expected an image: a path, encoded bytes, an ArtImage or a Pillow image, not {}",
        value.get_type().name()?
    )))
}

/// An owned copy of an image argument, for backends that take one.
fn owned_image(value: &Bound<'_, PyAny>) -> PyResult<DynamicImage> {
    Ok((*load_image(value)?).clone())
}

/// A decoded image: what every art class takes (besides paths and bytes),
/// and what `DiffReport.heatmap()` returns.
#[pyclass(name = "ArtImage", module = "rs_rich.art", frozen)]
pub(crate) struct ArtImage {
    pub(crate) inner: Arc<DynamicImage>,
}

impl ArtImage {
    pub(crate) fn wrap(image: DynamicImage) -> ArtImage {
        ArtImage {
            inner: Arc::new(image),
        }
    }
}

fn mode_of(image: &DynamicImage) -> &'static str {
    match image {
        DynamicImage::ImageLuma8(_) => "L",
        DynamicImage::ImageLumaA8(_) => "LA",
        DynamicImage::ImageRgb8(_) => "RGB",
        DynamicImage::ImageRgba8(_) => "RGBA",
        DynamicImage::ImageLuma16(_) => "I;16",
        DynamicImage::ImageLumaA16(_) => "LA;16",
        DynamicImage::ImageRgb16(_) => "RGB;16",
        DynamicImage::ImageRgba16(_) => "RGBA;16",
        DynamicImage::ImageRgb32F(_) => "RGB;F",
        DynamicImage::ImageRgba32F(_) => "RGBA;F",
        _ => "unknown",
    }
}

#[pymethods]
impl ArtImage {
    /// `ArtImage(source)`: a path, encoded bytes, another `ArtImage` or a
    /// Pillow image.
    #[new]
    fn new(source: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(ArtImage {
            inner: load_image(source)?,
        })
    }

    /// Decode an image file.
    #[staticmethod]
    fn open(path: &Bound<'_, PyAny>) -> PyResult<Self> {
        let path = path_arg(path)?.ok_or_else(|| PyTypeError::new_err("expected a path"))?;
        Ok(ArtImage::wrap(decode(&read_file(&path)?)?))
    }

    /// Decode encoded image bytes (PNG, JPEG or GIF).
    #[staticmethod]
    fn from_bytes(data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let bytes = buffer_bytes(data)?.ok_or_else(|| PyTypeError::new_err("expected bytes"))?;
        Ok(ArtImage::wrap(decode(&bytes)?))
    }

    /// Convert a Pillow image (through its RGBA bytes).
    #[staticmethod]
    fn from_pil(image: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(ArtImage::wrap(from_pillow(image)?))
    }

    /// Raw pixels, as Pillow's `Image.frombytes`: mode `"RGBA"`, `"RGB"`,
    /// `"LA"` or `"L"`, 8 bits per channel, rows top to bottom.
    #[staticmethod]
    fn frombytes(mode: &str, size: (u32, u32), data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data = buffer_bytes(data)?.ok_or_else(|| PyTypeError::new_err("expected bytes"))?;
        let (width, height) = size;
        let mismatch = || {
            PyValueError::new_err(format!(
                "{} bytes do not make a {width}x{height} {mode} image",
                data.len()
            ))
        };
        let image = match mode {
            "RGBA" => DynamicImage::ImageRgba8(
                img::RgbaImage::from_raw(width, height, data.clone()).ok_or_else(mismatch)?,
            ),
            "RGB" => DynamicImage::ImageRgb8(
                img::RgbImage::from_raw(width, height, data.clone()).ok_or_else(mismatch)?,
            ),
            "LA" => DynamicImage::ImageLumaA8(
                img::GrayAlphaImage::from_raw(width, height, data.clone()).ok_or_else(mismatch)?,
            ),
            "L" => DynamicImage::ImageLuma8(
                img::GrayImage::from_raw(width, height, data.clone()).ok_or_else(mismatch)?,
            ),
            other => return Err(bad_choice("mode", other, "RGBA, RGB, LA or L")),
        };
        Ok(ArtImage::wrap(image))
    }

    #[getter]
    fn width(&self) -> u32 {
        self.inner.width()
    }

    #[getter]
    fn height(&self) -> u32 {
        self.inner.height()
    }

    #[getter]
    fn size(&self) -> (u32, u32) {
        (self.inner.width(), self.inner.height())
    }

    /// The pixel format, in Pillow's names (`"RGBA"`, `"RGB"`, `"L"`, ...).
    #[getter]
    fn mode(&self) -> &'static str {
        mode_of(&self.inner)
    }

    /// The raw pixels in this image's own mode.
    fn tobytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.inner.as_bytes())
    }

    /// The pixels as 8-bit RGBA.
    fn to_rgba<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.inner.to_rgba8().as_raw())
    }

    /// The image encoded as PNG.
    fn to_png<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let mut out = std::io::Cursor::new(Vec::new());
        self.inner
            .write_to(&mut out, img::ImageFormat::Png)
            .map_err(|error| PyValueError::new_err(format!("could not encode PNG: {error}")))?;
        Ok(PyBytes::new(py, out.get_ref()))
    }

    /// Save as PNG, JPEG or GIF, chosen by the file's extension.
    fn save(&self, path: &Bound<'_, PyAny>) -> PyResult<()> {
        let path = path_arg(path)?.ok_or_else(|| PyTypeError::new_err("expected a path"))?;
        self.inner.save(&path).map_err(|error| match error {
            img::ImageError::IoError(io) => PyErr::from(io),
            other => PyValueError::new_err(format!("could not save the image: {other}")),
        })
    }

    /// A Pillow image with the same pixels (needs Pillow installed).
    fn to_pil<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let pil = py.import("PIL.Image")?;
        pil.call_method1("frombytes", ("RGBA", self.size(), self.to_rgba(py)))
    }

    fn __repr__(&self) -> String {
        format!(
            "<ArtImage {}x{} {}>",
            self.inner.width(),
            self.inner.height(),
            self.mode()
        )
    }
}

// ---------------------------------------------------------------------------
// Option names (the `rich` CLI's spellings)

pub(crate) fn image_mode(name: &str) -> PyResult<ImageMode> {
    Ok(match normalized(name).as_str() {
        "auto" => ImageMode::Auto,
        "ascii" | "art" => ImageMode::Ascii,
        "blocks" | "block" | "half-block" | "half-blocks" => ImageMode::Blocks,
        "braille" => ImageMode::Braille,
        "quadrants" | "quadrant" => ImageMode::Quadrants,
        "sixel" => ImageMode::Sixel,
        _ => {
            return Err(bad_choice(
                "image mode",
                name,
                "auto, ascii, blocks, braille, quadrants or sixel",
            ))
        }
    })
}

pub(crate) fn image_mode_name(mode: ImageMode) -> &'static str {
    match mode {
        ImageMode::Auto => "auto",
        ImageMode::Ascii => "ascii",
        ImageMode::Blocks => "blocks",
        ImageMode::Braille => "braille",
        ImageMode::Quadrants => "quadrants",
        ImageMode::Sixel => "sixel",
    }
}

fn image_fit(name: &str) -> PyResult<ImageFit> {
    Ok(match normalized(name).as_str() {
        "contain" => ImageFit::Contain,
        "cover" => ImageFit::Cover,
        "stretch" => ImageFit::Stretch,
        "native" => ImageFit::Native,
        _ => return Err(bad_choice("fit", name, "contain, cover, stretch or native")),
    })
}

fn image_fit_name(fit: ImageFit) -> &'static str {
    match fit {
        ImageFit::Contain => "contain",
        ImageFit::Cover => "cover",
        ImageFit::Stretch => "stretch",
        ImageFit::Native => "native",
    }
}

const ANCHORS: [(&str, ImageAnchor); 9] = [
    ("center", ImageAnchor::Center),
    ("top", ImageAnchor::Top),
    ("bottom", ImageAnchor::Bottom),
    ("left", ImageAnchor::Left),
    ("right", ImageAnchor::Right),
    ("top-left", ImageAnchor::TopLeft),
    ("top-right", ImageAnchor::TopRight),
    ("bottom-left", ImageAnchor::BottomLeft),
    ("bottom-right", ImageAnchor::BottomRight),
];

fn image_anchor(name: &str) -> PyResult<ImageAnchor> {
    let key = normalized(name);
    let key = if key == "centre" {
        "center".into()
    } else {
        key
    };
    ANCHORS
        .iter()
        .find(|(n, _)| *n == key)
        .map(|(_, anchor)| *anchor)
        .ok_or_else(|| {
            bad_choice(
                "anchor",
                name,
                "center, top, bottom, left, right, top-left, top-right, bottom-left or bottom-right",
            )
        })
}

fn image_anchor_name(anchor: ImageAnchor) -> &'static str {
    ANCHORS
        .iter()
        .find(|(_, a)| *a == anchor)
        .map(|(n, _)| *n)
        .unwrap_or("center")
}

pub(crate) fn color_mode(name: &str) -> PyResult<ImageColorMode> {
    Ok(match normalized(name).as_str() {
        "truecolor" => ImageColorMode::TrueColor,
        "ansi256" => ImageColorMode::Ansi256,
        "ansi16" => ImageColorMode::Ansi16,
        "grayscale" | "greyscale" => ImageColorMode::Grayscale,
        _ => {
            return Err(bad_choice(
                "color mode",
                name,
                "truecolor, ansi256, ansi16 or grayscale",
            ))
        }
    })
}

pub(crate) fn color_mode_name(mode: ImageColorMode) -> &'static str {
    match mode {
        ImageColorMode::TrueColor => "truecolor",
        ImageColorMode::Ansi256 => "ansi256",
        ImageColorMode::Ansi16 => "ansi16",
        ImageColorMode::Grayscale => "grayscale",
    }
}

pub(crate) fn dither(name: Option<&str>) -> PyResult<Dither> {
    let Some(name) = name else {
        return Ok(Dither::None);
    };
    Ok(match normalized(name).as_str() {
        "none" => Dither::None,
        "floyd-steinberg" => Dither::FloydSteinberg,
        "bayer4x4" | "bayer" => Dither::Bayer4x4,
        "atkinson" => Dither::Atkinson,
        _ => {
            return Err(bad_choice(
                "dither",
                name,
                "none, floyd-steinberg, bayer4x4 or atkinson",
            ))
        }
    })
}

pub(crate) fn dither_name(dither: Dither) -> Option<&'static str> {
    match dither {
        Dither::None => None,
        Dither::FloydSteinberg => Some("floyd-steinberg"),
        Dither::Bayer4x4 => Some("bayer4x4"),
        Dither::Atkinson => Some("atkinson"),
    }
}

pub(crate) fn color_distance(name: &str) -> PyResult<ColorDistance> {
    Ok(match normalized(name).as_str() {
        "rgb" => ColorDistance::Rgb,
        "oklab" => ColorDistance::Oklab,
        _ => return Err(bad_choice("color distance", name, "rgb or oklab")),
    })
}

pub(crate) fn color_distance_name(distance: ColorDistance) -> &'static str {
    match distance {
        ColorDistance::Rgb => "rgb",
        ColorDistance::Oklab => "oklab",
    }
}

fn rotation(degrees: i64) -> PyResult<Rotation> {
    Ok(match degrees.rem_euclid(360) {
        0 => Rotation::None,
        90 => Rotation::Clockwise90,
        180 => Rotation::Clockwise180,
        270 => Rotation::Clockwise270,
        _ => {
            return Err(PyValueError::new_err(format!(
                "invalid rotation {degrees}; expected a multiple of 90 degrees (clockwise)"
            )))
        }
    })
}

fn rotation_degrees(rotation: Rotation) -> u32 {
    match rotation {
        Rotation::None => 0,
        Rotation::Clockwise90 => 90,
        Rotation::Clockwise180 => 180,
        Rotation::Clockwise270 => 270,
    }
}

/// `background=`: `None`, `"default"` (the terminal's own), `"checkerboard"`,
/// an `(r, g, b)` tuple, or a colour (`"#102030"`, `"rgb(1,2,3)"`, `"red"`).
fn background(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<ImageBackground>> {
    let Some(value) = value.filter(|v| !v.is_none()) else {
        return Ok(None);
    };
    if let Ok(name) = value.extract::<String>() {
        return Ok(Some(match normalized(&name).as_str() {
            "default" | "terminal" => ImageBackground::TerminalDefault,
            "checkerboard" => ImageBackground::Checkerboard,
            _ => {
                let color = CoreColor::parse(&name).map_err(|error| {
                    PyValueError::new_err(format!(
                        "invalid background {}: {error}; expected a colour, 'default' or \
                         'checkerboard'",
                        repr_str(&name)
                    ))
                })?;
                let triplet = color
                    .get_truecolor()
                    .ok_or_else(|| bad_choice("background", &name, "a colour with an RGB value"))?;
                ImageBackground::Color([triplet.red, triplet.green, triplet.blue])
            }
        }));
    }
    if let Ok((r, g, b)) = value.extract::<(u8, u8, u8)>() {
        return Ok(Some(ImageBackground::Color([r, g, b])));
    }
    Err(PyTypeError::new_err(
        "background must be a colour string, an (r, g, b) tuple, \"default\" or \"checkerboard\"",
    ))
}

fn background_object(py: Python<'_>, value: Option<ImageBackground>) -> PyResult<Py<PyAny>> {
    Ok(match value {
        None => py.None(),
        Some(ImageBackground::TerminalDefault) => "default".into_pyobject(py)?.into_any().unbind(),
        Some(ImageBackground::Checkerboard) => {
            "checkerboard".into_pyobject(py)?.into_any().unbind()
        }
        Some(ImageBackground::Color([r, g, b])) => PyTuple::new(py, [r, g, b])?.into_any().unbind(),
    })
}

/// Map a render error to `ImageArtError`, with its `kind`.
pub(crate) fn image_art_error(py: Python<'_>, error: &rich_art::ImageArtError) -> PyErr {
    use rich_art::ImageArtError as E;
    let kind = match error {
        E::FeatureNotEnabled { .. } => "feature_not_enabled",
        E::SixelEncodeFailed => "sixel_encode_failed",
        E::NonTerminalDestination => "non_terminal_destination",
        E::SixelNotSupported => "sixel_not_supported",
        E::SixelTooLarge => "sixel_too_large",
        E::InvalidFitDimensions => "invalid_fit_dimensions",
        E::UnsupportedColorOptions => "unsupported_color_options",
        E::InvalidAdjustment => "invalid_adjustment",
    };
    kinded::<ImageArtError>(py, error.to_string(), kind)
}

// ---------------------------------------------------------------------------
// ImageOptions and RenderCapabilities

/// `rich_art::ImageOptions`: the backend and size shared by every mode.
#[pyclass(name = "ImageOptions", module = "rs_rich.art", frozen)]
pub(crate) struct ImageOptions {
    inner: rich_art::ImageOptions,
}

#[pymethods]
impl ImageOptions {
    #[new]
    #[pyo3(signature = (mode="auto", width=None, height=None, color=false))]
    fn new(mode: &str, width: Option<usize>, height: Option<usize>, color: bool) -> PyResult<Self> {
        Ok(ImageOptions {
            inner: rich_art::ImageOptions {
                mode: image_mode(mode)?,
                width,
                height,
                color,
            },
        })
    }

    #[getter]
    fn mode(&self) -> &'static str {
        image_mode_name(self.inner.mode)
    }

    #[getter]
    fn width(&self) -> Option<usize> {
        self.inner.width
    }

    #[getter]
    fn height(&self) -> Option<usize> {
        self.inner.height
    }

    #[getter]
    fn color(&self) -> bool {
        self.inner.color
    }

    fn __repr__(&self) -> String {
        format!(
            "ImageOptions(mode={}, width={}, height={}, color={})",
            repr_str(self.mode()),
            optional(self.inner.width),
            optional(self.inner.height),
            if self.inner.color { "True" } else { "False" }
        )
    }
}

fn optional(value: Option<usize>) -> String {
    value.map_or_else(|| "None".to_string(), |v| v.to_string())
}

/// `rich_art::RenderCapabilities`: what resolves `mode="auto"`.
#[pyclass(name = "RenderCapabilities", module = "rs_rich.art")]
pub(crate) struct RenderCapabilities {
    #[pyo3(get, set)]
    color: bool,
    #[pyo3(get, set)]
    sixel_supported: bool,
}

#[pymethods]
impl RenderCapabilities {
    #[new]
    #[pyo3(signature = (color=false, sixel_supported=false))]
    fn new(color: bool, sixel_supported: bool) -> Self {
        RenderCapabilities {
            color,
            sixel_supported,
        }
    }

    /// Read them off a console: colour from its colour system, and Sixel
    /// from `RICH_GRAPHICS` / `RICH_SIXEL` and the terminal's name.
    #[staticmethod]
    fn from_console(console: &Bound<'_, PyAny>) -> PyResult<Self> {
        let caps = rich_art::RenderCapabilities::from_console(&core_console(console)?);
        Ok(RenderCapabilities {
            color: caps.color,
            sixel_supported: caps.sixel_supported,
        })
    }

    fn __repr__(&self) -> String {
        let flag = |b: bool| if b { "True" } else { "False" };
        format!(
            "RenderCapabilities(color={}, sixel_supported={})",
            flag(self.color),
            flag(self.sixel_supported)
        )
    }
}

// ---------------------------------------------------------------------------
// ImageArt

/// An image in any mode: `rich_art::ImageArt`. Printing it renders
/// strictly (`ImageArt::render`): what it cannot honour raises
/// `ImageArtError` instead of falling back to ASCII.
#[pyclass(name = "ImageArt", module = "rs_rich.art", frozen)]
pub(crate) struct ImageArt {
    image: Arc<DynamicImage>,
    options: rich_art::ImageOptions,
    fit: Option<ImageFit>,
    anchor: ImageAnchor,
    background: Option<ImageBackground>,
    color_mode: ImageColorMode,
    dither: Dither,
    distance: ColorDistance,
    transforms: ImageTransforms,
    max_width: Option<usize>,
    max_height: Option<usize>,
    built: Arc<rich_art::ImageArt>,
}

/// Prints an `ImageArt` through its strict entry point.
struct StrictImage(Arc<rich_art::ImageArt>);

impl Renderable for StrictImage {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        match self.0.render(console, options) {
            Ok(segments) => segments,
            Err(error) => {
                let error = Python::attach(|py| image_art_error(py, &error));
                fail_render(error, console, options);
                Vec::new()
            }
        }
    }
}

impl AsRenderable for ImageArt {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(StrictImage(Arc::clone(&self.built))))
    }
}

#[pymethods]
impl ImageArt {
    #[new]
    #[pyo3(signature = (
        image, *, mode="auto", width=None, height=None, fit=None, anchor="center",
        background=None, color=false, color_mode="truecolor", dither=None,
        color_distance="rgb", rotate=0, flip_horizontal=false, flip_vertical=false,
        grayscale=false, brightness=1.0, contrast=1.0, gamma=1.0, max_width=None,
        max_height=None, options=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        image: &Bound<'_, PyAny>,
        mode: &str,
        width: Option<usize>,
        height: Option<usize>,
        fit: Option<&str>,
        anchor: &str,
        background: Option<&Bound<'_, PyAny>>,
        color: bool,
        color_mode: &str,
        dither: Option<&str>,
        color_distance: &str,
        rotate: i64,
        flip_horizontal: bool,
        flip_vertical: bool,
        grayscale: bool,
        brightness: f32,
        contrast: f32,
        gamma: f32,
        max_width: Option<usize>,
        max_height: Option<usize>,
        options: Option<PyRef<'_, ImageOptions>>,
    ) -> PyResult<Self> {
        let image = load_image(image)?;
        let options = match options {
            Some(options) => options.inner,
            None => rich_art::ImageOptions {
                mode: image_mode(mode)?,
                width,
                height,
                color,
            },
        };
        let fit = fit.map(image_fit).transpose()?;
        let anchor = image_anchor(anchor)?;
        let background = self::background(background)?;
        let color_mode = self::color_mode(color_mode)?;
        let dither = self::dither(dither)?;
        let distance = self::color_distance(color_distance)?;
        let transforms = ImageTransforms {
            rotation: rotation(rotate)?,
            flip_horizontal,
            flip_vertical,
            grayscale,
            brightness,
            contrast,
            gamma,
        };
        // What fails whatever the console: report it now.
        use rich_art::ImageArtError as E;
        let reduced = color_mode != ImageColorMode::TrueColor;
        let tuned = dither != Dither::None || distance != ColorDistance::Rgb;
        if !transforms.adjustments_valid() {
            return Err(image_art_error(py, &E::InvalidAdjustment));
        }
        if (tuned && !reduced) || (reduced && options.mode == ImageMode::Braille) {
            return Err(image_art_error(py, &E::UnsupportedColorOptions));
        }
        let sized = |v: Option<usize>| v.is_some_and(|v| v > 0);
        if matches!(
            fit,
            Some(ImageFit::Contain | ImageFit::Cover | ImageFit::Stretch)
        ) && !(sized(options.width) && sized(options.height))
        {
            return Err(image_art_error(py, &E::InvalidFitDimensions));
        }
        let mut built = rich_art::ImageArt::new((*image).clone())
            .options(options)
            .anchor(anchor)
            .color_mode(color_mode)
            .dither(dither)
            .color_distance(distance)
            .transforms(transforms);
        if let Some(fit) = fit {
            built = built.fit(fit);
        }
        if let Some(background) = background {
            built = built.background_mode(background);
        }
        if let Some(columns) = max_width {
            built = built.max_width(columns);
        }
        if let Some(rows) = max_height {
            built = built.max_height(rows);
        }
        Ok(ImageArt {
            image,
            options,
            fit,
            anchor,
            background,
            color_mode,
            dither,
            distance,
            transforms,
            max_width,
            max_height,
            built: Arc::new(built),
        })
    }

    /// The backend `mode="auto"` picks for these capabilities (an explicit
    /// mode is returned unchanged).
    fn resolve_mode(&self, capabilities: PyRef<'_, RenderCapabilities>) -> &'static str {
        image_mode_name(self.built.resolve_mode(rich_art::RenderCapabilities {
            color: capabilities.color,
            sixel_supported: capabilities.sixel_supported,
        }))
    }

    /// The `(columns, rows)` grid `fit="native"` renders `mode` at, capped
    /// by `available` columns and the size limits.
    fn native_grid(&self, mode: &str, available: usize) -> PyResult<(usize, usize)> {
        Ok(self.built.native_grid(image_mode(mode)?, available))
    }

    #[getter]
    fn image(&self) -> ArtImage {
        ArtImage {
            inner: Arc::clone(&self.image),
        }
    }

    #[getter]
    fn options(&self) -> ImageOptions {
        ImageOptions {
            inner: self.options,
        }
    }

    #[getter]
    fn mode(&self) -> &'static str {
        image_mode_name(self.options.mode)
    }

    #[getter]
    fn width(&self) -> Option<usize> {
        self.options.width
    }

    #[getter]
    fn height(&self) -> Option<usize> {
        self.options.height
    }

    #[getter]
    fn color(&self) -> bool {
        self.options.color
    }

    #[getter]
    fn fit(&self) -> Option<&'static str> {
        self.fit.map(image_fit_name)
    }

    #[getter]
    fn anchor(&self) -> &'static str {
        image_anchor_name(self.anchor)
    }

    #[getter]
    fn background(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        background_object(py, self.background)
    }

    #[getter]
    fn color_mode(&self) -> &'static str {
        color_mode_name(self.color_mode)
    }

    #[getter]
    fn dither(&self) -> Option<&'static str> {
        dither_name(self.dither)
    }

    #[getter]
    fn color_distance(&self) -> &'static str {
        color_distance_name(self.distance)
    }

    #[getter]
    fn rotate(&self) -> u32 {
        rotation_degrees(self.transforms.rotation)
    }

    #[getter]
    fn flip_horizontal(&self) -> bool {
        self.transforms.flip_horizontal
    }

    #[getter]
    fn flip_vertical(&self) -> bool {
        self.transforms.flip_vertical
    }

    #[getter]
    fn grayscale(&self) -> bool {
        self.transforms.grayscale
    }

    #[getter]
    fn brightness(&self) -> f32 {
        self.transforms.brightness
    }

    #[getter]
    fn contrast(&self) -> f32 {
        self.transforms.contrast
    }

    #[getter]
    fn gamma(&self) -> f32 {
        self.transforms.gamma
    }

    #[getter]
    fn max_width(&self) -> Option<usize> {
        self.max_width
    }

    #[getter]
    fn max_height(&self) -> Option<usize> {
        self.max_height
    }

    fn __repr__(&self) -> String {
        format!(
            "<ImageArt {}x{} mode={}>",
            self.image.width(),
            self.image.height(),
            repr_str(self.mode())
        )
    }
}

// ---------------------------------------------------------------------------
// Single-backend renderables

/// `rich_art::AsciiArt`: a character ramp (`jp2a`-style).
#[pyclass(name = "AsciiArt", module = "rs_rich.art", frozen)]
pub(crate) struct AsciiArt {
    pub(crate) inner: Arc<rich_art::AsciiArt>,
}

impl AsRenderable for AsciiArt {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Shared(Arc::clone(&self.inner))))
    }
}

#[pymethods]
impl AsciiArt {
    #[new]
    #[pyo3(signature = (image, *, width=None, height=None, ramp=None, invert=false, color=false, normalize=true))]
    fn new(
        image: &Bound<'_, PyAny>,
        width: Option<usize>,
        height: Option<usize>,
        ramp: Option<String>,
        invert: bool,
        color: bool,
        normalize: bool,
    ) -> PyResult<Self> {
        let mut art = rich_art::AsciiArt::new(owned_image(image)?)
            .invert(invert)
            .color(color)
            .normalize(normalize);
        if let Some(width) = width {
            art = art.width(width);
        }
        if let Some(height) = height {
            art = art.height(height);
        }
        if let Some(ramp) = ramp {
            art = art.ramp(ramp);
        }
        Ok(AsciiArt {
            inner: Arc::new(art),
        })
    }

    /// Columns the art takes given `available` columns.
    fn columns(&self, available: usize) -> usize {
        self.inner.columns(available)
    }

    /// The art as plain text, laid out for `width` columns.
    fn to_text(&self, width: usize) -> String {
        self.inner.to_text(width)
    }
}

/// `rich_art::BlockArt`: half-block characters, two pixels per cell.
#[pyclass(name = "BlockArt", module = "rs_rich.art", frozen)]
pub(crate) struct BlockArt {
    inner: Arc<rich_art::BlockArt>,
}

impl AsRenderable for BlockArt {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Shared(Arc::clone(&self.inner))))
    }
}

#[pymethods]
impl BlockArt {
    #[new]
    #[pyo3(signature = (image, *, width=None, height=None))]
    fn new(
        image: &Bound<'_, PyAny>,
        width: Option<usize>,
        height: Option<usize>,
    ) -> PyResult<Self> {
        let mut art = rich_art::BlockArt::new(owned_image(image)?);
        if let Some(width) = width {
            art = art.width(width);
        }
        if let Some(height) = height {
            art = art.height(height);
        }
        Ok(BlockArt {
            inner: Arc::new(art),
        })
    }
}

/// `rich_art::BrailleArt`: monochrome Braille dots, 2×4 pixels per cell.
#[pyclass(name = "BrailleArt", module = "rs_rich.art", frozen)]
pub(crate) struct BrailleArt {
    inner: Arc<rich_art::BrailleArt>,
}

impl AsRenderable for BrailleArt {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Shared(Arc::clone(&self.inner))))
    }
}

#[pymethods]
impl BrailleArt {
    #[new]
    #[pyo3(signature = (image, *, width=None, height=None))]
    fn new(
        image: &Bound<'_, PyAny>,
        width: Option<usize>,
        height: Option<usize>,
    ) -> PyResult<Self> {
        let mut art = rich_art::BrailleArt::new(owned_image(image)?);
        if let Some(width) = width {
            art = art.width(width);
        }
        if let Some(height) = height {
            art = art.height(height);
        }
        Ok(BrailleArt {
            inner: Arc::new(art),
        })
    }

    /// The art as plain text, laid out for `width` columns.
    fn to_text(&self, width: usize) -> String {
        self.inner.to_text(width)
    }
}

/// `rich_art::QuadrantArt`: quadrant blocks, 2×2 pixels in two colours per
/// cell.
#[pyclass(name = "QuadrantArt", module = "rs_rich.art", frozen)]
pub(crate) struct QuadrantArt {
    inner: Arc<rich_art::QuadrantArt>,
}

impl AsRenderable for QuadrantArt {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Shared(Arc::clone(&self.inner))))
    }
}

#[pymethods]
impl QuadrantArt {
    #[new]
    #[pyo3(signature = (image, *, width=None, height=None))]
    fn new(
        image: &Bound<'_, PyAny>,
        width: Option<usize>,
        height: Option<usize>,
    ) -> PyResult<Self> {
        let mut art = rich_art::QuadrantArt::new(owned_image(image)?);
        if let Some(width) = width {
            art = art.width(width);
        }
        if let Some(height) = height {
            art = art.height(height);
        }
        Ok(QuadrantArt {
            inner: Arc::new(art),
        })
    }
}

/// `rich_art::SixelArt`: real pixels through the Sixel graphics protocol.
#[pyclass(name = "SixelArt", module = "rs_rich.art", frozen)]
pub(crate) struct SixelArt {
    inner: Arc<rich_art::SixelArt>,
}

impl AsRenderable for SixelArt {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Shared(Arc::clone(&self.inner))))
    }
}

#[pymethods]
impl SixelArt {
    #[new]
    #[pyo3(signature = (image, *, width=None, height=None, cell_px=None, max_colors=256))]
    fn new(
        image: &Bound<'_, PyAny>,
        width: Option<usize>,
        height: Option<usize>,
        cell_px: Option<(u32, u32)>,
        max_colors: u16,
    ) -> PyResult<Self> {
        let mut art = rich_art::SixelArt::new(owned_image(image)?).max_colors(max_colors);
        if let Some(width) = width {
            art = art.width(width);
        }
        if let Some(height) = height {
            art = art.height(height);
        }
        if let Some((w, h)) = cell_px {
            art = art.cell_px(w, h);
        }
        Ok(SixelArt {
            inner: Arc::new(art),
        })
    }

    /// The Sixel escape sequence for `available` columns, or `None` when
    /// it cannot be encoded (or would exceed `SIXEL_MAX_PIXELS`).
    fn encode(&self, py: Python<'_>, available: usize) -> Option<String> {
        let inner = Arc::clone(&self.inner);
        py.detach(move || inner.encode(available))
    }
}

/// `rich_art::sixel::is_probably_supported`: a guess from `RICH_GRAPHICS`,
/// `RICH_SIXEL` and the terminal's name.
#[pyfunction]
fn sixel_is_probably_supported() -> bool {
    rich_art::sixel::is_probably_supported()
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ArtImage>()?;
    m.add_class::<ImageOptions>()?;
    m.add_class::<RenderCapabilities>()?;
    renderable::add_renderable_class::<ImageArt>(m)?;
    renderable::add_renderable_class::<AsciiArt>(m)?;
    renderable::add_renderable_class::<BlockArt>(m)?;
    renderable::add_renderable_class::<BrailleArt>(m)?;
    renderable::add_renderable_class::<QuadrantArt>(m)?;
    renderable::add_renderable_class::<SixelArt>(m)?;
    m.add("DEFAULT_RAMP", rich_art::DEFAULT_RAMP)?;
    m.add("SIXEL_DEFAULT_CELL_PX", rich_art::sixel::DEFAULT_CELL_PX)?;
    m.add("SIXEL_MAX_PIXELS", rich_art::sixel::MAX_PIXELS)?;
    m.add_function(pyo3::wrap_pyfunction!(sixel_is_probably_supported, m)?)?;
    Ok(())
}
