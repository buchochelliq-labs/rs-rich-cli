//! Graceful degradation: pick how rich output may be, then render at that level.
//!
//! [`Fidelity`] runs from [`Animated`](Fidelity::Animated) down to
//! [`Ascii`](Fidelity::Ascii). [`Fidelity::select`] maps capabilities onto a
//! level; a [`Degradable`] renderable says which levels it renders natively and
//! [`Adaptive`] picks the best one. [`Degrade`] makes any renderable degradable
//! by post-processing its segments with [`strip_color`], [`strip_styles`] and
//! [`ascii_fallback`].

use crate::capabilities::Report;
use rich::cells::char_cell_width;
use rich::protocol::{ConsoleEnvironment, TargetCapabilities};
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

/// How much of the terminal's feature set output may use. `Ord` runs from low
/// ([`Ascii`](Self::Ascii)) to high ([`Animated`](Self::Animated)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Fidelity {
    /// ASCII-only glyphs, no styles.
    Ascii,
    /// Unicode glyphs, no styles.
    Plain,
    /// Unicode with bold/italic/underline, but no colour.
    Styled,
    /// Static full colour.
    Rich,
    /// Full colour plus live updates and animation.
    Animated,
}

impl Fidelity {
    /// Every level, high to low.
    pub const ALL: [Fidelity; 5] = [
        Fidelity::Animated,
        Fidelity::Rich,
        Fidelity::Styled,
        Fidelity::Plain,
        Fidelity::Ascii,
    ];

    /// The lowercase name.
    pub fn name(self) -> &'static str {
        match self {
            Fidelity::Animated => "animated",
            Fidelity::Rich => "rich",
            Fidelity::Styled => "styled",
            Fidelity::Plain => "plain",
            Fidelity::Ascii => "ascii",
        }
    }

    /// Select a level. Rules, in order:
    ///
    /// 1. no Unicode → `Ascii`;
    /// 2. no colour → `Styled` on an interactive terminal (attributes still
    ///    work there; `NO_COLOR` only removes colour), `Plain` elsewhere;
    /// 3. colour, animation allowed and interactive → `Animated`;
    /// 4. otherwise → `Rich`.
    ///
    /// Then the policy's `ceiling` caps the result and its `floor` raises it
    /// (the floor wins over capabilities: the caller insists).
    pub fn select(source: &dyn FidelitySource, policy: &Policy) -> Fidelity {
        let facts = source.facts();
        let natural = if !facts.unicode {
            Fidelity::Ascii
        } else if !facts.color {
            if facts.interactive {
                Fidelity::Styled
            } else {
                Fidelity::Plain
            }
        } else if facts.animation && facts.interactive && policy.allow_animation {
            Fidelity::Animated
        } else {
            Fidelity::Rich
        };
        policy.clamp(natural)
    }

    /// Select a level for a console: its render environment when one is
    /// attached, else its own settings.
    pub fn for_console(console: &Console, policy: &Policy) -> Fidelity {
        match console.render_environment() {
            Some(env) => Self::select(&env.capabilities(), policy),
            None => Self::select(&ConsoleFacts(console), policy),
        }
    }
}

/// The facts [`Fidelity::select`] needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FidelityFacts {
    pub unicode: bool,
    pub color: bool,
    pub interactive: bool,
    pub animation: bool,
}

/// Anything fidelity can be selected from.
pub trait FidelitySource {
    fn facts(&self) -> FidelityFacts;
}

impl FidelitySource for FidelityFacts {
    fn facts(&self) -> FidelityFacts {
        *self
    }
}

impl FidelitySource for Report {
    fn facts(&self) -> FidelityFacts {
        FidelityFacts {
            unicode: self.unicode.value,
            color: self.color.value.color_system().is_some(),
            interactive: self.interactive.value,
            animation: self.animation.value,
        }
    }
}

/// Core capabilities carry no animation fact; an interactive target animates.
impl FidelitySource for TargetCapabilities {
    fn facts(&self) -> FidelityFacts {
        FidelityFacts {
            unicode: self.unicode,
            color: self.color_system.is_some(),
            interactive: self.interactive,
            animation: self.interactive,
        }
    }
}

struct ConsoleFacts<'a>(&'a Console);

impl FidelitySource for ConsoleFacts<'_> {
    fn facts(&self) -> FidelityFacts {
        let c = self.0;
        FidelityFacts {
            unicode: !c.ascii_only(),
            color: c.color_system().is_some() && !c.no_color(),
            interactive: c.is_terminal(),
            animation: c.is_terminal(),
        }
    }
}

