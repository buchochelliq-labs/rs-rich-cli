//! Terminal capability detection with provenance. No probes: every answer is
//! derived from environment variables and tty facts supplied through an
//! [`Environment`], so detection is deterministic under a [`MapEnvironment`].
//!
//! # Detection rules
//!
//! Each field of a [`Report`] is decided by the first rule that matches, in
//! this order: an explicit [`Overrides`] value, a `RICH_*` override variable,
//! then the heuristics below.
//!
//! * **colour** — non-empty `NO_COLOR` → none; `FORCE_COLOR` (`0`/`false` →
//!   none, `1` → 16, `2` → 256, `3` → truecolor, other values → at least 16,
//!   ignoring the tty check); not a terminal → none, unless a CI provider whose
//!   log viewer renders ANSI is detected (GitHub/Gitea Actions → truecolor;
//!   GitLab, Buildkite, CircleCI, Travis, AppVeyor, Drone, Azure Pipelines,
//!   TeamCity → 16, after the `supports-color` package's table); `TERM=dumb` →
//!   none; `COLORTERM=truecolor|24bit` → truecolor; `WT_SESSION` (Windows
//!   Terminal), kitty, `TERM_PROGRAM` iTerm.app/WezTerm/vscode/ghostty or a
//!   `TERM` naming `direct`/`truecolor` → truecolor; `TERM_PROGRAM=Apple_Terminal`
//!   or a `TERM` containing `256` → 256; Windows → truecolor (as core does);
//!   anything else → 16.
//! * **unicode** — the first non-empty of `LC_ALL`, `LC_CTYPE`, `LANG`: UTF-8 →
//!   yes, `C`/`POSIX` → yes (Python coerces the C locale to UTF-8, which is
//!   what upstream rich then sees), any other explicit charset → no;
//!   `WT_SESSION` → yes; otherwise yes by default, matching core's
//!   `ascii_only(false)` default.
//! * **hyperlinks** (OSC 8) — only for terminals known to support them and only
//!   on a tty: iTerm2, WezTerm, kitty, Windows Terminal, VS Code, ghostty, foot,
//!   contour, DomTerm and VTE ≥ 0.50 (`VTE_VERSION` ≥ 5000: GNOME Terminal,
//!   Tilix, Terminator…). Inside tmux/screen, on `TERM=dumb` or anywhere else:
//!   no. Alacritty (≥ 0.11) supports them but does not publish its version, so
//!   it is left out on purpose — override with `RICH_HYPERLINKS=1`.
//! * **graphics** — on a tty only: kitty (`TERM=xterm-kitty` or
//!   `KITTY_WINDOW_ID`) → kitty; `TERM_PROGRAM` iTerm.app or WezTerm → iTerm
//!   inline images; else the Sixel heuristic → sixel. **sixel** is reported
//!   separately (WezTerm speaks both): `WT_SESSION`, a `TERM` containing
//!   `sixel`/`mlterm`/`foot`, or `TERM_PROGRAM` wezterm/mintty. This is the
//!   same heuristic `rich_art::sixel::is_probably_supported` uses, which can
//!   later call into this module instead.
//! * **width / height** — `RICH_WIDTH`/`RICH_HEIGHT`, then `COLUMNS`/`LINES`,
//!   then the terminal's reported size, then 80×25.
//! * **interactive** — stdout is a terminal.
//! * **animation** — interactive, not CI, not `TERM=dumb`, and no
//!   reduced-motion preference (`RICH_A11Y` containing `reduced-motion`,
//!   `no-animation` or `screen-reader`).
//!
//! # Override variables
//!
//! `RICH_COLOR=none|16|256|truecolor`, `RICH_UNICODE=0|1`,
//! `RICH_HYPERLINKS=0|1`, `RICH_GRAPHICS=none|sixel|kitty|iterm`,
//! `RICH_SIXEL=0|1` (the CLI's existing variable, kept as an alias: `1` means
//! sixel graphics, `0` means no sixel), `RICH_ANIMATION=0|1`, `RICH_WIDTH`,
//! `RICH_HEIGHT`. Booleans also accept `true/false/yes/no/on/off`. Invalid values
//! are ignored and listed in [`Report::warnings`].

use crate::target::{CapabilityOrigin, DetectedCapabilities, TargetObservations};
use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Console, ConsoleOptions, Renderable, Segment, Table, Text};
use std::collections::BTreeMap;
use std::io::IsTerminal;

