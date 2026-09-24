//! A compact bar for a size relative to a total or a limit.
//!
//! [`SizeBar`] shows how much of a total or limit a size takes: a package's
//! share of a release, a directory's share of a disk, an allocation against a
//! budget. Sizes are formatted with [`format::bytes`](crate::format::bytes)
//! (or [`format::bytes_binary`](crate::format::bytes_binary)), the share
//! with [`format::percent`](crate::format::percent).
//!
//! Over a limit is never shown by colour alone: the part of the bar past the
//! limit uses its own glyph (`▓`, `!` in ASCII) and the line says
//! `over by …`. On an ASCII-only console the bar is drawn with `#`, `.` and
//! `!`.
//!
//! ```
//! use rich::Console;
//! use rich_ext::size_bar::SizeBar;
//!
//! let console = Console::builder().width(60).color_system(None).build();
//! let bar = SizeBar::new(1_500_000, 4_000_000).label("docs").bar_width(10);
//! assert_eq!(
//!     console.render_to_string(&bar).trim_end(),
//!     "docs  ████░░░░░░  1.5 MB / 4.0 MB  38%"
//! );
//!
//! let over = SizeBar::limit(5_000_000, 4_000_000).bar_width(10);
//! assert_eq!(
//!     console.render_to_string(&over).trim_end(),
//!     "████████▓▓  5.0 MB / 4.0 MB  125%  over by 1.0 MB"
//! );
//! ```
//!
//! Styles come from the theme keys in [`STYLES`]; the same values are the
//! fallbacks when a theme lacks them.

use rich::cells::cell_len;
use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Overflow, Renderable, Segment, Style, Text};

use crate::format;

/// Theme keys for size bars, chained into
/// [`extended_theme`](crate::theme::extended_theme).
pub const STYLES: &[(&str, &str)] = &[
    ("size_bar.used", "green"),
    ("size_bar.high", "yellow"),
    ("size_bar.over", "bold red"),
    ("size_bar.free", "bright_black"),
    ("size_bar.label", "bold"),
    ("size_bar.size", "none"),
];

fn theme_style(console: &Console, key: &str) -> Style {
    if let Some(style) = console.theme().get(key) {
        return style.clone();
    }
    STYLES
        .iter()
        .find(|(name, _)| *name == key)
        .and_then(|(_, spec)| Style::parse(spec).ok())
        .unwrap_or_default()
}

/// How byte counts are written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Units {
    /// Base 1000: `1.5 MB` ([`format::bytes`]).
    #[default]
    Decimal,
    /// Base 1024: `1.5 MiB` ([`format::bytes_binary`]).
    Binary,
}

impl Units {
    /// `n` bytes in these units.
    pub fn format(self, n: u64) -> String {
        match self {
            Units::Decimal => format::bytes(n),
            Units::Binary => format::bytes_binary(n),
        }
    }
}

/// Glyphs for the used, free and over-limit cells.
const UNICODE: [&str; 3] = ["█", "░", "▓"];
const ASCII: [&str; 3] = ["#", ".", "!"];

/// Gap between the parts of the line.
const GAP: &str = "  ";
/// The narrowest bar drawn when the line has to shrink.
const MIN_BAR: usize = 4;

/// A size as a bar against a total or limit. See the [module docs](self).
#[derive(Clone, Debug, PartialEq)]
pub struct SizeBar {
    used: u64,
    total: u64,
    label: Option<String>,
    units: Units,
    bar_width: usize,
    warn_at: Option<f64>,
    show_sizes: bool,
    show_percent: bool,
}

impl SizeBar {
    /// `used` against a `total` it is part of: a file in a package, a
    /// directory on a disk.
    pub fn new(used: u64, total: u64) -> Self {
        SizeBar {
            used,
            total,
            label: None,
            units: Units::Decimal,
            bar_width: 20,
            warn_at: None,
            show_sizes: true,
            show_percent: true,
        }
    }

    /// `used` against a `limit` it should stay under: as [`new`](Self::new),
    /// warning from 90% (see [`warn_at`](Self::warn_at)).
    pub fn limit(used: u64, limit: u64) -> Self {
        Self::new(used, limit).warn_at(0.9)
    }

    /// A label before the bar.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Decimal (default) or binary units.
    pub fn units(mut self, units: Units) -> Self {
        self.units = units;
        self
    }

    /// Cells in the bar (default 20; at least 1). It shrinks, down to 4,
    /// when the line would not fit.
    pub fn bar_width(mut self, width: usize) -> Self {
        self.bar_width = width.max(1);
        self
    }