/// Caller limits on [`Fidelity::select`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    /// Never go above this level.
    pub ceiling: Option<Fidelity>,
    /// Never go below this level.
    pub floor: Option<Fidelity>,
    /// Allow [`Fidelity::Animated`] at all.
    pub allow_animation: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            ceiling: None,
            floor: None,
            allow_animation: true,
        }
    }
}

impl Policy {
    pub fn ceiling(mut self, level: Fidelity) -> Self {
        self.ceiling = Some(level);
        self
    }
    pub fn floor(mut self, level: Fidelity) -> Self {
        self.floor = Some(level);
        self
    }
    /// Apply the ceiling, then the floor.
    pub fn clamp(&self, level: Fidelity) -> Fidelity {
        let level = self.ceiling.map_or(level, |c| level.min(c));
        self.floor.map_or(level, |f| level.max(f))
    }
}

/// A renderable that renders natively at some fidelity levels.
pub trait Degradable {
    /// The levels [`render_at`](Self::render_at) supports.
    fn levels(&self) -> &[Fidelity];
    /// Render at `level`, one of [`levels`](Self::levels).
    fn render_at(
        &self,
        level: Fidelity,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Vec<Segment>;
}

/// Renders a [`Degradable`] at the best supported level not above the selected
/// one. When every supported level is above it, the lowest supported level is
/// rendered and then degraded generically.
pub struct Adaptive<T> {
    inner: T,
    level: Option<Fidelity>,
    policy: Policy,
}

impl<T: Degradable> Adaptive<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            level: None,
            policy: Policy::default(),
        }
    }
    /// Force the selected level instead of deriving it from the console.
    pub fn level(mut self, level: Fidelity) -> Self {
        self.level = Some(level);
        self
    }
    /// Limits applied when the level is derived from the console.
    pub fn policy(mut self, policy: Policy) -> Self {
        self.policy = policy;
        self
    }
    /// The level the console would get and the level actually rendered.
    pub fn resolve(&self, console: &Console) -> (Fidelity, Fidelity) {
        let selected = self
            .level
            .unwrap_or_else(|| Fidelity::for_console(console, &self.policy));
        let levels = self.inner.levels();
        let chosen = levels
            .iter()
            .copied()
            .filter(|l| *l <= selected)
            .max()
            .or_else(|| levels.iter().copied().min())
            .unwrap_or(selected);
        (selected, chosen)
    }
}

impl<T: Degradable> Renderable for Adaptive<T> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let (selected, chosen) = self.resolve(console);
        let segments = self.inner.render_at(chosen, console, options);
        if chosen > selected {
            degrade_segments(segments, selected)
        } else {
            segments
        }
    }
}

/// Degrade already-rendered segments to `level`.
pub fn degrade_segments(segments: Vec<Segment>, level: Fidelity) -> Vec<Segment> {
    match level {
        Fidelity::Animated | Fidelity::Rich => segments,
        Fidelity::Styled => strip_color(segments),
        Fidelity::Plain => strip_styles(segments),
        Fidelity::Ascii => ascii_fallback(strip_styles(segments)),
    }
}

/// Any renderable, made degradable by post-processing its segments.
pub struct Degrade<R> {
    inner: R,
    level: Option<Fidelity>,
    policy: Policy,
}

impl<R: Renderable> Degrade<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            level: None,
            policy: Policy::default(),
        }
    }
    /// Force a level instead of deriving it from the console.
    pub fn level(mut self, level: Fidelity) -> Self {
        self.level = Some(level);
        self
    }
    pub fn policy(mut self, policy: Policy) -> Self {
        self.policy = policy;
        self
    }
}

/// A borrowed renderable, so a `&dyn Renderable` can be wrapped.
pub struct Borrowed<'a>(pub &'a dyn Renderable);

impl Renderable for Borrowed<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.0.rich_render(console, options)
    }
    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        self.0.measure(console, options)
    }
}

impl<'a> Degrade<Borrowed<'a>> {
    /// Degrade a borrowed renderable.
    pub fn borrowed(inner: &'a dyn Renderable) -> Self {
        Self::new(Borrowed(inner))
    }
}

impl<R: Renderable> Degradable for Degrade<R> {
    fn levels(&self) -> &[Fidelity] {
        &Fidelity::ALL
    }
    fn render_at(
        &self,
        level: Fidelity,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Vec<Segment> {
        degrade_segments(self.inner.rich_render(console, options), level)
    }
}

impl<R: Renderable> Renderable for Degrade<R> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let level = self
            .level
            .unwrap_or_else(|| Fidelity::for_console(console, &self.policy));
        self.render_at(level, console, options)
    }
    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        self.inner.measure(console, options)
    }
}

/// Style attribute names by core index (`Style::attr`).
pub(crate) const ATTR_NAMES: [&str; 13] = [
    "bold",
    "dim",
    "italic",
    "underline",
    "blink",
    "blink2",
    "reverse",
    "conceal",
    "strike",
    "underline2",
    "frame",
    "encircle",
    "overline",
];