/// Environment facts detection reads. Implement it to feed detection from
/// anywhere; [`SystemEnvironment`] reads the process, [`MapEnvironment`] a map.
pub trait Environment {
    /// An environment variable, `None` when unset or not Unicode.
    fn var(&self, name: &str) -> Option<String>;
    /// Whether stdout is a terminal.
    fn is_terminal(&self) -> bool;
    /// The terminal size in cells `(width, height)`, when known.
    fn size(&self) -> Option<(usize, usize)>;
    /// Whether this is Windows (core assumes truecolor there).
    fn is_windows(&self) -> bool {
        false
    }
}

/// The real process environment: `std::env`, stdout's tty status and the
/// size core's console detects.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemEnvironment;

impl Environment for SystemEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
    fn is_terminal(&self) -> bool {
        std::io::stdout().is_terminal()
    }
    fn size(&self) -> Option<(usize, usize)> {
        // Core falls back to 80×25 when the size is unknown; only a terminal
        // has a size worth reporting.
        if !self.is_terminal() {
            return None;
        }
        let console = Console::builder().force_terminal(true).build();
        Some((console.width(), console.height()))
    }
    fn is_windows(&self) -> bool {
        cfg!(windows)
    }
}

/// A deterministic environment for tests and CI.
#[derive(Clone, Debug, Default)]
pub struct MapEnvironment {
    pub vars: BTreeMap<String, String>,
    pub terminal: bool,
    pub size: Option<(usize, usize)>,
    pub windows: bool,
}

impl MapEnvironment {
    /// An empty, non-terminal environment.
    pub fn new() -> Self {
        Self::default()
    }
    /// An empty terminal environment.
    pub fn tty() -> Self {
        Self {
            terminal: true,
            ..Self::default()
        }
    }
    /// Set a variable.
    pub fn var(mut self, name: &str, value: &str) -> Self {
        self.vars.insert(name.into(), value.into());
        self
    }
    /// Set the terminal flag.
    pub fn terminal(mut self, terminal: bool) -> Self {
        self.terminal = terminal;
        self
    }
    /// Set the reported size.
    pub fn size(mut self, width: usize, height: usize) -> Self {
        self.size = Some((width, height));
        self
    }
    /// Mark the environment as Windows.
    pub fn windows(mut self, windows: bool) -> Self {
        self.windows = windows;
        self
    }
}

impl Environment for MapEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }
    fn is_terminal(&self) -> bool {
        self.terminal
    }
    fn size(&self) -> Option<(usize, usize)> {
        self.size
    }
    fn is_windows(&self) -> bool {
        self.windows
    }
}

/// Where a capability value came from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Origin {
    /// An explicit [`Overrides`] value.
    Override,
    /// An environment variable, named.
    Environment(String),
    /// Nothing said anything; the documented default.
    Default,
    /// Derived from other facts (tty status, terminal identity, platform).
    Inferred,
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Origin::Override => f.write_str("override"),
            Origin::Environment(name) => write!(f, "environment ({name})"),
            Origin::Default => f.write_str("default"),
            Origin::Inferred => f.write_str("inferred"),
        }
    }
}

/// A value with its provenance and a human reason (`"COLORTERM=truecolor"`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Field<T> {
    pub value: T,
    pub origin: Origin,
    pub reason: String,
}

impl<T> Field<T> {
    fn new(value: T, origin: Origin, reason: impl Into<String>) -> Self {
        Self {
            value,
            origin,
            reason: reason.into(),
        }
    }
}

/// Colour depth, ordered from none to truecolor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ColorDepth {
    None,
    Ansi16,
    Ansi256,
    TrueColor,
}

impl ColorDepth {
    /// The core colour system, `None` for no colour.
    pub fn color_system(self) -> Option<ColorSystem> {
        match self {
            ColorDepth::None => None,
            ColorDepth::Ansi16 => Some(ColorSystem::Standard),
            ColorDepth::Ansi256 => Some(ColorSystem::EightBit),
            ColorDepth::TrueColor => Some(ColorSystem::Truecolor),
        }
    }
    /// The depth of a core colour system.
    pub fn from_color_system(system: Option<ColorSystem>) -> Self {
        match system {
            None => ColorDepth::None,
            Some(ColorSystem::Standard | ColorSystem::Windows) => ColorDepth::Ansi16,
            Some(ColorSystem::EightBit) => ColorDepth::Ansi256,
            Some(ColorSystem::Truecolor) => ColorDepth::TrueColor,
        }
    }
    /// Parse `none|16|256|truecolor` (also `0`, `24bit`, `8bit`).
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" | "0" | "no" | "off" | "false" => Some(ColorDepth::None),
            "16" | "standard" | "ansi" => Some(ColorDepth::Ansi16),
            "256" | "8bit" | "eightbit" => Some(ColorDepth::Ansi256),
            "truecolor" | "24bit" | "16m" => Some(ColorDepth::TrueColor),
            _ => None,
        }
    }
    /// `none`, `16`, `256` or `truecolor`.
    pub fn name(self) -> &'static str {
        match self {
            ColorDepth::None => "none",
            ColorDepth::Ansi16 => "16",
            ColorDepth::Ansi256 => "256",
            ColorDepth::TrueColor => "truecolor",
        }
    }
}

