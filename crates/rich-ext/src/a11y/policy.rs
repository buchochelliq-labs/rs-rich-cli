//! Accessibility presentation policy: user preferences that change how output
//! is presented, independent of what the terminal can do.
//!
//! # Environment
//!
//! * `NO_COLOR` (non-empty) → [`monochrome`](AccessibilityPolicy::monochrome).
//! * `RICH_A11Y` — a comma list of `screen-reader`, `reduced-motion`,
//!   `no-animation`, `high-contrast`, `compact`, `monochrome`, plus a symbol
//!   set: `ascii-symbols` or `word-symbols`. Unknown items are ignored and
//!   listed in [`AccessibilityPolicy::warnings`].

use crate::capabilities::Environment;
use crate::fidelity::{style_without_color, Fidelity, ATTR_NAMES};
use rich::color::ColorType;
use rich::console::ConsoleBuilder;
use rich::{Color, Style, Theme};

/// How status is shown without relying on colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum SymbolSet {
    /// A symbol and a word: `✔ ok`.
    #[default]
    Unicode,
    /// A bracketed tag: `[OK]`.
    Ascii,
    /// A word only: `ok:`.
    Words,
}

/// An outcome to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Status {
    Ok,
    Warning,
    Error,
    Info,
    Pending,
    Skipped,
}

impl Status {
    /// Every status.
    pub const ALL: [Status; 6] = [
        Status::Ok,
        Status::Warning,
        Status::Error,
        Status::Info,
        Status::Pending,
        Status::Skipped,
    ];

    /// The word for this status.
    pub fn word(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warning => "warning",
            Status::Error => "error",
            Status::Info => "info",
            Status::Pending => "pending",
            Status::Skipped => "skipped",
        }
    }

    /// The marker in `set`; every set carries meaning without colour.
    pub fn symbol(self, set: SymbolSet) -> &'static str {
        match (set, self) {
            (SymbolSet::Unicode, Status::Ok) => "✔ ok",
            (SymbolSet::Unicode, Status::Warning) => "⚠ warning",
            (SymbolSet::Unicode, Status::Error) => "✖ error",
            (SymbolSet::Unicode, Status::Info) => "ℹ info",
            (SymbolSet::Unicode, Status::Pending) => "… pending",
            (SymbolSet::Unicode, Status::Skipped) => "↷ skipped",
            (SymbolSet::Ascii, Status::Ok) => "[OK]",
            (SymbolSet::Ascii, Status::Warning) => "[WARN]",
            (SymbolSet::Ascii, Status::Error) => "[ERROR]",
            (SymbolSet::Ascii, Status::Info) => "[INFO]",
            (SymbolSet::Ascii, Status::Pending) => "[PENDING]",
            (SymbolSet::Ascii, Status::Skipped) => "[SKIP]",
            (SymbolSet::Words, Status::Ok) => "ok:",
            (SymbolSet::Words, Status::Warning) => "warning:",
            (SymbolSet::Words, Status::Error) => "error:",
            (SymbolSet::Words, Status::Info) => "info:",
            (SymbolSet::Words, Status::Pending) => "pending:",
            (SymbolSet::Words, Status::Skipped) => "skipped:",
        }
    }

    /// `"<symbol> <message>"`.
    pub fn label(self, set: SymbolSet, message: &str) -> String {
        format!("{} {message}", self.symbol(set))
    }
}

/// User presentation preferences.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct AccessibilityPolicy {
    /// Prefer dense layouts (less padding, fewer blank lines).
    pub compact: bool,
    /// Output is read by a screen reader: linear, undecorated text.
    pub screen_reader: bool,
    /// Never animate.
    pub no_animation: bool,
    /// Avoid motion (spinners, progress animation) where possible.
    pub reduced_motion: bool,
    /// Replace dim and grey styles with readable ones.
    pub high_contrast: bool,
    /// No colour; emphasis through attributes only.
    pub monochrome: bool,
    /// How statuses are marked.
    pub status_symbols: SymbolSet,
    /// `RICH_A11Y` items that were not understood.
    pub warnings: Vec<String>,
}

impl AccessibilityPolicy {
    /// Linear, undecorated, still output with word markers.
    pub fn screen_reader() -> Self {
        Self {
            compact: true,
            screen_reader: true,
            no_animation: true,
            reduced_motion: true,
            status_symbols: SymbolSet::Words,
            ..Self::default()
        }
    }
    /// No animation or motion.
    pub fn reduced_motion() -> Self {
        Self {
            no_animation: true,
            reduced_motion: true,
            ..Self::default()
        }
    }
    /// Readable colours, no dim text.
    pub fn high_contrast() -> Self {
        Self {
            high_contrast: true,
            ..Self::default()
        }
    }
    /// No colour, attributes kept, bracketed status tags.
    pub fn monochrome() -> Self {
        Self {
            monochrome: true,
            status_symbols: SymbolSet::Ascii,
            ..Self::default()
        }
    }

