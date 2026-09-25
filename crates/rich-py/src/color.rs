//! `rich.color`, `rich.emoji` and `rich.markup` (beyond `escape`): `Color`,
//! `ColorTriplet`, `ColorSystem`, `ColorType`, `ColorParseError`, `Emoji`,
//! `NoEmoji`, `Tag` and markup `render`, plus the `Span` tuple of
//! `rich.text`.
//!
//! Owner: the text/style area (with `text.rs`, `style.rs` and `theme.rs`).
//!
//! Rich defines `ColorTriplet`, `Color`, `Span` and `Tag` as `NamedTuple`s
//! and `ColorSystem`/`ColorType` as `IntEnum`s, and code relies on that
//! (unpacking, comparing with plain tuples and ints). So these few value
//! types are real Python `NamedTuple`/`IntEnum` classes, defined by the small
//! Python source in [`TYPES`] when the module loads. They hold data only:
//! every method that computes anything (parsing, downgrading, SGR codes,
//! palettes) calls a Rust function here, which uses core `rich::color`.

pub(crate) mod emoji;
pub(crate) mod markup;

use std::ffi::CString;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyTuple, PyType};

use rich::color::{Color as CoreColor, ColorSystem as CoreSystem, ColorTriplet, ColorType as Kind};

pub(crate) use emoji::emoji_replace;

create_exception!(_native, ColorParseError, PyException);

/// The value types, as Rich declares them. `_native` is this extension
/// module; the methods call its private `_color_*` functions.
const TYPES: &str = r##"
from enum import IntEnum
from typing import Any, NamedTuple, Optional, Tuple, Union


class ColorSystem(IntEnum):
    """One of the 3 color system supported by terminals."""

    STANDARD = 1
    EIGHT_BIT = 2
    TRUECOLOR = 3
    WINDOWS = 4

    def __repr__(self) -> str:
        return f"ColorSystem.{self.name}"

    def __str__(self) -> str:
        return repr(self)


class ColorType(IntEnum):
    """Type of color stored in Color class."""

    DEFAULT = 0
    STANDARD = 1
    EIGHT_BIT = 2
    TRUECOLOR = 3
    WINDOWS = 4

    def __repr__(self) -> str:
        return f"ColorType.{self.name}"


class ColorTriplet(NamedTuple):
    """The red, green, and blue components of a color."""

    red: int
    green: int
    blue: int

    @property
    def hex(self) -> str:
        red, green, blue = self
        return f"#{red:02x}{green:02x}{blue:02x}"

    @property
    def rgb(self) -> str:
        red, green, blue = self
        return f"rgb({red},{green},{blue})"

    @property
    def normalized(self) -> Tuple[float, float, float]:
        red, green, blue = self
        return red / 255.0, green / 255.0, blue / 255.0


class Color(NamedTuple):
    """Terminal color definition."""

    name: str
    type: ColorType
    number: Optional[int] = None
    triplet: Optional[ColorTriplet] = None

    def __repr__(self) -> str:
        args = [repr(self.name), repr(self.type)]
        if self.number is not None:
            args.append(f"number={self.number!r}")
        if self.triplet is not None:
            args.append(f"triplet={self.triplet!r}")
        return f"Color({', '.join(args)})"

    def __rich__(self) -> Any:
        return _native.Text.assemble(
            f"<color {self.name!r} ({self.type.name.lower()})",
            ("⬤", _native.Style(color=self)),
            " >",
        )

    @property
    def system(self) -> ColorSystem:
        if self.type == ColorType.DEFAULT:
            return ColorSystem.STANDARD
        return ColorSystem(int(self.type))

    @property
    def is_system_defined(self) -> bool:
        return self.system not in (ColorSystem.EIGHT_BIT, ColorSystem.TRUECOLOR)

    @property
    def is_default(self) -> bool:
        return self.type == ColorType.DEFAULT

    def get_truecolor(self, theme: Any = None, foreground: bool = True) -> ColorTriplet:
        return _native._color_truecolor(self, theme, foreground)

    @classmethod
    def from_ansi(cls, number: int) -> "Color":
        return cls(
            name=f"color({number})",
            type=(ColorType.STANDARD if number < 16 else ColorType.EIGHT_BIT),
            number=number,
        )

    @classmethod
    def from_triplet(cls, triplet: ColorTriplet) -> "Color":
        return cls(name=triplet.hex, type=ColorType.TRUECOLOR, triplet=triplet)

    @classmethod
    def from_rgb(cls, red: float, green: float, blue: float) -> "Color":
        return cls.from_triplet(ColorTriplet(int(red), int(green), int(blue)))

    @classmethod
    def default(cls) -> "Color":
        return cls(name="default", type=ColorType.DEFAULT)

    @classmethod
    def parse(cls, color: str) -> "Color":
        return _native._color_parse(color)

    def get_ansi_codes(self, foreground: bool = True) -> Tuple[str, ...]:
        return _native._color_ansi_codes(self, foreground)

    def downgrade(self, system: ColorSystem) -> "Color":
        return _native._color_downgrade(self, system)