/// An inline graphics protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Graphics {
    None,
    Sixel,
    Kitty,
    Iterm,
}

impl Graphics {
    /// Parse `none|sixel|kitty|iterm`.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" | "0" | "no" | "off" | "false" => Some(Graphics::None),
            "sixel" => Some(Graphics::Sixel),
            "kitty" => Some(Graphics::Kitty),
            "iterm" | "iterm2" => Some(Graphics::Iterm),
            _ => None,
        }
    }
    /// `none`, `sixel`, `kitty` or `iterm`.
    pub fn name(self) -> &'static str {
        match self {
            Graphics::None => "none",
            Graphics::Sixel => "sixel",
            Graphics::Kitty => "kitty",
            Graphics::Iterm => "iterm",
        }
    }
}

/// Caller overrides, applied last with [`Origin::Override`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Overrides {
    pub color: Option<ColorDepth>,
    pub unicode: Option<bool>,
    pub hyperlinks: Option<bool>,
    pub graphics: Option<Graphics>,
    pub sixel: Option<bool>,
    pub animation: Option<bool>,
    pub interactive: Option<bool>,
    pub width: Option<usize>,
    pub height: Option<usize>,
}

/// The documented override variables, in report order.
pub const OVERRIDE_VARS: &[&str] = &[
    "RICH_COLOR",
    "RICH_UNICODE",
    "RICH_HYPERLINKS",
    "RICH_GRAPHICS",
    "RICH_SIXEL",
    "RICH_ANIMATION",
    "RICH_WIDTH",
    "RICH_HEIGHT",
];

/// Parse a boolean override (`0|1|true|false|yes|no|on|off`).
pub fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// A detection result. Every field records its provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Report {
    pub color: Field<ColorDepth>,
    pub unicode: Field<bool>,
    pub hyperlinks: Field<bool>,
    pub graphics: Field<Graphics>,
    pub sixel: Field<bool>,
    pub width: Field<usize>,
    pub height: Field<usize>,
    pub interactive: Field<bool>,
    pub animation: Field<bool>,
    /// The identified terminal (`"iTerm.app"`, `"Windows Terminal"`…), if any.
    pub terminal: Option<String>,
    /// The identified CI provider, if any.
    pub ci: Option<String>,
    /// Override values that could not be parsed.
    pub warnings: Vec<String>,
}

/// Capability detection. See the [module docs](self) for the rules.
#[derive(Clone, Copy, Debug, Default)]
pub struct Capabilities;

const CI_PROVIDERS: &[(&str, &str, ColorDepth)] = &[
    ("GITHUB_ACTIONS", "GitHub Actions", ColorDepth::TrueColor),
    ("GITEA_ACTIONS", "Gitea Actions", ColorDepth::TrueColor),
    ("GITLAB_CI", "GitLab CI", ColorDepth::Ansi16),
    ("BUILDKITE", "Buildkite", ColorDepth::Ansi16),
    ("CIRCLECI", "CircleCI", ColorDepth::Ansi16),
    ("TRAVIS", "Travis CI", ColorDepth::Ansi16),
    ("APPVEYOR", "AppVeyor", ColorDepth::Ansi16),
    ("DRONE", "Drone", ColorDepth::Ansi16),
    ("TF_BUILD", "Azure Pipelines", ColorDepth::Ansi16),
    ("TEAMCITY_VERSION", "TeamCity", ColorDepth::Ansi16),
    ("JENKINS_URL", "Jenkins", ColorDepth::None),
    (
        "BITBUCKET_BUILD_NUMBER",
        "Bitbucket Pipelines",
        ColorDepth::None,
    ),
];

struct Probe<'a> {
    env: &'a dyn Environment,
}

