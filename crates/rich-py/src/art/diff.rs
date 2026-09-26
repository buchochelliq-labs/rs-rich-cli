//! Perceptual image diffs: `image_diff`, `DiffSettings`, `DiffReport` and
//! `DiffRegion` (`rich_art::imagediff`).

use std::sync::Arc;

use pyo3::prelude::*;

use rich_art::imagediff::{DiffError, DiffReport as CoreReport, DiffSettings as CoreSettings};

use super::image::{load_image, ArtImage};
use super::{kinded, repr_float, ImageDiffError};

/// Tuning for `image_diff` (the defaults match the reference oracle's).
#[pyclass(name = "DiffSettings", module = "rs_rich.art")]
pub(crate) struct DiffSettings {
    #[pyo3(get, set)]
    blur: f32,
    #[pyo3(get, set)]
    threshold: f32,
    #[pyo3(get, set)]
    open_kernel: usize,
    #[pyo3(get, set)]
    min_region: u64,
    #[pyo3(get, set)]
    top: usize,
}

impl DiffSettings {
    fn to_core(&self) -> CoreSettings {
        CoreSettings {
            blur: self.blur,
            threshold: self.threshold,
            open_kernel: self.open_kernel,
            min_region: self.min_region,
            top: self.top,
        }
    }
}

#[pymethods]
impl DiffSettings {
    #[new]
    #[pyo3(signature = (*, blur=None, threshold=None, open_kernel=None, min_region=None, top=None))]
    fn new(
        blur: Option<f32>,
        threshold: Option<f32>,
        open_kernel: Option<usize>,
        min_region: Option<u64>,
        top: Option<usize>,
    ) -> Self {
        let default = CoreSettings::default();
        DiffSettings {
            blur: blur.unwrap_or(default.blur),
            threshold: threshold.unwrap_or(default.threshold),
            open_kernel: open_kernel.unwrap_or(default.open_kernel),
            min_region: min_region.unwrap_or(default.min_region),
            top: top.unwrap_or(default.top),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "DiffSettings(blur={}, threshold={}, open_kernel={}, min_region={}, top={})",
            repr_float(f64::from(self.blur)),
            repr_float(f64::from(self.threshold)),
            self.open_kernel,
            self.min_region,
            self.top
        )
    }
}

/// One connected area of perceptible change.
#[pyclass(name = "DiffRegion", module = "rs_rich.art", frozen)]
pub(crate) struct DiffRegion {
    #[pyo3(get)]
    x: u32,
    #[pyo3(get)]
    y: u32,
    #[pyo3(get)]
    width: u32,
    #[pyo3(get)]
    height: u32,
    #[pyo3(get)]
    area_px: u64,
    #[pyo3(get)]
    share_of_change: f32,
    #[pyo3(get)]
    mean_delta_e: f32,
}

#[pymethods]
impl DiffRegion {
    fn __repr__(&self) -> String {
        format!(
            "DiffRegion(x={}, y={}, width={}, height={}, area_px={}, share_of_change={}, \
             mean_delta_e={})",
            self.x,
            self.y,
            self.width,
            self.height,
            self.area_px,
            repr_float(f64::from(self.share_of_change)),
            repr_float(f64::from(self.mean_delta_e))
        )
    }
}

/// What `image_diff` found.
#[pyclass(name = "DiffReport", module = "rs_rich.art", frozen)]
pub(crate) struct DiffReport {
    inner: Arc<CoreReport>,
}

#[pymethods]
impl DiffReport {
    #[getter]
    fn width(&self) -> u32 {
        self.inner.width
    }

    #[getter]
    fn height(&self) -> u32 {
        self.inner.height
    }

    /// Fraction of pixels over the threshold, before denoising.
    #[getter]
    fn changed_fraction(&self) -> f32 {
        self.inner.changed_fraction
    }

    /// Fraction a plain byte comparison would call changed.
    #[getter]
    fn naive_changed_fraction(&self) -> f32 {
        self.inner.naive_changed_fraction
    }

    #[getter]
    fn mean_delta_e(&self) -> f32 {
        self.inner.mean_delta_e
    }

    #[getter]
    fn max_delta_e(&self) -> f32 {
        self.inner.max_delta_e
    }

    /// The regions of change, largest (by area × severity) first.
    #[getter]
    fn regions(&self) -> Vec<DiffRegion> {
        self.inner
            .regions
            .iter()
            .map(|r| DiffRegion {
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
                area_px: r.area_px,
                share_of_change: r.share_of_change,
                mean_delta_e: r.mean_delta_e,
            })
            .collect()
    }

    /// ΔE per pixel, row by row (`width * height` values).
    #[getter]
    fn delta_e(&self) -> Vec<f32> {
        self.inner.delta_e.clone()
    }

    /// The ΔE map as a heat image.
    fn heatmap(&self, py: Python<'_>) -> ArtImage {
        let report = Arc::clone(&self.inner);
        ArtImage::wrap(py.detach(move || report.heatmap()))
    }

    /// `after` dimmed, with the regions left at full brightness.
    fn highlight(&self, py: Python<'_>, after: &Bound<'_, PyAny>) -> PyResult<ArtImage> {
        let after = load_image(after)?;
        let report = Arc::clone(&self.inner);
        Ok(ArtImage::wrap(py.detach(move || report.highlight(&after))))
    }

    fn __repr__(&self) -> String {
        format!(
            "<DiffReport {}x{} changed={} regions={}>",
            self.inner.width,
            self.inner.height,
            repr_float(f64::from(self.inner.changed_fraction)),
            self.inner.regions.len()
        )
    }
}

/// `rich_art::diff`: compare two images perceptually (CIELAB ΔE, denoised,
/// ranked regions). Images of different sizes raise `ImageDiffError`.
#[pyfunction]
#[pyo3(signature = (
    before, after, settings=None, *, blur=None, threshold=None, open_kernel=None,
    min_region=None, top=None
))]
#[allow(clippy::too_many_arguments)]
fn image_diff(
    py: Python<'_>,
    before: &Bound<'_, PyAny>,
    after: &Bound<'_, PyAny>,
    settings: Option<PyRef<'_, DiffSettings>>,
    blur: Option<f32>,
    threshold: Option<f32>,
    open_kernel: Option<usize>,
    min_region: Option<u64>,
    top: Option<usize>,
) -> PyResult<DiffReport> {
    let mut core = match settings {
        Some(settings) => settings.to_core(),
        None => CoreSettings::default(),
    };
    core.blur = blur.unwrap_or(core.blur);
    core.threshold = threshold.unwrap_or(core.threshold);
    core.open_kernel = open_kernel.unwrap_or(core.open_kernel);
    core.min_region = min_region.unwrap_or(core.min_region);
    core.top = top.unwrap_or(core.top);
    let (before, after) = (load_image(before)?, load_image(after)?);
    let report = py
        .detach(move || rich_art::diff(&before, &after, &core))
        .map_err(|error| match &error {
            DiffError::SizeMismatch { .. } => {
                kinded::<ImageDiffError>(py, error.to_string(), "size_mismatch")
            }
        })?;
    Ok(DiffReport {
        inner: Arc::new(report),
    })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<DiffSettings>()?;
    m.add_class::<DiffRegion>()?;
    m.add_class::<DiffReport>()?;
    m.add_function(pyo3::wrap_pyfunction!(image_diff, m)?)?;
    Ok(())
}