class Span(NamedTuple):
    """A marked up region in some text."""

    start: int
    end: int
    style: Union[str, Any]

    def __repr__(self) -> str:
        return f"Span({self.start}, {self.end}, {self.style!r})"

    def __bool__(self) -> bool:
        return self.end > self.start

    def split(self, offset: int) -> Tuple["Span", Optional["Span"]]:
        if offset < self.start:
            return self, None
        if offset >= self.end:
            return self, None
        start, end, style = self
        span1 = Span(start, min(end, offset), style)
        span2 = Span(span1.end, end, style)
        return span1, span2

    def move(self, offset: int) -> "Span":
        start, end, style = self
        return Span(start + offset, end + offset, style)

    def right_crop(self, offset: int) -> "Span":
        start, end, style = self
        if offset >= end:
            return self
        return Span(start, min(offset, end), style)

    def extend(self, cells: int) -> "Span":
        if cells:
            start, end, style = self
            return Span(start, end + cells, style)
        return self


class Tag(NamedTuple):
    """A tag in console markup."""

    name: str
    parameters: Optional[str]

    def __str__(self) -> str:
        return self.name if self.parameters is None else f"{self.name} {self.parameters}"

    @property
    def markup(self) -> str:
        return f"[{self.name}]" if self.parameters is None else f"[{self.name}={self.parameters}]"


ColorSystem.__module__ = ColorType.__module__ = "rs_rich.color"
ColorTriplet.__module__ = Color.__module__ = "rs_rich.color"
Span.__module__ = "rs_rich.text"
Tag.__module__ = "rs_rich.markup"
"##;

/// The classes [`TYPES`] defines, cached at registration.
struct Types {
    color_type: Py<PyType>,
    triplet: Py<PyType>,
    color: Py<PyType>,
    span: Py<PyType>,
}

static TYPES_CELL: PyOnceLock<Types> = PyOnceLock::new();

fn types(py: Python<'_>) -> &'static Types {
    TYPES_CELL
        .get(py)
        .expect("rs_rich._native registers the color types when it loads")
}

/// `rich.text.Span`.
pub(crate) fn span_class(py: Python<'_>) -> Bound<'_, PyType> {
    types(py).span.bind(py).clone()
}

// ---------------------------------------------------------------------------
// Conversions between core colours and Python `Color` tuples

fn kind_number(kind: Kind) -> u8 {
    match kind {
        Kind::Default => 0,
        Kind::Standard => 1,
        Kind::EightBit => 2,
        Kind::Truecolor => 3,
        Kind::Windows => 4,
    }
}

fn kind_from_number(number: i64) -> PyResult<Kind> {
    Ok(match number {
        0 => Kind::Default,
        1 => Kind::Standard,
        2 => Kind::EightBit,
        3 => Kind::Truecolor,
        4 => Kind::Windows,
        other => {
            return Err(PyValueError::new_err(format!(
                "{other} is not a valid ColorType"
            )))
        }
    })
}

/// A core colour system from `ColorSystem` (or its int value).
pub(crate) fn core_system(value: &Bound<'_, PyAny>) -> PyResult<CoreSystem> {
    Ok(match value.extract::<i64>()? {
        1 => CoreSystem::Standard,
        2 => CoreSystem::EightBit,
        3 => CoreSystem::Truecolor,
        4 => CoreSystem::Windows,
        other => {
            return Err(PyValueError::new_err(format!(
                "{other} is not a valid ColorSystem"
            )))
        }
    })
}

/// A `ColorTriplet` for a core triplet.
pub(crate) fn py_triplet<'py>(
    py: Python<'py>,
    triplet: ColorTriplet,
) -> PyResult<Bound<'py, PyAny>> {
    types(py)
        .triplet
        .bind(py)
        .call1((triplet.red, triplet.green, triplet.blue))
}