impl Probe<'_> {
    fn get(&self, name: &str) -> Option<String> {
        self.env.var(name).filter(|v| !v.is_empty())
    }
    fn term(&self) -> String {
        self.get("TERM").unwrap_or_default().to_ascii_lowercase()
    }
    fn program(&self) -> String {
        self.get("TERM_PROGRAM").unwrap_or_default()
    }
    fn kitty(&self) -> Option<String> {
        if self.term() == "xterm-kitty" {
            Some("TERM=xterm-kitty".into())
        } else {
            self.get("KITTY_WINDOW_ID")
                .map(|_| "KITTY_WINDOW_ID".into())
        }
    }
    fn ci(&self) -> Option<(&'static str, &'static str, ColorDepth)> {
        CI_PROVIDERS
            .iter()
            .find(|(var, _, _)| self.get(var).is_some_and(|v| v != "false"))
            .copied()
            .or_else(|| {
                self.get("CI")
                    .filter(|v| !matches!(v.to_ascii_lowercase().as_str(), "0" | "false"))
                    .map(|_| ("CI", "CI", ColorDepth::None))
            })
    }
    fn terminal(&self) -> Option<String> {
        if self.get("WT_SESSION").is_some() {
            return Some("Windows Terminal".into());
        }
        if self.kitty().is_some() {
            return Some("kitty".into());
        }
        if let Some(p) = self.get("TERM_PROGRAM") {
            return Some(p);
        }
        self.get("VTE_VERSION").map(|v| format!("VTE {v}"))
    }
    fn sixel(&self) -> Option<String> {
        if self.get("WT_SESSION").is_some() {
            return Some("WT_SESSION".into());
        }
        let term = self.term();
        if ["sixel", "mlterm", "foot"].iter().any(|t| term.contains(t)) {
            return Some(format!("TERM={}", self.get("TERM").unwrap_or_default()));
        }
        let program = self.program().to_ascii_lowercase();
        if program.contains("wezterm") || program.contains("mintty") {
            return Some(format!("TERM_PROGRAM={}", self.program()));
        }
        None
    }
    fn reduced_motion(&self) -> Option<String> {
        let list = self.get("RICH_A11Y")?;
        list.split(',')
            .map(|item| item.trim().to_ascii_lowercase())
            .find(|item| {
                matches!(
                    item.as_str(),
                    "reduced-motion" | "no-animation" | "screen-reader"
                )
            })
            .map(|item| format!("RICH_A11Y={item}"))
    }
}

fn from_var(name: &str) -> Origin {
    Origin::Environment(name.into())
}

impl Capabilities {
    /// Detect from `env` with no caller overrides.
    pub fn detect(env: &dyn Environment) -> Report {
        Self::detect_with(env, &Overrides::default())
    }

    /// Detect from the real process environment.
    pub fn system() -> Report {
        Self::detect(&SystemEnvironment)
    }