    /// Show the bar in the `size_bar.high` style from this share of the total
    /// (`0.9` = 90%).
    pub fn warn_at(mut self, ratio: f64) -> Self {
        self.warn_at = Some(ratio);
        self
    }

    /// Show `used / total` after the bar (default on).
    pub fn show_sizes(mut self, show: bool) -> Self {
        self.show_sizes = show;
        self
    }

    /// Show the percentage after the bar (default on).
    pub fn show_percent(mut self, show: bool) -> Self {
        self.show_percent = show;
        self
    }

    /// `used / total`; infinite when the total is 0 and something is used.
    pub fn ratio(&self) -> f64 {
        match (self.used, self.total) {
            (0, 0) => 0.0,
            (_, 0) => f64::INFINITY,
            (used, total) => used as f64 / total as f64,
        }
    }

    /// Whether the size is over the total.
    pub fn is_over(&self) -> bool {
        self.used > self.total
    }

    /// Whether the size has reached the [`warn_at`](Self::warn_at) share
    /// without going over.
    pub fn is_high(&self) -> bool {
        !self.is_over() && self.warn_at.is_some_and(|at| self.ratio() >= at)
    }

    /// The text after the bar: sizes, percentage and the over-limit note.
    fn tail(&self) -> Vec<(String, &'static str)> {
        let status = if self.is_over() {
            "size_bar.over"
        } else if self.is_high() {
            "size_bar.high"
        } else {
            "size_bar.size"
        };
        let mut parts = Vec::new();
        if self.show_sizes {
            parts.push((
                format!(
                    "{} / {}",
                    self.units.format(self.used),
                    self.units.format(self.total)
                ),
                "size_bar.size",
            ));
        }
        if self.show_percent {
            parts.push((format::percent(self.ratio(), 0), status));
        }
        if self.is_over() {
            let over = self.units.format(self.used - self.total);
            parts.push((format!("over by {over}"), "size_bar.over"));
        }
        parts
    }

    /// Width of everything but the bar.
    fn fixed_width(&self) -> usize {
        let label = self.label.as_deref().map_or(0, |l| cell_len(l) + GAP.len());
        let tail: usize = self
            .tail()
            .iter()
            .map(|(text, _)| GAP.len() + cell_len(text))
            .sum();
        label + tail
    }

    /// Cells of the bar: (used, free, over).
    fn cells(&self, width: usize) -> (usize, usize, usize) {
        if self.is_over() {
            // The limit sits at total/used of the way along; past it is over.
            let within = if self.total == 0 {
                0
            } else {
                ((width as f64) * self.total as f64 / self.used as f64).floor() as usize
            };
            let within = within.min(width.saturating_sub(1));
            return (within, 0, width - within);
        }
        let mut used = (self.ratio() * width as f64).round() as usize;
        if self.used > 0 {
            used = used.max(1);
        }
        if self.used < self.total {
            used = used.min(width.saturating_sub(1));
        }
        (used, width - used, 0)
    }

    /// The line as styled [`Text`] with a bar `bar` cells wide.
    fn line(&self, console: &Console, bar: usize) -> Text {
        let glyphs = if console.ascii_only() { ASCII } else { UNICODE };
        let used_style = if self.is_over() {
            "size_bar.over"
        } else if self.is_high() {
            "size_bar.high"
        } else {
            "size_bar.used"
        };
        let mut text = Text::new("");
        if let Some(label) = &self.label {
            text.append(label, Some(theme_style(console, "size_bar.label").into()));
            text.append(GAP, None);
        }
        let (used, free, over) = self.cells(bar);
        for (glyph, count, key) in [
            (glyphs[0], used, used_style),
            (glyphs[1], free, "size_bar.free"),
            (glyphs[2], over, "size_bar.over"),
        ] {
            if count > 0 {
                text.append(&glyph.repeat(count), Some(theme_style(console, key).into()));
            }
        }
        for (part, key) in self.tail() {
            text.append(GAP, None);
            text.append(&part, Some(theme_style(console, key).into()));
        }
        text
    }
}

impl Renderable for SizeBar {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let fixed = self.fixed_width();
        let bar = self
            .bar_width
            .min(options.max_width.saturating_sub(fixed))
            .max(MIN_BAR.min(self.bar_width));
        self.line(console, bar)
            .no_wrap(true)
            .overflow(Overflow::Ellipsis)
            .rich_render(console, options)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let fixed = self.fixed_width();
        Measurement::new(
            (fixed + MIN_BAR.min(self.bar_width)).min(options.max_width),
            (fixed + self.bar_width).min(options.max_width),
        )
    }
}