/// A Python `Color` for a core colour.
pub(crate) fn py_color<'py>(py: Python<'py>, color: &CoreColor) -> PyResult<Bound<'py, PyAny>> {
    let types = types(py);
    let kind = types
        .color_type
        .bind(py)
        .call1((kind_number(color.kind),))?;
    let triplet = match color.triplet {
        Some(triplet) => Some(py_triplet(py, triplet)?),
        None => None,
    };
    types
        .color
        .bind(py)
        .call1((color.name.as_str(), kind, color.number, triplet))
}

/// A core colour from a Python `Color` (any 4-tuple of name, type, number,
/// triplet) or a colour string (parsed, as `Style(color="red")` does).
pub(crate) fn core_color(value: &Bound<'_, PyAny>) -> PyResult<CoreColor> {
    if let Ok(name) = value.extract::<String>() {
        return parse(&name);
    }
    type Fields = (String, i64, Option<u8>, Option<(u8, u8, u8)>);
    let (name, kind, number, triplet): Fields = value
        .extract()
        .map_err(|_| pyo3::exceptions::PyTypeError::new_err("a color must be a str or a Color"))?;
    Ok(CoreColor {
        name,
        kind: kind_from_number(kind)?,
        number,
        triplet: triplet.map(|(red, green, blue)| ColorTriplet::new(red, green, blue)),
    })
}

/// `Color.parse`, raising `ColorParseError` with Rich's messages.
pub(crate) fn parse(color: &str) -> PyResult<CoreColor> {
    CoreColor::parse(color).map_err(|_| ColorParseError::new_err(parse_error_message(color)))
}

/// The message Rich's `Color.parse` raises for `color`.
fn parse_error_message(original: &str) -> String {
    let repr = python_repr(original);
    let color = original.trim().to_lowercase();
    let color_8 = color
        .strip_prefix("color(")
        .and_then(|rest| rest.strip_suffix(')'))
        .filter(|n| (1..=3).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()));
    if color_8.is_some() {
        return format!("color number must be <= 255 in {}", python_repr(&color));
    }
    if let Some(inner) = color
        .strip_prefix("rgb(")
        .and_then(|rest| rest.strip_suffix(')'))
        .filter(|inner| {
            !inner.is_empty()
                && inner
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == ',' || c.is_whitespace())
        })
    {
        let components: Vec<&str> = inner.split(',').collect();
        if components.len() != 3 {
            return format!("expected three components in {repr}");
        }
        return format!("color components must be <= 255 in {repr}");
    }
    format!("{repr} is not a valid color")
}

/// Python's `repr` of a `str` (single quotes unless it contains one).
pub(crate) fn python_repr(value: &str) -> String {
    Python::attach(|py| {
        pyo3::types::PyString::new(py, value)
            .repr()
            .map(|repr| repr.to_string())
            .unwrap_or_else(|_| format!("{value:?}"))
    })
}

// ---------------------------------------------------------------------------
// The functions the Python `Color` methods call

#[pyfunction]
fn _color_parse<'py>(py: Python<'py>, color: &str) -> PyResult<Bound<'py, PyAny>> {
    py_color(py, &parse(color)?)
}

#[pyfunction]
#[pyo3(signature = (color, foreground=true))]
fn _color_ansi_codes<'py>(
    py: Python<'py>,
    color: &Bound<'py, PyAny>,
    foreground: bool,
) -> PyResult<Bound<'py, PyTuple>> {
    PyTuple::new(py, core_color(color)?.ansi_codes(foreground))
}