    /// Detect from `env`, then apply `overrides`.
    pub fn detect_with(env: &dyn Environment, overrides: &Overrides) -> Report {
        let p = Probe { env };
        let mut warnings = Vec::new();
        let tty = env.is_terminal();
        let term = p.term();
        let program = p.program();
        let ci = p.ci();

        // Override variables, parsed once.
        fn parsed<T>(
            p: &Probe<'_>,
            name: &str,
            parse: impl Fn(&str) -> Option<T>,
            warnings: &mut Vec<String>,
        ) -> Option<Field<T>> {
            let raw = p.get(name)?;
            match parse(&raw) {
                Some(value) => Some(Field::new(value, from_var(name), format!("{name}={raw}"))),
                None => {
                    warnings.push(format!("ignored {name}={raw:?}: unrecognised value"));
                    None
                }
            }
        }
        let size = |v: &str| v.trim().parse::<usize>().ok().filter(|v| *v > 0);

        // Colour.
        let color = parsed(&p, "RICH_COLOR", ColorDepth::parse, &mut warnings)
            .unwrap_or_else(|| detect_color(&p, tty, &term, &program, ci, env.is_windows()));

        // Unicode.
        let unicode = parsed(&p, "RICH_UNICODE", parse_bool, &mut warnings).unwrap_or_else(|| {
            for name in ["LC_ALL", "LC_CTYPE", "LANG"] {
                if let Some(value) = p.get(name) {
                    let lower = value.to_ascii_lowercase();
                    let charset = lower.split('@').next().unwrap_or("");
                    let reason = format!("{name}={value}");
                    return if charset.contains("utf-8") || charset.contains("utf8") {
                        Field::new(true, from_var(name), reason)
                    } else if matches!(charset, "c" | "posix") {
                        Field::new(true, from_var(name), format!("{reason} (coerced to UTF-8)"))
                    } else {
                        Field::new(false, from_var(name), format!("{reason} is not UTF-8"))
                    };
                }
            }
            if p.get("WT_SESSION").is_some() {
                return Field::new(true, from_var("WT_SESSION"), "Windows Terminal");
            }
            Field::new(true, Origin::Default, "no locale set; UTF-8 assumed")
        });

        // Hyperlinks.
        let hyperlinks = parsed(&p, "RICH_HYPERLINKS", parse_bool, &mut warnings)
            .unwrap_or_else(|| detect_hyperlinks(&p, tty, &term, &program));

        // Graphics and sixel.
        let sixel_alias = parsed(&p, "RICH_SIXEL", parse_bool, &mut warnings);
        let graphics_var = parsed(&p, "RICH_GRAPHICS", Graphics::parse, &mut warnings);
        let sixel = if let Some(g) = &graphics_var {
            Field::new(
                g.value == Graphics::Sixel,
                g.origin.clone(),
                g.reason.clone(),
            )
        } else if let Some(s) = &sixel_alias {
            s.clone()
        } else if !tty {
            Field::new(false, Origin::Inferred, "stdout is not a terminal")
        } else if let Some(reason) = p.sixel() {
            let origin = from_var(reason.split('=').next().unwrap_or(&reason));
            Field::new(true, origin, reason)
        } else {
            Field::new(
                false,
                Origin::Default,
                "no sixel-capable terminal identified",
            )
        };
        let graphics = if let Some(g) = graphics_var {
            g
        } else if let Some(s) = sixel_alias {
            let value = if s.value {
                Graphics::Sixel
            } else {
                Graphics::None
            };
            Field::new(value, s.origin, s.reason)
        } else if !tty {
            Field::new(Graphics::None, Origin::Inferred, "stdout is not a terminal")
        } else if let Some(reason) = p.kitty() {
            let origin = from_var(reason.split('=').next().unwrap_or(&reason));
            Field::new(Graphics::Kitty, origin, reason)
        } else if program == "iTerm.app" || program.eq_ignore_ascii_case("wezterm") {
            Field::new(
                Graphics::Iterm,
                from_var("TERM_PROGRAM"),
                format!("TERM_PROGRAM={program}"),
            )
        } else if sixel.value {
            Field::new(Graphics::Sixel, sixel.origin.clone(), sixel.reason.clone())
        } else {
            Field::new(
                Graphics::None,
                Origin::Default,
                "no graphics protocol identified",
            )
        };

        // Dimensions.
        let dimension =
            |rich: &str, var: &str, index: usize, default: usize, w: &mut Vec<String>| {
                parsed(&p, rich, size, w)
                    .or_else(|| parsed(&p, var, size, w))
                    .unwrap_or_else(|| match env.size() {
                        Some(s) => {
                            let v = if index == 0 { s.0 } else { s.1 };
                            Field::new(v, Origin::Inferred, "terminal size")
                        }
                        None => Field::new(default, Origin::Default, format!("default {default}")),
                    })
            };
        let width = dimension("RICH_WIDTH", "COLUMNS", 0, 80, &mut warnings);
        let height = dimension("RICH_HEIGHT", "LINES", 1, 25, &mut warnings);

        let interactive = if tty {
            Field::new(true, Origin::Inferred, "stdout is a terminal")
        } else {
            Field::new(false, Origin::Inferred, "stdout is not a terminal")
        };

        let mut report = Report {
            color,
            unicode,
            hyperlinks,
            graphics,
            sixel,
            width,
            height,
            interactive,
            animation: Field::new(false, Origin::Default, ""),
            terminal: p.terminal(),
            ci: ci.map(|(_, name, _)| name.to_owned()),
            warnings,
        };
        report.apply(overrides);
        let animation_var = parsed(&p, "RICH_ANIMATION", parse_bool, &mut report.warnings);
        report.animation = match (overrides.animation, animation_var) {
            (Some(v), _) => Field::new(v, Origin::Override, "override"),
            (None, Some(field)) => field,
            (None, None) => {
                if !report.interactive.value {
                    Field::new(false, Origin::Inferred, "not interactive")
                } else if let Some((var, name, _)) = ci {
                    Field::new(false, from_var(var), format!("running in {name}"))
                } else if term == "dumb" {
                    Field::new(false, from_var("TERM"), "TERM=dumb")
                } else if let Some(reason) = p.reduced_motion() {
                    Field::new(false, from_var("RICH_A11Y"), reason)
                } else {
                    Field::new(true, Origin::Inferred, "interactive terminal")
                }
            }
        };
        report.escape_controls();
        report
    }
}

