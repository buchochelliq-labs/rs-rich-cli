//! Compact badges and status chips for statuses, labels, links and metadata.
//!
//! A [`Badge`] is a short run of text on a coloured background: a label
//! (`beta`), a status with a message (`✔ build`), a link (`docs`) or a
//! key/value pair (`version │ 1.2.0`). [`Badges`] lays several out in a row
//! and wraps between them, never inside one.
//!
//! Colour is never the only signal. When the console shows no colour (no
//! colour system, `NO_COLOR`, or [`Console::no_color`]) a badge becomes a
//! bracketed plain form that carries the same meaning: `[beta]`,
//! `[OK build]`, `[docs <https://…>]`, `[version: 1.2.0]`. Status chips use the
//! markers of [`a11y::Status`](crate::a11y::Status) in the chosen
//! [`SymbolSet`], ASCII tags when the console is ASCII-only.
//!
//! Links are OSC 8 hyperlinks wherever the console renders styles; where it
//! cannot, the URL is written out after the text.
//!
//! ```
//! use rich::{ColorSystem, Console};
//! use rich_ext::a11y::Status;
//! use rich_ext::badge::{Badge, Badges};
//!
//! let row = Badges::new([
//!     Badge::status(Status::Ok, "build"),
//!     Badge::status(Status::Error, "tests"),
//!     Badge::label("beta"),
//!     Badge::meta("version", "1.2.0"),
//! ]);
//!
//! // No colour: brackets and ASCII tags carry the meaning.
//! let plain = Console::builder().width(60).color_system(None).build();
//! assert_eq!(
//!     plain.render_to_string(&row).trim_end(),
//!     "[OK build] [ERROR tests] [beta] [version: 1.2.0]"
//! );
//!
//! // Colour: padded chips on the `badge.*` theme styles.
//! let color = Console::builder()
//!     .width(60)
//!     .force_terminal(true)
//!     .color_system(Some(ColorSystem::Standard))
//!     .theme(rich_ext::extended_theme())
//!     .build();
//! assert!(color.render_to_string(&Badge::label("beta")).starts_with("\x1b[30;47m beta \x1b[0m"));
//! ```
//!
//! Styles come from the theme keys in [`STYLES`]; the same values are the
//! fallbacks when a theme lacks them.

use rich::cells::cell_len;
use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Overflow, Renderable, Segment, Style, Text};

use crate::a11y::{AccessibleText, Status, SymbolSet};

/// Theme keys for badges, chained into
/// [`extended_theme`](crate::theme::extended_theme).
pub const STYLES: &[(&str, &str)] = &[
    ("badge.ok", "bold black on green"),
    ("badge.warning", "bold black on yellow"),
    ("badge.error", "bold bright_white on red"),
    ("badge.info", "bold black on cyan"),
    ("badge.pending", "bright_white on blue"),
    ("badge.skipped", "black on bright_black"),
    ("badge.label", "black on white"),
    ("badge.link", "underline black on bright_blue"),
    ("badge.key", "bright_white on grey30"),
    ("badge.value", "black on bright_cyan"),
];

/// The style for `key`: the console theme's, else this module's default.
fn theme_style(console: &Console, key: &str) -> Style {
    if let Some(style) = console.theme().get(key) {
        return style.clone();
    }
    STYLES
        .iter()
        .find(|(name, _)| *name == key)
        .and_then(|(_, spec)| Style::parse(spec).ok())
        .or_else(|| Style::parse(key).ok())
        .unwrap_or_default()
}

/// Whether colour reaches the output: a colour system and not `no_color`.
fn shows_color(console: &Console) -> bool {
    console.color_system().is_some() && !console.no_color()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Label,
    Status(Status),
    Link(String),
    Meta(String),
}

/// One badge. See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Badge {
    kind: Kind,
    text: String,
    style: Option<String>,
    symbols: Option<SymbolSet>,
    show_url: Option<bool>,
}

impl Badge {
    fn new(kind: Kind, text: impl Into<String>) -> Self {
        Badge {
            kind,
            text: text.into(),
            style: None,
            symbols: None,
            show_url: None,
        }
    }