/// Rich's `WINDOWS_PALETTE` (`rich/_palettes.py`), which core does not carry.
const WINDOWS_PALETTE: [(u8, u8, u8); 16] = [
    (12, 12, 12),
    (197, 15, 31),
    (19, 161, 14),
    (193, 156, 0),
    (0, 55, 218),
    (136, 23, 152),
    (58, 150, 221),
    (204, 204, 204),
    (118, 118, 118),
    (231, 72, 86),
    (22, 198, 12),
    (249, 241, 165),
    (59, 120, 255),
    (180, 0, 158),
    (97, 214, 214),
    (242, 242, 242),
];

/// Rich's `Palette.match`: the nearest colour by the "redmean" distance.
fn match_windows(red1: i64, green1: i64, blue1: i64) -> u8 {
    let distance = |(red2, green2, blue2): (u8, u8, u8)| {
        let (red2, green2, blue2) = (red2 as i64, green2 as i64, blue2 as i64);
        let red_mean = (red1 + red2) / 2;
        let red = red1 - red2;
        let green = green1 - green2;
        let blue = blue1 - blue2;
        let squared = (((512 + red_mean) * red * red) >> 8)
            + 4 * green * green
            + (((767 - red_mean) * blue * blue) >> 8);
        (squared as f64).sqrt()
    };
    let mut best = 0;
    for (index, colour) in WINDOWS_PALETTE.iter().enumerate() {
        if distance(*colour) < distance(WINDOWS_PALETTE[best]) {
            best = index;
        }
    }
    best as u8
}

#[pyfunction]
#[pyo3(signature = (color, theme=None, foreground=true))]
fn _color_truecolor<'py>(
    py: Python<'py>,
    color: &Bound<'py, PyAny>,
    theme: Option<&Bound<'py, PyAny>>,
    foreground: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let core = core_color(color)?;
    let theme = match theme.filter(|theme| !theme.is_none()) {
        Some(theme) => theme.clone(),
        None => py
            .import("rs_rich.terminal_theme")?
            .getattr("DEFAULT_TERMINAL_THEME")?,
    };
    let triplet = |value: Bound<'py, PyAny>| -> PyResult<Bound<'py, PyAny>> {
        let (red, green, blue): (u8, u8, u8) = value.extract()?;
        py_triplet(py, ColorTriplet::new(red, green, blue))
    };
    let number = core.number.unwrap_or(0) as usize;
    match core.kind {
        Kind::Truecolor => match core.triplet {
            Some(value) => py_triplet(py, value),
            None => Ok(py.None().into_bound(py)),
        },
        Kind::EightBit => {
            let value = CoreColor::from_ansi(number as u8)
                .get_truecolor()
                .unwrap_or(ColorTriplet::new(0, 0, 0));
            // `from_ansi` below 16 is a standard colour; the 8-bit palette's
            // first 16 entries are the same xterm values.
            py_triplet(py, value)
        }
        Kind::Standard => triplet(theme.getattr("ansi_colors")?.get_item(number)?),
        Kind::Windows => {
            let (red, green, blue) = WINDOWS_PALETTE[number.min(15)];
            py_triplet(py, ColorTriplet::new(red, green, blue))
        }
        Kind::Default => triplet(theme.getattr(if foreground {
            "foreground_color"
        } else {
            "background_color"
        })?),
    }
}

/// Rich's `Color.downgrade`, which core matches except for the Windows
/// palette and a few same-system cases.
pub(crate) fn downgrade(core: &CoreColor, system: CoreSystem) -> CoreColor {
    let system_number = match system {
        CoreSystem::Standard => 1,
        CoreSystem::EightBit => 2,
        CoreSystem::Truecolor => 3,
        CoreSystem::Windows => 4,
    };
    // Rich compares the `ColorType` with the `ColorSystem` as ints.
    if core.kind == Kind::Default || kind_number(core.kind) == system_number {
        return core.clone();
    }
    let native = match core.kind {
        Kind::Default | Kind::Standard => 1,
        other => kind_number(other),
    };
    match system {
        CoreSystem::EightBit if native == 3 => core.downgrade(CoreSystem::EightBit),
        CoreSystem::Standard if native == 3 || native == 2 => core.downgrade(CoreSystem::Standard),
        // A Windows colour reads its number in the 8-bit palette, as Rich does.
        CoreSystem::Standard if native == 4 => {
            let mut eight_bit = CoreColor::from_ansi(core.number.unwrap_or(0));
            eight_bit.kind = Kind::EightBit;
            let mut downgraded = eight_bit.downgrade(CoreSystem::Standard);
            downgraded.name = core.name.clone();
            downgraded
        }
        CoreSystem::Windows => {
            let number = core.number.unwrap_or(0);
            let windows = |number: u8| CoreColor {
                name: core.name.clone(),
                kind: Kind::Windows,
                number: Some(number),
                triplet: None,
            };
            if native != 3 && number < 16 {
                return windows(number);
            }
            let triplet = core.get_truecolor().unwrap_or(ColorTriplet::new(0, 0, 0));
            windows(match_windows(
                triplet.red as i64,
                triplet.green as i64,
                triplet.blue as i64,
            ))
        }
        _ => core.clone(),
    }
}