/// `text` with control and bidi characters written as escapes (`\u{1b}`),
/// so an environment value quoted in a reason is inert wherever it is shown.
fn escape_controls(text: &str) -> String {
    if !text
        .chars()
        .any(|c| c.is_control() || crate::sanitize::is_bidi_control(c))
    {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if c.is_control() || crate::sanitize::is_bidi_control(c) {
            out.push_str(&format!("\\u{{{:x}}}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

impl Report {
    /// Escape the environment values that reasons and facts quote.
    fn escape_controls(&mut self) {
        for reason in [
            &mut self.color.reason,
            &mut self.unicode.reason,
            &mut self.hyperlinks.reason,
            &mut self.graphics.reason,
            &mut self.sixel.reason,
            &mut self.width.reason,
            &mut self.height.reason,
            &mut self.interactive.reason,
            &mut self.animation.reason,
        ] {
            *reason = escape_controls(reason);
        }
        for warning in &mut self.warnings {
            *warning = escape_controls(warning);
        }
        for value in [&mut self.terminal, &mut self.ci].into_iter().flatten() {
            *value = escape_controls(value);
        }
    }
}

fn detect_color(
    p: &Probe<'_>,
    tty: bool,
    term: &str,
    program: &str,
    ci: Option<(&str, &str, ColorDepth)>,
    windows: bool,
) -> Field<ColorDepth> {
    use ColorDepth::*;
    if let Some(v) = p.get("NO_COLOR") {
        return Field::new(None, from_var("NO_COLOR"), format!("NO_COLOR={v}"));
    }
    // An empty FORCE_COLOR is ignored, as force-color.org specifies.
    let force = p.get("FORCE_COLOR");
    if let Some(force) = &force {
        let reason = format!("FORCE_COLOR={force}");
        match force.trim().to_ascii_lowercase().as_str() {
            "0" | "false" => return Field::new(None, from_var("FORCE_COLOR"), reason),
            "1" => return Field::new(Ansi16, from_var("FORCE_COLOR"), reason),
            "2" => return Field::new(Ansi256, from_var("FORCE_COLOR"), reason),
            "3" => return Field::new(TrueColor, from_var("FORCE_COLOR"), reason),
            _ => {}
        }
    }
    if !tty && force.is_none() {
        if let Some((var, name, depth)) = ci.filter(|c| c.2 != None) {
            return Field::new(depth, from_var(var), format!("{name} renders ANSI logs"));
        }
        return Field::new(None, Origin::Inferred, "stdout is not a terminal");
    }
    if term == "dumb" {
        return Field::new(None, from_var("TERM"), "TERM=dumb");
    }
    let terminal = detect_terminal_depth(p, term, program, windows);
    if let Some(force) = force {
        // FORCE_COLOR with no level: at least 16 colours.
        let depth = terminal.value.max(Ansi16);
        let reason = if terminal.value >= Ansi16 && terminal.origin != Origin::Default {
            terminal.reason
        } else {
            format!("FORCE_COLOR={force}")
        };
        return Field::new(depth, from_var("FORCE_COLOR"), reason);
    }
    terminal
}

fn detect_terminal_depth(
    p: &Probe<'_>,
    term: &str,
    program: &str,
    windows: bool,
) -> Field<ColorDepth> {
    use ColorDepth::*;
    if let Some(v) = p.get("COLORTERM") {
        let lower = v.to_ascii_lowercase();
        if lower.contains("truecolor") || lower.contains("24bit") {
            return Field::new(TrueColor, from_var("COLORTERM"), format!("COLORTERM={v}"));
        }
    }
    if p.get("WT_SESSION").is_some() {
        return Field::new(TrueColor, from_var("WT_SESSION"), "Windows Terminal");
    }
    if let Some(reason) = p.kitty() {
        let origin = from_var(reason.split('=').next().unwrap_or(&reason));
        return Field::new(TrueColor, origin, reason);
    }
    if ["iTerm.app", "WezTerm", "vscode", "ghostty"]
        .iter()
        .any(|t| program.eq_ignore_ascii_case(t))
    {
        return Field::new(
            TrueColor,
            from_var("TERM_PROGRAM"),
            format!("TERM_PROGRAM={program}"),
        );
    }
    let raw_term = p.get("TERM").unwrap_or_default();
    if term.contains("direct") || term.contains("truecolor") {
        return Field::new(TrueColor, from_var("TERM"), format!("TERM={raw_term}"));
    }
    if program == "Apple_Terminal" {
        return Field::new(
            Ansi256,
            from_var("TERM_PROGRAM"),
            "TERM_PROGRAM=Apple_Terminal",
        );
    }
    if term.contains("256") {
        return Field::new(Ansi256, from_var("TERM"), format!("TERM={raw_term}"));
    }
    if windows {
        return Field::new(TrueColor, Origin::Inferred, "Windows console");
    }
    if !term.is_empty() {
        return Field::new(Ansi16, from_var("TERM"), format!("TERM={raw_term}"));
    }
    Field::new(Ansi16, Origin::Default, "terminal with no TERM")
}

fn detect_hyperlinks(p: &Probe<'_>, tty: bool, term: &str, program: &str) -> Field<bool> {
    if !tty {
        return Field::new(false, Origin::Inferred, "stdout is not a terminal");
    }
    if term == "dumb" {
        return Field::new(false, from_var("TERM"), "TERM=dumb");
    }
    if p.get("TMUX").is_some() || term.starts_with("screen") || term.starts_with("tmux") {
        let origin = if p.get("TMUX").is_some() {
            from_var("TMUX")
        } else {
            from_var("TERM")
        };
        return Field::new(false, origin, "inside a multiplexer");
    }
    if p.get("WT_SESSION").is_some() {
        return Field::new(true, from_var("WT_SESSION"), "Windows Terminal");
    }
    if let Some(reason) = p.kitty() {
        let origin = from_var(reason.split('=').next().unwrap_or(&reason));
        return Field::new(true, origin, reason);
    }
    if [
        "iTerm.app",
        "WezTerm",
        "vscode",
        "ghostty",
        "contour",
        "DomTerm",
    ]
    .iter()
    .any(|t| program.eq_ignore_ascii_case(t))
    {
        return Field::new(
            true,
            from_var("TERM_PROGRAM"),
            format!("TERM_PROGRAM={program}"),
        );
    }
    if let Some(v) = p.get("VTE_VERSION") {
        let ok = v.trim().parse::<u32>().is_ok_and(|n| n >= 5000);
        let reason = format!("VTE_VERSION={v}");
        return Field::new(ok, from_var("VTE_VERSION"), reason);
    }
    if ["foot", "contour", "xterm-ghostty"]
        .iter()
        .any(|t| term.starts_with(t))
    {
        return Field::new(true, from_var("TERM"), format!("TERM={term}"));
    }
    if p.get("DOMTERM").is_some() {
        return Field::new(true, from_var("DOMTERM"), "DOMTERM");
    }
    Field::new(
        false,
        Origin::Default,
        "terminal not known to support OSC 8",
    )
}

impl Report {
    /// Apply caller overrides with [`Origin::Override`].
    pub fn apply(&mut self, o: &Overrides) {
        fn set<T>(field: &mut Field<T>, value: Option<T>) {
            if let Some(value) = value {
                *field = Field::new(value, Origin::Override, "override");
            }
        }
        set(&mut self.color, o.color);
        set(&mut self.unicode, o.unicode);
        set(&mut self.hyperlinks, o.hyperlinks);
        if let Some(g) = o.graphics {
            set(&mut self.graphics, Some(g));
            if o.sixel.is_none() {
                set(&mut self.sixel, Some(g == Graphics::Sixel));
            }
        }
        set(&mut self.sixel, o.sixel);
        set(&mut self.animation, o.animation);
        set(&mut self.interactive, o.interactive);
        set(&mut self.width, o.width);
        set(&mut self.height, o.height);
    }

    /// The core capabilities this report describes, for a
    /// [`RenderTarget`](crate::target::RenderTarget). Sixel is `Confirmed` only
    /// when the user said so (an override or a `RICH_*` variable).
    pub fn to_target_capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            width: self.width.value,
            height: self.height.value,
            color_system: self.color.value.color_system(),
            interactive: self.interactive.value,
            unicode: self.unicode.value,
            hyperlinks: self.hyperlinks.value,
            sixel: self.sixel_support(),
        }
    }

    fn sixel_support(&self) -> Support {
        match (&self.sixel.value, &self.sixel.origin) {
            (false, _) => Support::Unsupported,
            (true, Origin::Override) => Support::Confirmed,
            (true, Origin::Environment(name)) if name.starts_with("RICH_") => Support::Confirmed,
            (true, _) => Support::Inferred,
        }
    }

    /// The observations [`resolve_capabilities`](crate::target::resolve_capabilities)
    /// takes, so the older API can be fed from this one.
    pub fn observations(&self) -> TargetObservations {
        TargetObservations {
            width: Some(self.width.value),
            height: Some(self.height.value),
            is_terminal: self.interactive.value,
            color_system: self.color.value.color_system(),
            unicode: self.unicode.value,
            hyperlinks: self.hyperlinks.value,
            sixel_hint: self.sixel_support(),
        }
    }

    /// This report in the older [`DetectedCapabilities`] shape.
    pub fn to_detected(&self) -> DetectedCapabilities {
        fn origin(o: &Origin) -> CapabilityOrigin {
            match o {
                Origin::Override => CapabilityOrigin::Configured,
                Origin::Environment(name) if name.starts_with("RICH_") => {
                    CapabilityOrigin::Configured
                }
                Origin::Environment(_) => CapabilityOrigin::Detected,
                Origin::Default => CapabilityOrigin::Default,
                Origin::Inferred => CapabilityOrigin::Inferred,
            }
        }
        let origins = [
            ("width", &self.width.origin),
            ("height", &self.height.origin),
            ("color_system", &self.color.origin),
            ("interactive", &self.interactive.origin),
            ("unicode", &self.unicode.origin),
            ("hyperlinks", &self.hyperlinks.origin),
            ("sixel", &self.sixel.origin),
        ]
        .into_iter()
        .map(|(name, o)| (name.to_owned(), origin(o)))
        .collect();
        DetectedCapabilities {
            capabilities: self.to_target_capabilities(),
            origins,
        }
    }

    /// `(capability, value, origin, reason)` rows in a fixed order.
    pub fn rows(&self) -> Vec<(&'static str, String, &Origin, &str)> {
        let yes = |b: bool| if b { "yes" } else { "no" }.to_owned();
        vec![
            (
                "color",
                self.color.value.name().into(),
                &self.color.origin,
                &self.color.reason,
            ),
            (
                "unicode",
                yes(self.unicode.value),
                &self.unicode.origin,
                &self.unicode.reason,
            ),
            (
                "hyperlinks",
                yes(self.hyperlinks.value),
                &self.hyperlinks.origin,
                &self.hyperlinks.reason,
            ),
            (
                "graphics",
                self.graphics.value.name().into(),
                &self.graphics.origin,
                &self.graphics.reason,
            ),
            (
                "sixel",
                yes(self.sixel.value),
                &self.sixel.origin,
                &self.sixel.reason,
            ),
            (
                "width",
                self.width.value.to_string(),
                &self.width.origin,
                &self.width.reason,
            ),
            (
                "height",
                self.height.value.to_string(),
                &self.height.origin,
                &self.height.reason,
            ),
            (
                "interactive",
                yes(self.interactive.value),
                &self.interactive.origin,
                &self.interactive.reason,
            ),
            (
                "animation",
                yes(self.animation.value),
                &self.animation.origin,
                &self.animation.reason,
            ),
        ]
    }
}

/// A table of a [`Report`]: capability | value | source.
pub struct CapabilityReport<'a> {
    report: &'a Report,
}

impl<'a> CapabilityReport<'a> {
    pub fn new(report: &'a Report) -> Self {
        Self { report }
    }
}

impl Renderable for CapabilityReport<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut table = Table::new();
        table.add_column("Capability");
        table.add_column("Value");
        table.add_column("Source");
        for (name, value, origin, reason) in self.report.rows() {
            let source = if reason.is_empty() || reason == "override" {
                origin.to_string()
            } else {
                format!("{origin}: {reason}")
            };
            // Values come from the environment, so they are data, not markup.
            table.add_row_text(vec![Text::new(name), Text::new(value), Text::new(source)]);
        }
        let facts = [("terminal", &self.report.terminal), ("ci", &self.report.ci)];
        for (name, value) in facts {
            if let Some(value) = value {
                table.add_row_text(vec![
                    Text::new(name),
                    Text::new(value.as_str()),
                    Text::new("inferred"),
                ]);
            }
        }
        for warning in &self.report.warnings {
            table.add_row_text(vec![
                Text::new("warning"),
                Text::new(""),
                Text::new(warning.as_str()),
            ]);
        }
        table.rich_render(console, options)
    }
}