    /// A label chip: `beta`, `deprecated`, `linux`. Plain form `[beta]`.
    pub fn label(text: impl Into<String>) -> Self {
        Self::new(Kind::Label, text)
    }

    /// A status chip with a message: `✔ build`, plain `[OK build]`. An empty
    /// message shows the status word: `✔ ok`, plain `[OK]`.
    pub fn status(status: Status, message: impl Into<String>) -> Self {
        Self::new(Kind::Status(status), message)
    }

    /// A link chip: the text, linked to `url` with OSC 8. Where the console
    /// renders no styles the plain form spells the URL out: `[docs <url>]`.
    pub fn link(text: impl Into<String>, url: impl Into<String>) -> Self {
        Self::new(Kind::Link(url.into()), text)
    }

    /// A two-part metadata chip: the key on one style, the value on another.
    /// Plain form `[key: value]`.
    pub fn meta(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(Kind::Meta(key.into()), value)
    }

    /// Use this style instead of the kind's theme key: another theme key
    /// (`"badge.warning"`) or a style definition (`"black on magenta"`). For
    /// a metadata chip it styles the value.
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// How a status is marked (default: [`SymbolSet::Unicode`] with colour,
    /// [`SymbolSet::Ascii`] without; ASCII whenever the console is
    /// ASCII-only).
    pub fn symbols(mut self, symbols: SymbolSet) -> Self {
        self.symbols = Some(symbols);
        self
    }

    /// Always write a link's URL after its text (`true`), or never (`false`).
    /// By default it is written only where no hyperlink can be shown.
    pub fn show_url(mut self, show: bool) -> Self {
        self.show_url = Some(show);
        self
    }

    /// The status this badge shows, if it is a status chip.
    pub fn status_of(&self) -> Option<Status> {
        match self.kind {
            Kind::Status(status) => Some(status),
            _ => None,
        }
    }

    /// The plain, colourless form with ASCII markers: `[OK build]`,
    /// `[beta]`, `[docs <https://…>]`, `[version: 1.2.0]`.
    pub fn plain(&self) -> String {
        let set = self.symbols.unwrap_or(SymbolSet::Ascii);
        let set = if set == SymbolSet::Unicode {
            SymbolSet::Ascii
        } else {
            set
        };
        format!("[{}]", self.body(set, self.show_url != Some(false)))
    }

    fn body(&self, set: SymbolSet, url: bool) -> String {
        match &self.kind {
            Kind::Label => self.text.clone(),
            Kind::Status(status) => {
                let marker = marker(*status, set, self.text.is_empty());
                if self.text.is_empty() {
                    marker
                } else {
                    format!("{marker} {}", self.text)
                }
            }
            Kind::Link(target) if url => format!("{} <{target}>", self.text),
            Kind::Link(_) => self.text.clone(),
            Kind::Meta(key) => format!("{key}: {}", self.text),
        }
    }

    fn key(&self) -> String {
        match &self.kind {
            Kind::Label => "badge.label".into(),
            Kind::Status(status) => format!("badge.{}", status.word()),
            Kind::Link(_) => "badge.link".into(),
            Kind::Meta(_) => "badge.value".into(),
        }
    }

    fn resolved_style(&self, console: &Console) -> Style {
        let style = theme_style(console, self.style.as_deref().unwrap_or(&self.key()));
        match &self.kind {
            Kind::Link(url) if console.color_system().is_some() => style.with_link(url.clone()),
            _ => style,
        }
    }

    /// This badge as styled [`Text`] for `console`: the padded colour chip, or
    /// the bracketed plain form when the console shows no colour.
    pub fn to_text(&self, console: &Console) -> Text {
        let style = self.resolved_style(console);
        let links = console.color_system().is_some();
        let url = self
            .show_url
            .unwrap_or(matches!(self.kind, Kind::Link(_)) && !links);
        let default_set = if shows_color(console) {
            SymbolSet::Unicode
        } else {
            SymbolSet::Ascii
        };
        let mut set = self.symbols.unwrap_or(default_set);
        if set == SymbolSet::Unicode && console.ascii_only() {
            set = SymbolSet::Ascii;
        }
        let mut text = Text::new("");
        if !shows_color(console) {
            text.append(&format!("[{}]", self.body(set, url)), Some(style.into()));
            return text;
        }
        if let Kind::Meta(key) = &self.kind {
            let key_style = theme_style(console, "badge.key");
            text.append(&format!(" {key} "), Some(key_style.into()));
        }
        let body = match self.kind {
            Kind::Meta(_) => self.text.clone(),
            _ => self.body(set, url),
        };
        text.append(&format!(" {body} "), Some(style.into()));
        text
    }
}