#[pyfunction]
fn _color_downgrade<'py>(
    py: Python<'py>,
    color: &Bound<'py, PyAny>,
    system: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let core = core_color(color)?;
    let downgraded = downgrade(&core, core_system(system)?);
    if downgraded == core {
        return Ok(color.clone());
    }
    py_color(py, &downgraded)
}

/// `rich.color.parse_rgb_hex`: a `ColorTriplet` from six hex digits.
#[pyfunction]
fn parse_rgb_hex<'py>(py: Python<'py>, hex_color: &str) -> PyResult<Bound<'py, PyAny>> {
    if hex_color.len() != 6 {
        return Err(pyo3::exceptions::PyAssertionError::new_err(
            "must be 6 characters",
        ));
    }
    let channel = |range: std::ops::Range<usize>| {
        u8::from_str_radix(&hex_color[range], 16).map_err(|_| {
            PyValueError::new_err(format!(
                "invalid literal for int() with base 16: {}",
                python_repr(hex_color)
            ))
        })
    };
    py_triplet(
        py,
        ColorTriplet::new(channel(0..2)?, channel(2..4)?, channel(4..6)?),
    )
}

/// `rich.color.blend_rgb`: blend one triplet into another.
#[pyfunction]
#[pyo3(signature = (color1, color2, cross_fade=0.5))]
fn blend_rgb<'py>(
    py: Python<'py>,
    color1: (f64, f64, f64),
    color2: (f64, f64, f64),
    cross_fade: f64,
) -> PyResult<Bound<'py, PyAny>> {
    let blend = |a: f64, b: f64| (a + (b - a) * cross_fade) as i64;
    types(py).triplet.bind(py).call1((
        blend(color1.0, color2.0),
        blend(color1.1, color2.1),
        blend(color1.2, color2.2),
    ))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("ColorParseError", py.get_type::<ColorParseError>())?;
    m.add_function(wrap_pyfunction!(_color_parse, m)?)?;
    m.add_function(wrap_pyfunction!(_color_ansi_codes, m)?)?;
    m.add_function(wrap_pyfunction!(_color_truecolor, m)?)?;
    m.add_function(wrap_pyfunction!(_color_downgrade, m)?)?;
    m.add_function(wrap_pyfunction!(parse_rgb_hex, m)?)?;
    m.add_function(wrap_pyfunction!(blend_rgb, m)?)?;

    let globals = PyDict::new(py);
    globals.set_item("__name__", "rs_rich.color")?;
    globals.set_item("_native", m)?;
    let code = CString::new(TYPES).expect("the type definitions hold no NUL");
    py.run(code.as_c_str(), Some(&globals), None)?;
    let class = |name: &str| -> PyResult<Py<PyType>> {
        let value = globals
            .get_item(name)?
            .ok_or_else(|| PyValueError::new_err(format!("{name} was not defined")))?;
        m.add(name, &value)?;
        Ok(value.cast_into::<PyType>()?.unbind())
    };
    class("ColorSystem")?;
    class("Tag")?;
    let defined = Types {
        color_type: class("ColorType")?,
        triplet: class("ColorTriplet")?,
        color: class("Color")?,
        span: class("Span")?,
    };
    let _ = TYPES_CELL.set(py, defined);
    emoji::register(m)?;
    markup::register(m)?;
    Ok(())
}
