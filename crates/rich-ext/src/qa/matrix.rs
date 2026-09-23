//! A compatibility matrix: fixtures × capability profiles.
//!
//! [`run`] renders every fixture under every [`CapabilityProfile`] and
//! checks, per cell, with no approvals needed:
//!
//! * the render does not panic;
//! * no line is wider than the width;
//! * an ASCII profile's output is pure ASCII;
//! * a no-colour profile's output has no colour SGR codes (`30–49`,
//!   `90–107`, `38;…`, `48;…`);
//! * a no-hyperlink profile's output has no OSC 8.
//!
//! With [`Approvals`], each cell's output is also compared with its approved
//! file (key `fixture@<width>.<profile>`), exactly as screenshots are.
//!
//! [`CapabilityProfile::standard`] gives the 12 combinations of 16/256/
//! truecolor × unicode/ascii × hyperlinks on/off plus four named profiles:
//! `dumb` (no colour, ASCII, no links), `ci` (16 colours, unicode, no links,
//! not interactive), `windows-terminal` (truecolor, unicode, links) and
//! `screen-reader`, which renders through [`semantic_text`] under
//! [`AccessibilityPolicy::screen_reader`] (no colour, linear text).

use rich::cells::cell_len;
use rich::{Console, ConsoleOptions, Renderable, Segment, Table, Text};

use super::screenshot::{Approvals, Outcome, Shot};
use super::{panic_message, plain_lines, plural, table_then_line, NoHeight, Probe};
use crate::a11y::policy::{AccessibilityPolicy, Status, SymbolSet};
use crate::a11y::semantic_text;
use crate::capabilities::{ColorDepth, Overrides};
use crate::testing::RenderSnapshot;

/// A named set of capabilities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityProfile {
    pub name: String,
    /// Colour, unicode and hyperlinks come from here; unset fields mean
    /// truecolor, unicode and no links.
    pub overrides: Overrides,
    /// An accessibility policy; a screen-reader policy renders semantic text.
    pub policy: Option<AccessibilityPolicy>,
}

impl CapabilityProfile {
    pub fn new(name: impl Into<String>, overrides: Overrides) -> Self {
        CapabilityProfile {
            name: name.into(),
            overrides,
            policy: None,
        }
    }

    /// A combination profile, named e.g. `256-ascii-links`.
    pub fn combo(color: ColorDepth, unicode: bool, hyperlinks: bool) -> Self {
        let name = format!(
            "{}-{}-{}",
            color.name(),
            if unicode { "unicode" } else { "ascii" },
            if hyperlinks { "links" } else { "nolinks" }
        );
        CapabilityProfile::new(
            name,
            Overrides {
                color: Some(color),
                unicode: Some(unicode),
                hyperlinks: Some(hyperlinks),
                ..Overrides::default()
            },
        )
    }

    /// `dumb`: no colour, ASCII, no links.
    pub fn dumb() -> Self {
        CapabilityProfile::new(
            "dumb",
            Overrides {
                color: Some(ColorDepth::None),
                unicode: Some(false),
                hyperlinks: Some(false),
                interactive: Some(false),
                ..Overrides::default()
            },
        )
    }

    /// `ci`: 16 colours, unicode, no links, not interactive.
    pub fn ci() -> Self {
        CapabilityProfile::new(
            "ci",
            Overrides {
                color: Some(ColorDepth::Ansi16),
                unicode: Some(true),
                hyperlinks: Some(false),
                interactive: Some(false),
                ..Overrides::default()
            },
        )
    }

    /// `windows-terminal`: truecolor, unicode, links.
    pub fn windows_terminal() -> Self {
        CapabilityProfile::new(
            "windows-terminal",
            Overrides {
                color: Some(ColorDepth::TrueColor),
                unicode: Some(true),
                hyperlinks: Some(true),
                interactive: Some(true),
                ..Overrides::default()
            },
        )
    }

    /// `screen-reader`: semantic text, no colour, no links.
    pub fn screen_reader() -> Self {
        CapabilityProfile {
            policy: Some(AccessibilityPolicy::screen_reader()),
            ..CapabilityProfile::new(
                "screen-reader",
                Overrides {
                    color: Some(ColorDepth::None),
                    unicode: Some(true),
                    hyperlinks: Some(false),
                    ..Overrides::default()
                },
            )
        }
    }

    /// The 12 combinations, then `dumb`, `ci`, `windows-terminal` and
    /// `screen-reader`.
    pub fn standard() -> Vec<Self> {
        let mut out = Vec::new();
        for color in [
            ColorDepth::Ansi16,
            ColorDepth::Ansi256,
            ColorDepth::TrueColor,
        ] {
            for unicode in [true, false] {
                for links in [true, false] {
                    out.push(Self::combo(color, unicode, links));
                }
            }
        }
        out.extend([
            Self::dumb(),
            Self::ci(),
            Self::windows_terminal(),
            Self::screen_reader(),
        ]);
        out
    }