/// The marker for `status` in `set`, alone (`bare`) or before a message.
fn marker(status: Status, set: SymbolSet, bare: bool) -> String {
    let symbol = status.symbol(set);
    match set {
        SymbolSet::Unicode if bare => symbol.to_string(),
        SymbolSet::Unicode => symbol.split(' ').next().unwrap_or(symbol).to_string(),
        SymbolSet::Ascii => symbol.trim_matches(['[', ']']).to_string(),
        SymbolSet::Words if bare => symbol.trim_end_matches(':').to_string(),
        SymbolSet::Words => symbol.to_string(),
    }
}

fn render_line(text: Text, options: &ConsoleOptions, console: &Console) -> Vec<Segment> {
    text.no_wrap(true)
        .overflow(Overflow::Ellipsis)
        .rich_render(console, options)
}

impl Renderable for Badge {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        render_line(self.to_text(console), options, console)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let width = self.to_text(console).cell_len().min(options.max_width);
        Measurement::new(width, width)
    }
}

impl AccessibleText for Badge {
    fn accessible_text(&self, _width: usize) -> String {
        self.plain()
    }
}

/// A row of badges, wrapped between badges to the available width.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Badges {
    badges: Vec<Badge>,
    separator: String,
}

impl Badges {
    /// These badges, separated by one space.
    pub fn new(badges: impl IntoIterator<Item = Badge>) -> Self {
        Badges {
            badges: badges.into_iter().collect(),
            separator: " ".into(),
        }
    }

    /// Add a badge.
    pub fn push(mut self, badge: Badge) -> Self {
        self.badges.push(badge);
        self
    }

    /// The text between badges (default one space).
    pub fn separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// The badges in this row.
    pub fn badges(&self) -> &[Badge] {
        &self.badges
    }

    /// The plain forms joined by the separator.
    pub fn plain(&self) -> String {
        self.badges
            .iter()
            .map(Badge::plain)
            .collect::<Vec<_>>()
            .join(&self.separator)
    }

    /// The row on one line as styled [`Text`].
    pub fn to_text(&self, console: &Console) -> Text {
        let mut text = Text::new("");
        for (index, badge) in self.badges.iter().enumerate() {
            if index > 0 {
                text.append(&self.separator, None);
            }
            text = text.append_text(&badge.to_text(console));
        }
        text
    }
}

impl Renderable for Badges {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let separator = cell_len(&self.separator);
        let mut lines: Vec<Text> = Vec::new();
        let mut line = Text::new("");
        for badge in &self.badges {
            let chip = badge.to_text(console);
            let used = line.cell_len();
            if used > 0 && used + separator + chip.cell_len() > width {
                lines.push(std::mem::replace(&mut line, Text::new("")));
            }
            if line.cell_len() > 0 {
                line.append(&self.separator, None);
            }
            line = line.append_text(&chip);
        }
        lines.push(line);
        let mut out = Vec::new();
        for (index, line) in lines.into_iter().enumerate() {
            if index > 0 {
                out.push(Segment::line());
            }
            out.extend(render_line(line, options, console));
        }
        out
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let widths: Vec<usize> = self
            .badges
            .iter()
            .map(|b| b.to_text(console).cell_len())
            .collect();
        let minimum = widths.iter().copied().max().unwrap_or(0);
        let maximum = widths.iter().sum::<usize>()
            + cell_len(&self.separator) * widths.len().saturating_sub(1);
        Measurement::new(
            minimum.min(options.max_width),
            maximum.min(options.max_width),
        )
    }
}

impl AccessibleText for Badges {
    fn accessible_text(&self, _width: usize) -> String {
        self.plain()
    }
}