    /// Read `NO_COLOR` and `RICH_A11Y` (see the [module docs](self)).
    pub fn from_env(env: &dyn Environment) -> Self {
        let mut policy = Self::default();
        if env.var("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            policy.monochrome = true;
        }
        let list = env.var("RICH_A11Y").unwrap_or_default();
        for item in list.split(',').map(str::trim).filter(|i| !i.is_empty()) {
            match item.to_ascii_lowercase().as_str() {
                "screen-reader" => {
                    let warnings = std::mem::take(&mut policy.warnings);
                    let monochrome = policy.monochrome;
                    let high_contrast = policy.high_contrast;
                    policy = Self::screen_reader();
                    policy.warnings = warnings;
                    policy.monochrome |= monochrome;
                    policy.high_contrast |= high_contrast;
                }
                "reduced-motion" => policy.reduced_motion = true,
                "no-animation" => policy.no_animation = true,
                "high-contrast" => policy.high_contrast = true,
                "compact" => policy.compact = true,
                "monochrome" => policy.monochrome = true,
                "ascii-symbols" => policy.status_symbols = SymbolSet::Ascii,
                "word-symbols" => policy.status_symbols = SymbolSet::Words,
                other => policy
                    .warnings
                    .push(format!("ignored RICH_A11Y item {other:?}")),
            }
        }
        policy
    }

    /// The highest fidelity this policy allows: screen reader → `Plain`,
    /// monochrome → `Styled`, no animation or reduced motion → `Rich`,
    /// otherwise `Animated`.
    pub fn fidelity_ceiling(&self) -> Fidelity {
        if self.screen_reader {
            Fidelity::Plain
        } else if self.monochrome {
            Fidelity::Styled
        } else if self.no_animation || self.reduced_motion {
            Fidelity::Rich
        } else {
            Fidelity::Animated
        }
    }

    /// A fidelity [`Policy`](crate::fidelity::Policy) carrying this ceiling.
    pub fn fidelity_policy(&self) -> crate::fidelity::Policy {
        crate::fidelity::Policy {
            ceiling: Some(self.fidelity_ceiling()),
            floor: None,
            allow_animation: !(self.no_animation || self.reduced_motion),
        }
    }

    /// Whether statuses should be marked with `status`.
    pub fn status(&self, status: Status) -> &'static str {
        status.symbol(self.status_symbols)
    }

    /// `theme` adjusted by this policy.
    ///
    /// * high contrast: `dim` is dropped; black, bright black and grey
    ///   foregrounds become the terminal default (which contrasts with the
    ///   terminal's own background whatever it is); dark blue becomes bright blue.
    /// * monochrome: colours are removed, but bold/underline/reverse and every
    ///   other attribute stay so emphasis survives.
    pub fn theme(&self, theme: &Theme) -> Theme {
        let mut out = Theme::new();
        for name in theme.names() {
            let Some(style) = theme.get(name) else {
                continue;
            };
            let mut style = style.clone();
            if self.high_contrast {
                style = high_contrast_style(&style);
            }
            if self.monochrome {
                style = style_without_color(&style);
            }
            out.insert(name, style);
        }
        out
    }

    /// Apply this policy to a console builder: the adjusted default theme,
    /// no colour when monochrome, and no emoji or highlighting for screen
    /// readers.
    pub fn console_builder(&self, builder: ConsoleBuilder) -> ConsoleBuilder {
        self.console_builder_with_theme(builder, &Theme::default_theme())
    }

    /// As [`console_builder`](Self::console_builder) with a base theme.
    pub fn console_builder_with_theme(
        &self,
        mut builder: ConsoleBuilder,
        theme: &Theme,
    ) -> ConsoleBuilder {
        builder = builder.theme(self.theme(theme));
        if self.monochrome {
            builder = builder.no_color(true);
        }
        if self.screen_reader {
            builder = builder.emoji(false).highlight(false);
        }
        builder
    }
}

fn is_grey(color: &Color) -> bool {
    match color.kind {
        ColorType::Standard | ColorType::Windows => matches!(color.number, Some(0 | 8)),
        ColorType::EightBit => matches!(color.number, Some(0 | 8 | 16 | 232..=250)),
        ColorType::Truecolor => color.triplet.is_some_and(|t| {
            let (max, min) = (
                t.red.max(t.green).max(t.blue),
                t.red.min(t.green).min(t.blue),
            );
            max - min < 24 && max < 200
        }),
        ColorType::Default => false,
    }
}

fn high_contrast_style(style: &Style) -> Style {
    let words: Vec<String> = ATTR_NAMES
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 1)
        .filter_map(|(i, name)| match style.attr(i) {
            Some(true) => Some((*name).to_owned()),
            Some(false) => Some(format!("not {name}")),
            None => None,
        })
        .collect();
    let mut out = if words.is_empty() {
        Style::new()
    } else {
        Style::parse(&words.join(" ")).unwrap_or_default()
    };
    if let Some(color) = style.color() {
        let color = if is_grey(color) {
            Color::default_color()
        } else if matches!(color.kind, ColorType::Standard) && color.number == Some(4) {
            Color::parse("bright_blue").unwrap_or_else(|_| color.clone())
        } else {
            color.clone()
        };
        out = out.with_color(color);
    }
    if let Some(bg) = style.bgcolor() {
        out = out.with_bgcolor(bg.clone());
    }
    out.update_link(style.link().map(str::to_owned))
}