    pub fn color(&self) -> ColorDepth {
        self.overrides.color.unwrap_or(ColorDepth::TrueColor)
    }
    pub fn unicode(&self) -> bool {
        self.overrides.unicode.unwrap_or(true)
    }
    pub fn hyperlinks(&self) -> bool {
        self.overrides.hyperlinks.unwrap_or(false)
    }
    fn screen_reader_policy(&self) -> bool {
        self.policy.as_ref().is_some_and(|p| p.screen_reader)
    }
}

/// A named renderable factory.
pub type Fixture = (&'static str, fn() -> Box<dyn Renderable>);

/// One cell's verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CellStatus {
    Pass,
    /// Structural failures.
    Fail(Vec<String>),
    /// Differs from its approved output.
    Mismatch,
    /// No approved output yet.
    Missing,
}

/// One fixture under one profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub fixture: String,
    pub profile: String,
    pub status: CellStatus,
}

/// The grid [`run`] produces.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MatrixReport {
    pub width: usize,
    pub fixtures: Vec<String>,
    pub profiles: Vec<String>,
    /// Row-major: fixture by fixture, profiles in order.
    pub cells: Vec<Cell>,
    /// The approval outcome, when approvals were given.
    pub approvals: Option<Outcome>,
    /// How cells are marked; `None` picks unicode or ASCII from the console.
    pub symbols: Option<SymbolSet>,
}

impl MatrixReport {
    /// Every cell passed.
    pub fn is_ok(&self) -> bool {
        self.cells.iter().all(|c| c.status == CellStatus::Pass)
    }
    pub fn cell(&self, fixture: &str, profile: &str) -> Option<&Cell> {
        self.cells
            .iter()
            .find(|c| c.fixture == fixture && c.profile == profile)
    }
    /// Cells that did not pass.
    pub fn failures(&self) -> impl Iterator<Item = &Cell> {
        self.cells.iter().filter(|c| c.status != CellStatus::Pass)
    }
    pub fn symbols(mut self, set: SymbolSet) -> Self {
        self.symbols = Some(set);
        self
    }
}

/// Whether ANSI `text` contains a colour SGR parameter.
pub fn has_color_codes(text: &str) -> bool {
    let mut rest = text;
    while let Some(start) = rest.find("\x1b[") {
        let after = &rest[start + 2..];
        let end = after
            .find(|c: char| !(c.is_ascii_digit() || c == ';' || c == ':'))
            .unwrap_or(after.len());
        if after[end..].starts_with('m') {
            // Any parameter in a colour range: sub-parameters of 38/48 only
            // ever follow a colour parameter.
            let colour = after[..end].split([';', ':']).any(|p| {
                matches!(
                    p.parse::<u32>().unwrap_or(0),
                    30..=49 | 90..=97 | 100..=107
                )
            });
            if colour {
                return true;
            }
        }
        rest = &after[end..];
    }
    false
}

/// Render one fixture under one profile: `(shot, structural failures)`.
fn render_cell(
    name: &str,
    fixture: fn() -> Box<dyn Renderable>,
    profile: &CapabilityProfile,
    width: usize,
) -> (Option<Shot>, Vec<String>) {
    let mut probe = Probe::new(width);
    probe.color = profile.color();
    probe.unicode = profile.unicode();
    probe.hyperlinks = profile.hyperlinks();
    probe.height = Some(25);
    if let Some(policy) = &profile.policy {
        probe.theme = policy.theme(&probe.theme);
    }
    let target = probe.target();
    let key = format!("{name}@{width}.{}", profile.name);
    let screen_reader = profile.screen_reader_policy();
    let captured = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let renderable = fixture();
        if screen_reader {
            let text = Text::new(semantic_text(&*renderable, width));
            RenderSnapshot::capture(&target, &NoHeight(&text))
        } else {
            RenderSnapshot::capture(&target, &NoHeight(&*renderable))
        }
    }));
    let snapshot = match captured {
        Ok(s) => s,
        Err(payload) => return (None, vec![format!("panicked: {}", panic_message(payload))]),
    };
    let mut failures = Vec::new();
    let lines = plain_lines(&[Segment::new(snapshot.plain.clone(), None)]);
    if let Some((i, l)) = lines.iter().enumerate().find(|(_, l)| cell_len(l) > width) {
        failures.push(format!(
            "line {} is {} cells wide (width {width})",
            i + 1,
            cell_len(l)
        ));
    }
    if !profile.unicode() {
        let glyphs: String = snapshot
            .plain
            .chars()
            .filter(|c| !c.is_ascii())
            .take(8)
            .collect();
        if !glyphs.is_empty() {
            failures.push(format!("non-ASCII on an ASCII profile: {glyphs}"));
        }
    }
    if profile.color() == ColorDepth::None && has_color_codes(&snapshot.ansi) {
        failures.push("colour codes on a no-colour profile".into());
    }
    if !profile.hyperlinks() && snapshot.ansi.contains("\x1b]8;") {
        failures.push("OSC 8 hyperlinks on a no-hyperlink profile".into());
    }
    let shot = Shot::from_snapshot(name, key, snapshot, profile.color(), profile.unicode());
    (Some(shot), failures)
}