/// `style` with its colours removed; attributes and link are kept.
pub fn style_without_color(style: &Style) -> Style {
    let words: Vec<String> = ATTR_NAMES
        .iter()
        .enumerate()
        .filter_map(|(i, name)| match style.attr(i) {
            Some(true) => Some((*name).to_owned()),
            Some(false) => Some(format!("not {name}")),
            None => None,
        })
        .collect();
    let base = if words.is_empty() {
        Style::new()
    } else {
        Style::parse(&words.join(" ")).unwrap_or_default()
    };
    base.update_link(style.link().map(str::to_owned))
}

/// Remove foreground and background colours, keeping attributes and links.
pub fn strip_color(segments: Vec<Segment>) -> Vec<Segment> {
    segments
        .into_iter()
        .map(|mut s| {
            s.style = s
                .style
                .as_ref()
                .map(style_without_color)
                .filter(|st| !st.is_null());
            s
        })
        .collect()
}

/// Remove every style and drop control segments.
pub fn strip_styles(segments: Vec<Segment>) -> Vec<Segment> {
    segments
        .into_iter()
        .filter(|s| !s.control)
        .map(|mut s| {
            s.style = None;
            s
        })
        .collect()
}

/// Replace non-ASCII glyphs with ASCII of the same cell width: box drawing,
/// blocks, arrows, bullets, check/cross marks and common punctuation map to a
/// look-alike; anything else becomes `?` per cell.
pub fn ascii_fallback(segments: Vec<Segment>) -> Vec<Segment> {
    segments
        .into_iter()
        .map(|mut s| {
            if !s.control && !s.text.is_ascii() {
                s.text = ascii_text(&s.text);
            }
            s
        })
        .collect()
}

/// [`ascii_fallback`] for a string.
pub fn ascii_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
        } else if let Some(a) = ascii_char(c) {
            out.push(a);
            for _ in 1..char_cell_width(c) {
                out.push(' ');
            }
        } else {
            // Zero-width marks vanish; wide glyphs keep their width.
            for _ in 0..char_cell_width(c) {
                out.push('?');
            }
        }
    }
    out
}

/// The ASCII look-alike of one glyph, when there is one.
pub fn ascii_char(c: char) -> Option<char> {
    Some(match c {
        // Box drawing: horizontals, verticals, then every corner and junction.
        '─' | '━' | '┄' | '┅' | '┈' | '┉' | '╌' | '╍' | '╴' | '╶' | '╸' | '╺' | '╼' | '╾' => {
            '-'
        }
        '═' => '=',
        '│' | '┃' | '┆' | '┇' | '┊' | '┋' | '╎' | '╏' | '║' | '╵' | '╷' | '╹' | '╻' | '╽' | '╿' => {
            '|'
        }
        '╱' => '/',
        '╲' => '\\',
        '╳' => 'X',
        '\u{2500}'..='\u{257f}' => '+',
        // Blocks and shades.
        '░' => '.',
        '▒' => ':',
        '▓' | '█' => '#',
        '▁' | '▂' => '_',
        '\u{2580}'..='\u{259f}' => '#',
        // Braille (spinners, sparklines).
        '\u{2800}' => ' ',
        '\u{2801}'..='\u{28ff}' => '.',
        // Arrows and triangles.
        '→' | '⇒' | '⟶' | '➜' | '➔' | '▶' | '►' | '▸' | '❯' | '›' | '»' => '>',
        '←' | '⇐' | '⟵' | '◀' | '◄' | '◂' | '❮' | '‹' | '«' => '<',
        '↑' | '⇑' | '▲' | '▴' => '^',
        '↓' | '⇓' | '▼' | '▾' => 'v',
        '↔' | '⇔' => '-',
        '↷' | '↻' | '⟳' => '~',
        // Bullets and markers.
        '•' | '●' | '◉' | '▪' | '■' | '◆' | '★' | '∙' | '⁃' => '*',
        '○' | '◦' | '◯' | '□' | '◇' | '☐' => 'o',
        '·' | '…' | '⋯' => '.',
        '✓' | '✔' | '☑' | '✅' => 'v',
        '✗' | '✘' | '✕' | '✖' | '×' | '☒' | '❌' => 'x',
        '⚠' => '!',
        'ℹ' => 'i',
        // Punctuation.
        '‘' | '’' | '′' => '\'',
        '“' | '”' | '″' => '"',
        '‐' | '‑' | '‒' | '–' | '—' | '―' | '−' => '-',
        '\u{a0}' | '\u{2002}'..='\u{200a}' => ' ',
        _ => return None,
    })
}