/// Run `fixtures` across `profiles` at `width`; see the [module docs](self).
pub fn run(
    fixtures: &[Fixture],
    profiles: &[CapabilityProfile],
    width: usize,
    approvals: Option<&Approvals>,
) -> std::io::Result<MatrixReport> {
    let mut report = MatrixReport {
        width,
        fixtures: fixtures.iter().map(|(n, _)| (*n).to_owned()).collect(),
        profiles: profiles.iter().map(|p| p.name.clone()).collect(),
        ..MatrixReport::default()
    };
    let mut shots = Vec::new();
    for (name, factory) in fixtures {
        for profile in profiles {
            let (shot, failures) = render_cell(name, *factory, profile, width);
            let status = if failures.is_empty() {
                CellStatus::Pass
            } else {
                CellStatus::Fail(failures)
            };
            report.cells.push(Cell {
                fixture: (*name).to_owned(),
                profile: profile.name.clone(),
                status,
            });
            if let Some(shot) = shot {
                shots.push(shot);
            }
        }
    }
    if let Some(approvals) = approvals {
        let outcome = approvals.check(&shots)?;
        for cell in &mut report.cells {
            if cell.status != CellStatus::Pass {
                continue;
            }
            let key = format!("{}@{width}.{}", cell.fixture, cell.profile);
            if outcome.missing.contains(&key) {
                cell.status = CellStatus::Missing;
            } else if outcome.mismatched.iter().any(|m| m.key == key) {
                cell.status = CellStatus::Mismatch;
            }
        }
        report.approvals = Some(outcome);
    }
    Ok(report)
}

/// Structural checks only: [`run`] without approvals.
pub fn regression(
    fixtures: &[Fixture],
    profiles: &[CapabilityProfile],
    width: usize,
) -> MatrixReport {
    run(fixtures, profiles, width, None).unwrap_or_default()
}

impl Renderable for MatrixReport {
    /// Profiles down the side, fixtures across (profiles usually outnumber
    /// fixtures), then each failure's reasons and a summary.
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let set = self.symbols.unwrap_or(if console.ascii_only() {
            SymbolSet::Ascii
        } else {
            SymbolSet::Unicode
        });
        let mark = |status: &CellStatus| -> &'static str {
            let status = match status {
                CellStatus::Pass => Status::Ok,
                CellStatus::Fail(_) | CellStatus::Mismatch => Status::Error,
                CellStatus::Missing => Status::Pending,
            };
            match set {
                SymbolSet::Unicode => match status {
                    Status::Ok => "✔",
                    Status::Error => "✖",
                    _ => "…",
                },
                other => status.symbol(other),
            }
        };
        let mut table = Table::new();
        table.add_column("Profile");
        for fixture in &self.fixtures {
            table.add_column(fixture.clone());
        }
        for profile in &self.profiles {
            let mut row = vec![Text::new(profile.clone())];
            for fixture in &self.fixtures {
                let cell = self.cell(fixture, profile);
                let text = match cell {
                    Some(c) => {
                        let style = match c.status {
                            CellStatus::Pass => "green",
                            CellStatus::Missing => "yellow",
                            _ => "bold red",
                        };
                        Text::styled(mark(&c.status), style)
                    }
                    None => Text::new(""),
                };
                row.push(text);
            }
            table.add_row_text(row);
        }
        let mut out = table.rich_render(console, options);
        if out.last().is_some_and(|s| !s.text.ends_with('\n')) {
            out.push(Segment::line());
        }
        let failures: Vec<&Cell> = self.failures().collect();
        let detail = (!failures.is_empty()).then(|| {
            let mut t = Table::new();
            for header in ["Fixture", "Profile", "Problem"] {
                t.add_column(header);
            }
            for c in &failures {
                let problem = match &c.status {
                    CellStatus::Fail(reasons) => reasons.join("; "),
                    CellStatus::Mismatch => "differs from approved output".into(),
                    CellStatus::Missing => "no approved output".into(),
                    CellStatus::Pass => String::new(),
                };
                t.add_row_text(vec![
                    Text::new(c.fixture.clone()),
                    Text::new(c.profile.clone()),
                    Text::new(problem),
                ]);
            }
            t
        });
        let summary = format!(
            "{} × {} at width {}: {} passed, {} failed",
            plural(self.fixtures.len(), "fixture"),
            plural(self.profiles.len(), "profile"),
            self.width,
            self.cells.len() - failures.len(),
            failures.len()
        );
        out.extend(table_then_line(detail, summary, console, options));
        out
    }
}
